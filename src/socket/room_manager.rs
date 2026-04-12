use std::collections::HashMap;
use tokio::sync::{ RwLock, mpsc::UnboundedSender };

use axum::extract::ws::Message;

#[derive(Clone)]
pub struct ParticipantState {
    pub id: String,
    pub name: String,
    pub session_id: String,
    pub last_seen: u64,
}
pub struct Room {
    pub participants: HashMap<String, ParticipantState>, // user_id -> state
    pub sessions: HashMap<String, String>, // session_id -> user_id
    pub senders: HashMap<String, UnboundedSender<Message>>, // session_id -> sender
}

pub type Rooms = std::sync::Arc<RwLock<HashMap<String, Room>>>;
