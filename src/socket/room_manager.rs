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
pub struct JoinRequest {
    pub id: String,
    pub room_id: String,
    pub user_id: String,
    pub name: String,
}
pub struct Room {
    pub participants: HashMap<String, ParticipantState>,
    pub sessions: HashMap<String, String>,
    pub senders: HashMap<String, UnboundedSender<Message>>,
    pub presenter_id: Option<String>,
    pub host_id: Option<String>,
    pub is_open: Option<bool>,
    pub pending_users: HashMap<String, UnboundedSender<Message>>,
}

pub type Rooms = std::sync::Arc<RwLock<HashMap<String, Room>>>;
