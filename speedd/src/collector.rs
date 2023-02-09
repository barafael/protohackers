use itertools::Itertools;
use speedd_codecs::{
    camera::Camera, plate::PlateRecord, server::TicketRecord, Limit, Mile, Road, Timestamp,
};
use std::collections::{BTreeMap, HashMap};
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct Collector {
    records: HashMap<String, HashMap<Road, BTreeMap<Timestamp, Mile>>>,
    limits: HashMap<Road, Limit>,
    dispatchers: HashMap<Road, Vec<mpsc::Sender<TicketRecord>>>,
}

impl Collector {
    pub fn new() -> Self {
        Self {
            records: HashMap::default(),
            limits: HashMap::default(),
            dispatchers: HashMap::default(),
        }
    }

    pub async fn run(
        mut self,
        mut reporting: mpsc::Receiver<(PlateRecord, Camera)>,
        mut dispatcher_subscription: mpsc::Receiver<(Road, mpsc::Sender<TicketRecord>)>,
    ) -> anyhow::Result<()> {
        println!("Starting Collector loop");
        loop {
            tokio::select! {
                Some((record, camera)) = reporting.recv() => {
                    println!("{camera:?} reports {record:?}");
                    self.handle_plate(record, camera).await?;
                }
                Some((road, sender)) = dispatcher_subscription.recv() => {
                    println!("Received subscription for road {road}");
                    self.handle_subscription(road, sender).await?;
                }
                else => break
            }
            self.emit_tickets().await?;
            dbg!(&self.records);
            dbg!(&self.dispatchers.keys());
        }
        println!("Ended Collector loop");
        Ok(())
    }

    pub async fn handle_plate(
        &mut self,
        PlateRecord { plate, timestamp }: PlateRecord,
        Camera { road, mile, limit }: Camera,
    ) -> anyhow::Result<()> {
        self.limits.insert(road, limit);
        *self
            .records
            .entry(plate)
            .or_default()
            .entry(road)
            .or_default()
            .entry(timestamp)
            .or_default() = mile;
        Ok(())
    }

    pub async fn handle_subscription(
        &mut self,
        road: u16,
        sender: mpsc::Sender<TicketRecord>,
    ) -> anyhow::Result<()> {
        self.dispatchers.entry(road).or_default().push(sender);
        Ok(())
    }

    async fn emit_tickets(&mut self) -> anyhow::Result<()> {
        let mut tickets = Vec::new();
        for (car, road_to_records) in &self.records {
            for (road, records) in road_to_records {
                for ((ts1, mile1), (ts2, mile2)) in records.iter().tuple_windows() {
                    let delta_t = ts1.abs_diff(*ts2);
                    let delta_m = mile1.abs_diff(*mile2);
                    let speed = (delta_m as f32 / delta_t as f32) * 60.0 * 60.0;
                    let speed = speed.round() as u16;
                    let speed = 100 * speed;
                    if &speed > self.limits.get(road).unwrap() {
                        let ticket = TicketRecord {
                            plate: car.clone(),
                            road: *road,
                            mile1: *mile1,
                            timestamp1: *ts1,
                            mile2: *mile2,
                            timestamp2: *ts2,
                            speed,
                        };
                        tickets.push(ticket);
                    }
                }
            }
        }
        for mut ticket in tickets {
            println!("Violation found: {ticket:?}");
            let mut sent = false;
            if let Some(cbs) = self.dispatchers.get_mut(&ticket.road) {
                for cb in cbs {
                    if let Err(e) = cb.send(ticket.clone()).await {
                        ticket = e.0;
                    } else {
                        println!("Dispatched ticket {ticket:?}");
                        sent = true;
                        break;
                    }
                }
            }
            if sent {
                self.records
                    .get_mut(&ticket.plate)
                    .unwrap()
                    .get_mut(&ticket.road)
                    .unwrap()
                    .remove(&ticket.timestamp1);
                self.records
                    .get_mut(&ticket.plate)
                    .unwrap()
                    .get_mut(&ticket.road)
                    .unwrap()
                    .remove(&ticket.timestamp2);
            }
        }
        Ok(())
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
        let (ticket_tx, mut ticket_rx) = mpsc::channel(1);
        disp_tx.send((12, ticket_tx)).await.unwrap();
        drop(disp_tx);
        drop(sender);
        let col = Collector::new();
        println!("running");
        col.run(receiver, disp_rx).await.unwrap();
        let val = ticket_rx.recv().await.unwrap();
        dbg!(val);
    }
}
