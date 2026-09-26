use async_channel as mpmc;
use itertools::Itertools;
use speedd_codecs::{
    camera::Camera, plate::PlateRecord, server::TicketRecord, Limit, Mile, Road, Timestamp,
    SECONDS_PER_DAY,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use tokio::sync::{mpsc, oneshot};

/// A message for the [`Collector`].
#[derive(Debug)]
pub enum Message {
    /// A camera observed a plate.
    Plate { record: PlateRecord, camera: Camera },

    /// A dispatcher wants to receive the tickets for a road.
    Subscribe {
        road: Road,
        callback: oneshot::Sender<mpmc::Receiver<TicketRecord>>,
    },
}

/// Keeps records of the samples taken, such as observed speed measurements, ticketed days, and speed limits.
///
/// The collector *is* this data. The ticket queues are runtime resources, so they belong to the event loop:
/// one mpmc (tx, rx) pair per road.
/// The rx is kept for cloning it into new dispatchers which register for a specific road.
/// The tx is used to dispatch tickets, making use of the work-stealing behaviour of the mpmc channel:
/// if there are no dispatchers for a given road, the mpmc channel acts as a temporary queue, and
/// if there are one or more registered dispatchers, only one of them gets the ticket.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Collector {
    records: HashMap<String, HashMap<Road, BTreeMap<Timestamp, Mile>>>,
    ticketed_days: HashMap<String, HashSet<u32>>,
    limits: HashMap<Road, Limit>,
}

type TicketQueue = (mpmc::Sender<TicketRecord>, mpmc::Receiver<TicketRecord>);

impl Collector {
    pub async fn event_loop(mut self, mut messages: mpsc::Receiver<Message>) -> Self {
        tracing::info!("Starting Collector loop");
        let mut queues = HashMap::<Road, TicketQueue>::new();
        while let Some(message) = messages.recv().await {
            match message {
                Message::Plate { record, camera } => {
                    tracing::info!("{camera:?} reports {record:?}");
                    if let Some(ticket) = self.on_plate(record, camera) {
                        let (sender, _) = queue(&mut queues, ticket.road);
                        // Cannot fail as long as `queues` holds a receiver for the road.
                        if let Err(error) = sender.send(ticket).await {
                            tracing::error!(?error, "Ticket queue closed");
                        }
                    }
                }
                Message::Subscribe { road, callback } => {
                    tracing::info!("Received subscription for road {road}");
                    let (_, receiver) = queue(&mut queues, road);
                    if callback.send(receiver.clone()).is_err() {
                        tracing::warn!("They don't seem interested in this road anymore.");
                    }
                }
            }
        }
        tracing::info!("Exiting Collector loop");
        self
    }

    /// Subscribe to the tickets for `road`.
    ///
    /// Returns `None` if the collector is not running.
    pub async fn subscribe(
        sender: &mpsc::Sender<Message>,
        road: Road,
    ) -> Option<oneshot::Receiver<mpmc::Receiver<TicketRecord>>> {
        let (callback, callback_receiver) = oneshot::channel();
        let message = Message::Subscribe { road, callback };

        sender.send(message).await.ok()?;
        Some(callback_receiver)
    }

    /// Record an observation, returning the ticket it warrants, if any.
    fn on_plate(&mut self, record: PlateRecord, camera: Camera) -> Option<TicketRecord> {
        let tickets = self.insert_record(record, camera);
        self.admit(tickets)
    }

    /// Admit the first ticket which does not touch an already ticketed day, marking its days as ticketed.
    fn admit(&mut self, tickets: Vec<TicketRecord>) -> Option<TicketRecord> {
        for ticket in tickets {
            tracing::info!("Violation found: {ticket:?}");
            let ticketed_days = self.ticketed_days.entry(ticket.plate.clone()).or_default();
            if Self::days(ticket.timestamp1, ticket.timestamp2)
                .any(|day| ticketed_days.contains(&day))
            {
                let day = Self::day(ticket.timestamp1);
                tracing::info!("Ignoring ticket starting on day {day}: {ticket:?}");
            } else {
                for day in Self::days(ticket.timestamp1, ticket.timestamp2) {
                    ticketed_days.insert(day);
                }
                return Some(ticket);
            }
        }
        None
    }

    fn insert_record(
        &mut self,
        PlateRecord { plate, timestamp }: PlateRecord,
        Camera { road, mile, limit }: Camera,
    ) -> Vec<TicketRecord> {
        let limit = limit.saturating_mul(100);
        self.limits.insert(road, limit);

        let map = self
            .records
            .entry(plate.to_string())
            .or_default()
            .entry(road)
            .or_default();

        let prev = map
            .range(..timestamp)
            .next_back()
            .map(|(ts, mile)| (*ts, *mile));
        let next = map.range(timestamp..).next().map(|(ts, mile)| (*ts, *mile));

        *map.entry(timestamp).or_default() = mile;

        let mut tickets: Vec<TicketRecord> = Vec::new();

        if let Some((earlier, previous_mile)) = prev {
            if let Some(speed) = Self::is_violation(limit, earlier, timestamp, previous_mile, mile)
            {
                tickets.push(TicketRecord {
                    plate: plate.clone(),
                    road,
                    mile1: previous_mile,
                    timestamp1: earlier,
                    mile2: mile,
                    timestamp2: timestamp,
                    speed,
                });
            }
        }
        if let Some((later, next_mile)) = next {
            if let Some(speed) = Self::is_violation(limit, timestamp, later, mile, next_mile) {
                tickets.push(TicketRecord {
                    plate,
                    road,
                    mile1: mile,
                    timestamp1: timestamp,
                    mile2: next_mile,
                    timestamp2: later,
                    speed,
                });
            }
        }

        tickets
    }

