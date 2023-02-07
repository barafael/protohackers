use crate::{
    camera::{Camera, PlateRecord},
    server::TicketRecord,
    Dispatchers, Limit, Mile, Road, Timestamp,
};
use itertools::Itertools;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, RwLock},
};
use tokio::sync::mpsc;

pub struct Collector {
    receiver: mpsc::Receiver<(PlateRecord, Camera)>,
    dispatchers: Arc<RwLock<Dispatchers>>,
    records: HashMap<String, HashMap<Road, BTreeMap<Timestamp, Mile>>>,
    limits: HashMap<Road, Limit>,
}

impl Collector {
    pub fn new(
        receiver: mpsc::Receiver<(PlateRecord, Camera)>,
        dispatchers: Arc<RwLock<Dispatchers>>,
    ) -> Self {
        Self {
            receiver,
            dispatchers,
            records: HashMap::default(),
            limits: HashMap::default(),
        }
    }

    pub async fn run(
        mut self,
    ) -> anyhow::Result<(
        HashMap<String, HashMap<Road, BTreeMap<Timestamp, Mile>>>,
        HashMap<Road, Limit>,
    )> {
        while let Some((PlateRecord { plate, timestamp }, Camera { road, mile, limit })) =
            self.receiver.recv().await
        {
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
                        if &speed > self.limits.get(&road).unwrap() {
                            let mut ticket = TicketRecord {
                                plate: car.clone(),
                                road: *road,
                                mile1: *mile1,
                                timestamp1: *ts1,
                                mile2: *mile2,
                                timestamp2: *ts2,
                                speed,
                            };
                            let dispatchers = self.dispatchers.read().unwrap();
                            let mut vec = Vec::new();
                            if let Some(cbs) = dispatchers.get(road) {
                                for cb in cbs {
                                    vec.extend(cbs);
                                    if let Err(e) = cb.send(ticket).await {
                                        ticket = e.0;
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok((self.records, self.limits))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn example() {
        let (sender, receiver) = mpsc::channel(3);
        let dispatchers = Arc::new(RwLock::new(Dispatchers::default()));
        let col = Collector::new(receiver, dispatchers);
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
        let val = col.run().await.unwrap();
        dbg!(val);
    }
}
