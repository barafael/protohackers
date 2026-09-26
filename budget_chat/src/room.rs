use itertools::Itertools;
use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
};
use tokio::sync::mpsc::{self, error::TrySendError};

/// A message for the [`Room`].
#[derive(Debug)]
pub enum Message {
    /// A named user enters. `outbox` delivers lines to them from now on.
    Join {
        id: SocketAddr,
        name: String,
        outbox: mpsc::Sender<String>,
    },

    /// A user says something.
    Chat { id: SocketAddr, text: String },

    /// A user leaves.
    Leave { id: SocketAddr },
}

/// The chat room. It *is* its data: who is present, under which name.
///
/// The outboxes to the present users are runtime resources, so they belong to the event loop.
/// They lead to each connection's writing half only, so the room never waits on a user who waits on the room.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Room {
    members: BTreeMap<SocketAddr, String>,
}

impl Room {
    pub async fn event_loop(mut self, mut messages: mpsc::Receiver<Message>) -> Self {
        let mut outboxes = HashMap::<SocketAddr, mpsc::Sender<String>>::new();
        while let Some(message) = messages.recv().await {
            match message {
                Message::Join { id, name, outbox } => {
                    let others = self.members.values().join(", ");
                    deliver(&outbox, format!("* The room contains: {others}"));
                    self.announce(&outboxes, id, format!("* {name} has joined the room"));
                    self.members.insert(id, name);
                    outboxes.insert(id, outbox);
                }
                Message::Chat { id, text } => {
                    if let Some(name) = self.members.get(&id) {
                        self.announce(&outboxes, id, format!("[{name}] {text}"));
                    }
                }
                Message::Leave { id } => {
                    outboxes.remove(&id);
                    if let Some(name) = self.members.remove(&id) {
                        self.announce(&outboxes, id, format!("* {name} has left the room"));
                    }
                }
            }
        }
        self
    }

    /// Tell everybody but `origin`.
    fn announce(
        &self,
        outboxes: &HashMap<SocketAddr, mpsc::Sender<String>>,
        origin: SocketAddr,
        line: String,
    ) {
        println!("{line}");
        for (id, outbox) in outboxes {
            if *id != origin {
                deliver(outbox, line.clone());
            }
        }
    }
}

/// Deliver without waiting: a user who does not keep up misses lines, but never stalls the room.
fn deliver(outbox: &mpsc::Sender<String>, line: String) {
    if let Err(TrySendError::Full(line)) = outbox.try_send(line) {
        println!("Dropping {line:?} for a slow user");
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    async fn drain(mut inbox: mpsc::Receiver<String>) -> Vec<String> {
        let mut lines = Vec::new();
        while let Some(line) = inbox.recv().await {
            lines.push(line);
        }
        lines
    }

    #[tokio::test]
    async fn chat_session() {
        let (alice, bob) = (addr(1), addr(2));
        let (alice_outbox, alice_inbox) = mpsc::channel(8);
        let (bob_outbox, bob_inbox) = mpsc::channel(8);
        let (tx, rx) = mpsc::channel(8);
        for message in [
            Message::Join {
                id: alice,
                name: "alice".to_string(),
                outbox: alice_outbox,
            },
            Message::Join {
                id: bob,
                name: "bob".to_string(),
                outbox: bob_outbox,
            },
            Message::Chat {
                id: alice,
                text: "hi bob".to_string(),
            },
            Message::Chat {
                id: bob,
                text: "hi alice".to_string(),
            },
            Message::Leave { id: bob },
            Message::Chat {
                id: alice,
                text: "bye".to_string(),
            },
        ] {
            tx.send(message).await.unwrap();
        }

        // Important for this test:
        drop(tx);

        let room = Room::default().event_loop(rx).await;

        assert_eq!(
            drain(alice_inbox).await,
            [
                "* The room contains: ",
                "* bob has joined the room",
                "[bob] hi alice",
                "* bob has left the room",
            ]
        );
        assert_eq!(
            drain(bob_inbox).await,
            ["* The room contains: alice", "[alice] hi bob"]
        );
        assert_eq!(
            room,
            Room {
                members: BTreeMap::from([(alice, "alice".to_string())])
            }
        );
    }

    #[tokio::test]
    async fn strangers_are_ignored() {
        let (outbox, inbox) = mpsc::channel(8);
        let (tx, rx) = mpsc::channel(8);
        tx.send(Message::Join {
            id: addr(1),
            name: "alice".to_string(),
            outbox,
        })
        .await
        .unwrap();
        tx.send(Message::Chat {
            id: addr(2),
            text: "who am I".to_string(),
        })
        .await
        .unwrap();
        tx.send(Message::Leave { id: addr(2) }).await.unwrap();
        drop(tx);

        Room::default().event_loop(rx).await;

        assert_eq!(drain(inbox).await, ["* The room contains: "]);
    }

    #[tokio::test]
    async fn slow_users_miss_lines() {
        let (alice_outbox, alice_inbox) = mpsc::channel(1);
        let (bob_outbox, _bob_inbox) = mpsc::channel(8);
        let (tx, rx) = mpsc::channel(8);
        for message in [
            Message::Join {
                id: addr(1),
                name: "alice".to_string(),
                outbox: alice_outbox,
            },
            Message::Join {
                id: addr(2),
                name: "bob".to_string(),
                outbox: bob_outbox,
            },
        ] {
            tx.send(message).await.unwrap();
        }
        drop(tx);

        Room::default().event_loop(rx).await;

        // The join announcement did not fit into alice's outbox.
        assert_eq!(drain(alice_inbox).await, ["* The room contains: "]);
    }
}
