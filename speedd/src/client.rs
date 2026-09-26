use crate::collector::{self, Collector};
use async_channel as mpmc;
use futures::{future::try_join_all, stream::SelectAll, Sink, SinkExt, Stream, StreamExt};
use speedd_codecs::{
    camera::Camera,
    client::Message,
    plate::PlateRecord,
    server::{self, TicketRecord},
    Road,
};
use std::time::Duration;
use tokio::{
    select,
    sync::mpsc,
    time::{interval, Interval},
};

/// A client connection. It starts out unidentified, then becomes a camera or a dispatcher.
///
/// The actor *is* its data: who the client claims to be, and which heartbeat it asked for.
/// The socket, the heartbeat timer, and the ticket queues are runtime resources owned by the event loop.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Client {
    role: Role,
    heartbeat: Option<Duration>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Role {
    #[default]
    Unidentified,
    Camera(Camera),
    Dispatcher(Vec<Road>),
}

/// Something that happened to a client.
#[derive(Debug)]
enum Event {
    Message(Message),
    Garbage(anyhow::Error),
    Heartbeat,
    Ticket(TicketRecord),
}

/// What the event loop must do in reaction to an [`Event`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Send(server::Message),
    Report(PlateRecord, Camera),
    StartHeartbeat(Duration),
    Subscribe(Vec<Road>),
}

impl Client {
    pub async fn event_loop<R, W>(
        mut self,
        mut reader: R,
        mut writer: W,
        collector: mpsc::Sender<collector::Message>,
    ) -> Self
    where
        R: Stream<Item = Result<Message, anyhow::Error>> + Unpin,
        W: Sink<server::Message> + Unpin,
        W::Error: Into<anyhow::Error>,
    {
        let mut heartbeat: Option<Interval> = None;
        let mut tickets = SelectAll::<mpmc::Receiver<TicketRecord>>::new();

        tracing::info!("Entering client connection loop");
        loop {
            let event = select! {
                message = reader.next() => match message {
                    Some(Ok(message)) => Event::Message(message),
                    Some(Err(error)) => Event::Garbage(error),
                    None => break,
                },
                () = tick(&mut heartbeat) => Event::Heartbeat,
                Some(ticket) = tickets.next() => Event::Ticket(ticket),
            };

            let result = match self.on_event(event) {
                Action::None => Ok(()),
                Action::Send(message) => writer.send(message).await.map_err(Into::into),
                Action::Report(record, camera) => collector
                    .send(collector::Message::Plate { record, camera })
                    .await
                    .map_err(Into::into),
                Action::StartHeartbeat(period) => {
                    heartbeat = Some(interval(period));
                    Ok(())
                }
                Action::Subscribe(roads) => subscribe(&collector, roads)
                    .await
                    .map(|queues| tickets.extend(queues)),
            };
            if let Err(error) = result {
                tracing::warn!(?error, "Closing client connection");
                break;
            }
        }
        tracing::info!("Leaving client connection loop");
        self
    }

    fn on_event(&mut self, event: Event) -> Action {
        match event {
            Event::Message(message) => {
                tracing::trace!("Received client message {message:?}");
                self.on_message(message)
            }
            Event::Garbage(error) => Action::Send(server::Message::Error(match self.role {
                Role::Unidentified => format!("... who even are you? {error:?}"),
                Role::Camera(_) => format!("Nahh... you're just a camera. {error:?}"),
                Role::Dispatcher(_) => format!("Nahh... you're just a dispatcher. {error:?}"),
            })),
            Event::Heartbeat => {
                tracing::trace!("Sending heartbeat");
                Action::Send(server::Message::Heartbeat)
            }
            Event::Ticket(ticket) => {
                tracing::info!("Received ticket {ticket:?}");
                Action::Send(server::Message::Ticket(ticket))
            }
        }
    }

