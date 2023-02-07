use crate::{
    camera::{Camera, PlateRecord},
    server::TicketRecord,
    Limit, Mile, Road, Timestamp,
};
use itertools::Itertools;
use std::collections::{BTreeMap, HashMap};
use tokio::sync::mpsc;

pub struct Collector {
    receiver: mpsc::Receiver<(PlateRecord, Camera)>,
    records: HashMap<String, HashMap<Road, BTreeMap<Timestamp, Mile>>>,
    limits: HashMap<Road, Limit>,
    dispatchers: HashMap<Road, Vec<mpsc::Sender<TicketRecord>>>,
    backlog: HashMap<Road, TicketRecord>,
}

impl Collector {
    pub fn new(receiver: mpsc::Receiver<(PlateRecord, Camera)>) -> Self {
        Self {
            receiver,
            dispatchers: HashMap::default(),
            records: HashMap::default(),
            limits: HashMap::default(),
            backlog: HashMap::default(),
        }
    }

    pub async fn run(
        mut self,
        mut subscription: mpsc::Receiver<(Road, mpsc::Sender<TicketRecord>)>,
    ) -> anyhow::Result<(
        HashMap<String, HashMap<Road, BTreeMap<Timestamp, Mile>>>,
        HashMap<Road, Limit>,
    )> {
        tokio::select! {
            Some((record, camera)) = self.receiver.recv() => {
                self.handle_plate(record, camera).await?;
            }
            Some((road, sender)) = subscription.recv() => {
                self.dispatchers.entry(road).or_default().push(sender)
            }
        }
        Ok((self.records, self.limits))
    }

    pub async fn handle_plate(
        &mut self,
        PlateRecord { plate, timestamp }: PlateRecord,
        Camera { road, mile, limit }: Camera,
    ) -> anyhow::Result<()> {
        self.limits.insert(road, limit);
        *self
            .records
            .entry(plate.clone())
            .or_default()
            .entry(road)
            .or_default()
            .entry(timestamp)
            .or_default() = mile;
        for (car, road_to_records) in &self.records {
            for (road, records) in road_to_records {
                for ((ts1, mile1), (ts2, mile2)) in records.iter().tuple_windows() {
                    let delta_t = ts1.abs_diff(*ts2);
                    let delta_m = mile1.abs_diff(*mile2);
                    let speed = delta_m as f32 / (delta_t as f32 * 60.0 * 60.0);
                    let speed = speed.round() as u16;
                    if &speed > self.limits.get(road).unwrap() {
                        let mut ticket = TicketRecord {
                            plate: car.clone(),
                            road: *road,
                            mile1: *mile1,
                            timestamp1: *ts1,
                            mile2: *mile2,
                            timestamp2: *ts2,
                            speed,
                        };
                        let mut sent = false;
                        if let Some(cbs) = self.dispatchers.get(road) {
                            for cb in cbs {
                                if let Err(e) = cb.send(ticket.clone()).await {
                                    ticket = e.0;
                                } else {
                                    sent = true;
                                    break;
                                }
                            }
                        }
                        if !sent {
                            self.backlog.insert(*road, ticket);
                        }
                    }
                }
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
        let (_disp_tx, disp_rx) = mpsc::channel(1);
        let col = Collector::new(receiver);
        sender
            .send((
                PlateRecord {
                    plate: "ABC".to_string(),
                    timestamp: 1,
                },
                Camera {
                    road: 12,
                    mile: 2,
                    limit: 60,
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
                    mile: 8,
                    limit: 60,
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
                    limit: 80,
                },
            ))
            .await
            .unwrap();
        drop(sender);
        let val = col.run(disp_rx).await.unwrap();
        dbg!(val);
    }
}
