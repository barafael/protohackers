pub mod encoder;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Error(String),
    Ticket(TicketRecord),
    Heartbeat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketRecord {
    pub plate: String,
    pub road: u16,
    pub mile1: u16,
    pub timestamp1: u32,
    pub mile2: u16,
    pub timestamp2: u32,
    pub speed: u16,
}