    /// The client protocol, as a pure state transition.
    fn on_message(&mut self, message: Message) -> Action {
        let error = |text: &str| Action::Send(server::Message::Error(text.to_string()));
        match (&self.role, message) {
            (_, Message::WantHeartbeat(period)) => {
                if self.heartbeat.is_some() {
                    tracing::warn!("Ignoring repeated heartbeat request");
                    return error("You already specified a heartbeat");
                }
                self.heartbeat = Some(period);
                if period.is_zero() {
                    tracing::warn!("Ignoring zero-duration heartbeat");
                    Action::None
                } else {
                    tracing::info!("Starting heartbeat");
                    Action::StartHeartbeat(period)
                }
            }
            (Role::Unidentified, Message::Plate(record)) => {
                tracing::warn!(
                    "Ignoring {record:?} due to client not having specialized as camera"
                );
                error("You are no camera")
            }
            (Role::Unidentified, Message::IAmCamera(camera)) => {
                self.role = Role::Camera(camera);
                Action::None
            }
            (Role::Unidentified, Message::IAmDispatcher(roads)) => {
                self.role = Role::Dispatcher(roads.clone());
                Action::Subscribe(roads)
            }
            (Role::Camera(camera), Message::Plate(record)) => {
                Action::Report(record, camera.clone())
            }
            (Role::Camera(_), Message::IAmCamera(_)) => {
                tracing::warn!("Ignoring repeated IAmCamera");
                error("Yes, you are (a camera)")
            }
            (Role::Camera(_), Message::IAmDispatcher(_)) => error("No you're not (a dispatcher)"),
            (Role::Dispatcher(_), Message::Plate(_)) => error("You Sir Dispatcher are confused"),
            (Role::Dispatcher(_), Message::IAmCamera(_)) => error("No you're not (a camera)"),
            (Role::Dispatcher(_), Message::IAmDispatcher(_)) => {
                error("Yes, you are (a dispatcher)")
            }
        }
    }
}

/// Wait for the next heartbeat, or forever if there is none.
async fn tick(heartbeat: &mut Option<Interval>) {
    match heartbeat {
        Some(interval) => {
            interval.tick().await;
        }
        None => std::future::pending().await,
    }
}

