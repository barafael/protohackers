use std::{
    collections::{BTreeMap, HashSet},
    net::SocketAddr,
    time::Duration,
};

use rand::prelude::*;
use serde::{Deserialize, Serialize};
use speedd_codecs::{camera::Camera, plate::PlateRecord, Mile, SECONDS_PER_DAY};
use tokio::task::JoinHandle;

use crate::{
    camera_client::{Action, CameraClient},
    landscape::Landscape,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sequence {
    roads: Vec<Road>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Road {
    id: u16,
    limit: u16,
    cameras: BTreeMap<Mile, CameraClient>,
    cars: HashSet<Car>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Car {
    plate: String,
}

impl Sequence {
    pub fn new(landscape: &Landscape) -> Self {
        let mut rng = thread_rng();
        let mut roads: Vec<Road> = Vec::new();

        log::info!("Initialize roads with a random speed limit and no cameras");
        for id in 0..landscape.number_of_roads {
            let speed_limit = rng.gen_range(10..240u16);
            roads.push(Road {
                id,
                limit: speed_limit,
                cameras: BTreeMap::default(),
                cars: HashSet::default(),
            });
        }

        log::info!("Randomly assign cameras to roads");
        for _ in 0..landscape.number_of_cameras {
            let index = rng.gen_range(0..landscape.number_of_roads) as usize;
            let last_cam = *roads[index].cameras.keys().max().unwrap_or(&0);
            let distance = rng.gen_range(1..50);
            let mile = last_cam + distance;
            let new_cam = CameraClient::from(Camera {
                road: roads[index].id,
                mile,
                limit: roads[index].limit,
            })
            .with_random_start_delay_then_connect(&mut rng)
            .with_random_delay_then_identify(&mut rng);
            roads[index].cameras.insert(mile, new_cam);
        }

        log::info!("Randomly insert cars in roads, generating unique license plates");
        for _ in 0..landscape.number_of_cars {
            let road_idx = rng.gen_range(0..landscape.number_of_roads);
            let cars = &mut roads[road_idx as usize].cars;
            loop {
                let new_license = license_plate(&mut rng);
                let new_car = Car { plate: new_license };
                if !cars.contains(&new_car) {
                    cars.insert(new_car);
                    break;
                }
            }
        }

        log::info!("Generating events for all cars");
        for road in &mut roads {
            for car in &road.cars {
                // Start somewhere in the first 2 days
                let mut last_timestamp = rng.gen_range(0..SECONDS_PER_DAY * 2);
                for [(mile1, camera1), (mile2, camera2)] in road.cameras.iter_mut().array_chunks() {
                    let speed = if rng.gen::<f64>() < landscape.ticket_likelihood {
                        let too_fast = rng.gen_range(1..100);
                        road.limit + too_fast
                    } else {
                        rng.gen_range(5..road.limit)
                    };
                    let report1 = PlateRecord {
                        plate: car.plate.clone(),
                        timestamp: last_timestamp,
                    };
                    camera1.reports.push(report1);
                    let distance = mile1.abs_diff(*mile2);
                    let time = distance / speed;
                    last_timestamp += u32::from(time);
                    let report2 = PlateRecord {
                        plate: car.plate.clone(),
                        timestamp: last_timestamp,
                    };
                    camera2.reports.push(report2);
                }
            }
        }
        for road in &mut roads {
            if landscape.shuffle_reports {
                for camera in road.cameras.values_mut() {
                    camera.append_shuffled_reports(&mut rng);
                }
            } else {
                for camera in road.cameras.values_mut() {
                    camera.append_reports(&mut rng);
                }
            }
            for camera in road.cameras.values_mut() {
                camera
                    .actions
                    .push(Action::Wait(Duration::from_millis(rng.gen_range(0..2000))));
                camera.actions.push(Action::Disconnect);
            }
        }

        Self { roads }
    }
}

fn license_plate(rng: &mut ThreadRng) -> String {
    use rand::distributions::Alphanumeric;
    rng.sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect()
}

impl Sequence {
    pub async fn run(
        self,
        addr: SocketAddr,
    ) -> anyhow::Result<Vec<JoinHandle<anyhow::Result<()>>>> {
        let mut handles = Vec::new();
        for road in self.roads {
            for (_, camera) in road.cameras {
                handles.push(tokio::spawn(async move { camera.run(addr).await }));
            }
        }
        Ok(handles)
    }
}
