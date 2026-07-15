use std::collections::{ HashMap, HashSet };
use tokio::sync::{ RwLock };

use axum::extract::ws::Message;

#[derive(Clone)]
pub struct ParticipantState {
    pub id: String,
    pub name: String,
    pub session_id: String,
    pub last_seen: u64,
    pub is_presenter: bool,
    pub is_host: bool,
    pub camera_stream_id: Option<String>,
    pub screen_share_stream_id: Option<String>,
    pub mic_enabled: bool,
    pub cam_enabled: bool,
}

#[derive(Clone)]
pub struct JoinRequest {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub tx: ClientSender,
}

#[derive(Clone)]
pub struct ClientSender {
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
}

pub struct Room {
    pub participants: HashMap<String, ParticipantState>,
    pub sessions: HashMap<String, String>,
    pub senders: HashMap<String, ClientSender>,
    pub watchers: HashMap<String, ClientSender>,
    pub presenter_id: Option<String>,
    pub host_id: Option<String>,
    pub is_open: Option<bool>,
    pub pending_requests: HashMap<String, JoinRequest>, // request_id -> JoinRequest
    pub approved_users: HashSet<String>,
    pub presenter_stream_id: Option<String>,
}

pub type Rooms = std::sync::Arc<RwLock<HashMap<String, Room>>>;

impl ClientSender {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<Message>) -> Self {
        Self { tx }
    }

    pub fn send(&self, msg: Message) -> Result<(), tokio::sync::mpsc::error::SendError<Message>> {
        self.tx.send(msg)
    }
}