/// Obtain the ticket queues for `roads` from the collector.
async fn subscribe(
    collector: &mpsc::Sender<collector::Message>,
    roads: Vec<Road>,
) -> anyhow::Result<Vec<mpmc::Receiver<TicketRecord>>> {
    let mut callbacks = Vec::with_capacity(roads.len());
    for road in roads {
        let callback = Collector::subscribe(collector, road)
            .await
            .ok_or_else(|| anyhow::anyhow!("Collector is not running"))?;
        callbacks.push(callback);
    }
    Ok(try_join_all(callbacks).await?)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::collector::test::{camera, observed, plate};
    use futures::{channel::mpsc as unbounded, future, stream};

    fn error(text: &str) -> Action {
        Action::Send(server::Message::Error(text.to_string()))
    }

    #[test]
    fn unidentified_client_cannot_report() {
        let mut client = Client::default();
        let action = client.on_message(Message::Plate(plate("UN1X", 0)));
        assert_eq!(action, error("You are no camera"));
        assert_eq!(client, Client::default());
    }

    #[test]
    fn heartbeat_can_only_be_requested_once() {
        let mut client = Client::default();
        let period = Duration::from_millis(2500);
        assert_eq!(
            client.on_message(Message::WantHeartbeat(period)),
            Action::StartHeartbeat(period)
        );
        assert_eq!(
            client.on_message(Message::WantHeartbeat(period)),
            error("You already specified a heartbeat")
        );

        let mut client = Client::default();
        assert_eq!(
            client.on_message(Message::WantHeartbeat(Duration::ZERO)),
            Action::None
        );
        assert_eq!(
            client.on_message(Message::WantHeartbeat(period)),
            error("You already specified a heartbeat")
        );
        assert_eq!(client.heartbeat, Some(Duration::ZERO));
    }

    #[test]
    fn roles_are_permanent() {
        let mut client = Client::default();
        assert_eq!(
            client.on_message(Message::IAmCamera(camera(1, 2, 3))),
            Action::None
        );
        assert_eq!(
            client.on_message(Message::IAmCamera(camera(4, 5, 6))),
            error("Yes, you are (a camera)")
        );
        assert_eq!(
            client.on_message(Message::IAmDispatcher(vec![1])),
            error("No you're not (a dispatcher)")
        );
        assert_eq!(client.role, Role::Camera(camera(1, 2, 3)));

        let mut client = Client::default();
        assert_eq!(
            client.on_message(Message::IAmDispatcher(vec![7, 8])),
            Action::Subscribe(vec![7, 8])
        );
        assert_eq!(
            client.on_message(Message::Plate(plate("UN1X", 0))),
            error("You Sir Dispatcher are confused")
        );
        assert_eq!(
            client.on_message(Message::IAmCamera(camera(1, 2, 3))),
            error("No you're not (a camera)")
        );
        assert_eq!(
            client.on_message(Message::IAmDispatcher(vec![9])),
            error("Yes, you are (a dispatcher)")
        );
        assert_eq!(client.role, Role::Dispatcher(vec![7, 8]));
    }

    #[tokio::test]
    async fn exits_when_client_hangs_up() {
        let (collector, _collector_rx) = mpsc::channel(1);
        let mut written = Vec::<server::Message>::new();

        let client = Client::default()
            .event_loop(stream::empty(), &mut written, collector)
            .await;

        assert_eq!(client, Client::default());
        assert!(written.is_empty());
    }

    #[tokio::test]
    async fn camera_reports_plates() {
        let reader = stream::iter([
            Ok(Message::IAmCamera(camera(123, 8, 60))),
            Ok(Message::Plate(plate("UN1X", 0))),
            Ok(Message::Plate(plate("RE05BKG", 1))),
        ]);
        let (collector, mut collector_rx) = mpsc::channel(3);
        let mut written = Vec::<server::Message>::new();

        let client = Client::default()
            .event_loop(reader, &mut written, collector)
            .await;

        for expected in [plate("UN1X", 0), plate("RE05BKG", 1)] {
            let message = collector_rx.recv().await.unwrap();
            assert!(matches!(
                message,
                collector::Message::Plate { record, camera: c }
                    if record == expected && c == camera(123, 8, 60)
            ));
        }
        assert!(collector_rx.recv().await.is_none());
        assert!(written.is_empty());
        assert_eq!(client.role, Role::Camera(camera(123, 8, 60)));
    }

    #[tokio::test]
    async fn garbage_is_answered_with_an_error() {
        let reader = stream::iter([
            Ok(Message::IAmCamera(camera(123, 8, 60))),
            Err(anyhow::anyhow!("bad")),
        ]);
        let (collector, _collector_rx) = mpsc::channel(1);
        let mut written = Vec::<server::Message>::new();

        let client = Client::default()
            .event_loop(reader, &mut written, collector)
            .await;

        assert_eq!(client.role, Role::Camera(camera(123, 8, 60)));
        // With `RUST_BACKTRACE` set, the debug-formatted error carries a backtrace.
        assert!(matches!(
            written.as_slice(),
            [server::Message::Error(text)] if text.starts_with("Nahh... you're just a camera. bad")
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn heartbeats_at_requested_interval() {
        // The client asks for a heartbeat per second, then hangs up after 3.5 seconds.
        let hang_up = stream::once(Box::pin(tokio::time::sleep(Duration::from_millis(3500))))
            .filter_map(|()| future::ready(None));
        let reader =
            stream::iter([Ok(Message::WantHeartbeat(Duration::from_secs(1)))]).chain(hang_up);
        let (collector, _collector_rx) = mpsc::channel(1);
        let mut written = Vec::<server::Message>::new();

        let client = Client::default()
            .event_loop(reader, &mut written, collector)
            .await;

        // At 0, 1, 2, and 3 seconds.
        assert_eq!(written, vec![server::Message::Heartbeat; 4]);
        assert_eq!(client.heartbeat, Some(Duration::from_secs(1)));
    }

    /// The example session from the problem statement: two cameras, one dispatcher, no sockets.
    #[tokio::test]
    async fn two_cameras_and_a_dispatcher() {
        let (collector, collector_rx) = mpsc::channel(8);

        let camera1 = stream::iter([
            Ok(Message::IAmCamera(camera(123, 8, 60))),
            Ok(Message::Plate(plate("UN1X", 0))),
        ]);
        let camera2 = stream::iter([
            Ok(Message::IAmCamera(camera(123, 9, 60))),
            Ok(Message::Plate(plate("UN1X", 45))),
        ]);

        // The dispatcher stays connected until it has received a ticket.
        let (dispatcher_tx, dispatcher_reader) = unbounded::unbounded();
        dispatcher_tx
            .unbounded_send(Ok(Message::IAmDispatcher(vec![123])))
            .unwrap();
        let (dispatcher_writer, mut dispatcher_rx) = unbounded::unbounded();
        let hang_up_after_ticket = async move {
            let message = dispatcher_rx.next().await;
            drop(dispatcher_tx);
            message
        };

        let (collector, camera1, camera2, dispatcher, ticket) = tokio::join!(
            Collector::default().event_loop(collector_rx),
            Client::default().event_loop(camera1, Vec::new(), collector.clone()),
            Client::default().event_loop(camera2, Vec::new(), collector.clone()),
            Client::default().event_loop(dispatcher_reader, dispatcher_writer, collector),
            hang_up_after_ticket,
        );

        assert_eq!(
            ticket,
            Some(server::Message::Ticket(TicketRecord {
                plate: "UN1X".to_string(),
                road: 123,
                mile1: 8,
                timestamp1: 0,
                mile2: 9,
                timestamp2: 45,
                speed: 8000,
            }))
        );
        assert_eq!(camera1.role, Role::Camera(camera(123, 8, 60)));
        assert_eq!(camera2.role, Role::Camera(camera(123, 9, 60)));
        assert_eq!(dispatcher.role, Role::Dispatcher(vec![123]));
        assert_eq!(
            collector,
            observed([
                (plate("UN1X", 0), camera(123, 8, 60)),
                (plate("UN1X", 45), camera(123, 9, 60)),
            ])
        );
    }
}