    fn is_violation(limit: u16, ts1: u32, ts2: u32, mile1: u16, mile2: u16) -> Option<u16> {
        let delta_t = ts1.abs_diff(ts2);
        let delta_m = mile1.abs_diff(mile2);
        let speed = (delta_m as f32 / delta_t as f32) * 60.0 * 60.0;
        let speed = speed.round() as u16;
        let speed = speed.saturating_mul(100);
        if speed > limit {
            Some(speed)
        } else {
            None
        }
    }

    fn days(timestamp1: u32, timestamp2: u32) -> impl Iterator<Item = u32> {
        (timestamp1..timestamp2).map(Self::day).unique()
    }

    fn day(timestamp: u32) -> u32 {
        f32::floor(timestamp as f32 / SECONDS_PER_DAY as f32) as u32
    }
}

/// The ticket queue for `road`, created on first use.
fn queue(queues: &mut HashMap<Road, TicketQueue>, road: Road) -> &TicketQueue {
    queues.entry(road).or_insert_with(|| mpmc::bounded(1024))
}

#[cfg(test)]
pub mod test {
    use super::*;

    pub fn plate(plate: &str, timestamp: u32) -> PlateRecord {
        PlateRecord {
            plate: plate.to_string(),
            timestamp,
        }
    }

    pub fn camera(road: Road, mile: Mile, limit: Limit) -> Camera {
        Camera { road, mile, limit }
    }

    /// The collector after these observations, fed straight to its pure core.
    pub fn observed(observations: impl IntoIterator<Item = (PlateRecord, Camera)>) -> Collector {
        let mut collector = Collector::default();
        for (record, camera) in observations {
            collector.on_plate(record, camera);
        }
        collector
    }

    #[tokio::test]
    async fn queues_ticket_until_dispatcher_subscribes() {
        let (sender, receiver) = mpsc::channel(4);
        for (record, camera) in [
            (plate("ABC", 1), camera(12, 2, 10)),
            (plate("ABC", 20), camera(12, 4, 10)),
            (plate("ABC", 24), camera(115, 17, 10)),
        ] {
            let message = Message::Plate { record, camera };
            sender.send(message).await.unwrap();
        }
        let subscription = Collector::subscribe(&sender, 12).await.unwrap();

        // Important for this test:
        drop(sender);

        let collector = Collector::default().event_loop(receiver).await;

        let tickets = subscription.await.unwrap();
        assert_eq!(
            tickets.recv().await.unwrap(),
            TicketRecord {
                plate: "ABC".to_string(),
                road: 12,
                mile1: 2,
                timestamp1: 1,
                mile2: 4,
                timestamp2: 20,
                speed: 37900,
            }
        );
        // The collector is gone, and so is the sender side of the queue.
        assert!(tickets.recv().await.is_err());

        assert_eq!(collector.limits, HashMap::from([(12, 1000), (115, 1000)]));
        assert_eq!(collector.ticketed_days["ABC"], HashSet::from([0]));
    }

    #[tokio::test]
    async fn subscription_fails_without_collector() {
        let (sender, receiver) = mpsc::channel(1);
        drop(receiver);
        assert!(Collector::subscribe(&sender, 1).await.is_none());
    }

    #[test]
    fn no_ticket_within_limit() {
        let mut collector = Collector::default();
        assert_eq!(collector.on_plate(plate("SLOW", 0), camera(1, 0, 60)), None);
        assert_eq!(
            collector.on_plate(plate("SLOW", 60), camera(1, 1, 60)),
            None
        );
        assert!(collector.ticketed_days.is_empty());
    }

    #[test]
    fn at_most_one_ticket_per_day() {
        let mut collector = Collector::default();
        assert_eq!(collector.on_plate(plate("FAST", 0), camera(1, 0, 60)), None);
        let first = collector.on_plate(plate("FAST", 60), camera(1, 10, 60));
        assert_eq!(first.map(|t| t.speed), Some(60000));
        assert_eq!(
            collector.on_plate(plate("FAST", 120), camera(1, 20, 60)),
            None
        );
    }

    #[test]
    fn out_of_order_observations() {
        let mut collector = Collector::default();
        assert_eq!(
            collector.on_plate(plate("LATE", 45), camera(123, 9, 60)),
            None
        );
        assert_eq!(
            collector.on_plate(plate("LATE", 0), camera(123, 8, 60)),
            Some(TicketRecord {
                plate: "LATE".to_string(),
                road: 123,
                mile1: 8,
                timestamp1: 0,
                mile2: 9,
                timestamp2: 45,
                speed: 8000,
            })
        );
    }
}
