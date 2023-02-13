use async_channel as mpmc;
use itertools::Itertools;
use speedd_codecs::{
    camera::Camera, plate::PlateRecord, server::TicketRecord, Limit, Mile, Road, Timestamp,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
pub struct Collector {
    records: HashMap<String, HashMap<Road, BTreeMap<Timestamp, Mile>>>,
    ticketed_days: HashMap<String, HashSet<u32>>,
    limits: HashMap<Road, Limit>,
    dispatchers: HashMap<Road, (mpmc::Sender<TicketRecord>, mpmc::Receiver<TicketRecord>)>,
}

impl Collector {
    pub fn new() -> Self {
        Self {
            records: HashMap::default(),
            ticketed_days: HashMap::default(),
            limits: HashMap::default(),
            dispatchers: HashMap::default(),
        }
    }

    pub async fn run(
        mut self,
        mut reporting: mpsc::Receiver<(PlateRecord, Camera)>,
        mut dispatcher_subscription: mpsc::Receiver<(
            Road,
            oneshot::Sender<mpmc::Receiver<TicketRecord>>,
        )>,
    ) -> anyhow::Result<()> {
        tracing::info!("Starting Collector loop");
        loop {
            tokio::select! {
                Some((record, camera)) = reporting.recv() => {
                    tracing::info!("{camera:?} reports {record:?}");
                    let tickets = self.insert_record(record, camera).await?;
                    self.dispatch_tickets(&tickets).await?;
                }
                Some((road, sender)) = dispatcher_subscription.recv() => {
                    tracing::info!("Received subscription for road {road}");
                    let ticket_sender = self.insert_dispatcher(road).await?;
                    let _x = sender.send(ticket_sender);
                }
                else => break
            }
            //dbg!(&self.records);
            //dbg!(&self.dispatchers.keys());
            //dbg!(&self.ticketed_days);
        }
        tracing::info!("Ended Collector loop");
        Ok(())
    }

    pub async fn insert_dispatcher(
        &mut self,
        road: u16,
    ) -> anyhow::Result<mpmc::Receiver<TicketRecord>> {
        // Create new channel for this road
        let (_, receiver) = self.dispatchers.entry(road).or_insert(mpmc::bounded(1024));
        Ok(receiver.clone())
    }

    async fn dispatch_tickets(&mut self, tickets: &[TicketRecord]) -> anyhow::Result<()> {
        for ticket in tickets {
            tracing::info!("Violation found: {ticket:?}");
            for day in Self::days(ticket.timestamp1, ticket.timestamp2) {
                let ticketed_days = self.ticketed_days.entry(ticket.plate.clone()).or_default();
                if ticketed_days.contains(&day) {
                    tracing::info!("Ignoring day {day}, already ticketed {}", ticket.plate);
                } else {
                    // Enqueue ticket in channel without existing dispatcher
                    let (tx, _) = self
                        .dispatchers
                        .entry(ticket.road)
                        .or_insert(mpmc::bounded(1024));
                    tx.send(ticket.clone()).await?;
                    ticketed_days.insert(day);
                }
            }
        }
        Ok(())
    }

    async fn insert_record(
        &mut self,
        PlateRecord { plate, timestamp }: PlateRecord,
        Camera { road, mile, limit }: Camera,
    ) -> anyhow::Result<Vec<TicketRecord>> {
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
                })
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
                })
            }
        }

        Ok(tickets)
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
        f32::floor(timestamp as f32 / 86400f32) as u32
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn example() {
        let (sender, receiver) = mpsc::channel(3);
        sender
            .send((
                PlateRecord {
                    plate: "ABC".to_string(),
                    timestamp: 1,
                },
                Camera {
                    road: 12,
                    mile: 2,
                    limit: 10,
                },
            ))
            .await
            .unwrap();
        sender
            .send((
                PlateRecord {
                    plate: "ABC".to_string(),
                    timestamp: 20,
                },
                Camera {
                    road: 12,
                    mile: 4,
                    limit: 10,
                },
            ))
            .await
            .unwrap();
        sender
            .send((
                PlateRecord {
                    plate: "ABC".to_string(),
                    timestamp: 24,
                },
                Camera {
                    road: 115,
                    mile: 17,
                    limit: 10,
                },
            ))
            .await
            .unwrap();
        let (disp_tx, disp_rx) = mpsc::channel(1);

        let (tx, rx) = oneshot::channel();
        disp_tx.send((12, tx)).await.unwrap();
        drop(disp_tx);
        drop(sender);

        let col = Collector::new();
        col.run(receiver, disp_rx).await.unwrap();
        let ticket_rx = rx.await.unwrap();
        let val = ticket_rx.recv().await.unwrap();
        dbg!(val);
    }
}
