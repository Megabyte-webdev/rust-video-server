use std::collections::{ HashMap, HashSet };
use std::sync::Arc;
use tokio::sync::RwLock;

use axum::extract::ws::Message;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::track::track_remote::TrackRemote;

#[derive(Clone)]
pub struct ServerPeer {
    pub user_id: String,
    pub publisher_pc: Arc<RTCPeerConnection>,

    // SFU -> Client
    pub subscriber_pc: Arc<RTCPeerConnection>,
}

impl ServerPeer {
    pub fn new(
        user_id: String,
        publisher_pc: Arc<RTCPeerConnection>,
        subscriber_pc: Arc<RTCPeerConnection>
    ) -> Self {
        Self {
            user_id,
            publisher_pc,
            subscriber_pc,
        }
    }
}

#[derive(Clone)]
pub struct ParticipantState {
    pub id: String,
    pub name: String,
    pub session_id: String,
    pub last_seen: u64,
    pub is_presenter: bool,
    pub is_host: bool,
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

impl ClientSender {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<Message>) -> Self {
        Self { tx }
    }

    pub fn send(&self, msg: Message) -> Result<(), tokio::sync::mpsc::error::SendError<Message>> {
        self.tx.send(msg)
    }
}

pub struct Room {
    pub participants: HashMap<String, ParticipantState>,
    pub sessions: HashMap<String, String>,
    pub senders: HashMap<String, ClientSender>,
    pub presenter_id: Option<String>,
    pub host_id: Option<String>,
    pub is_open: Option<bool>,
    pub pending_requests: HashMap<String, JoinRequest>,
    pub approved_users: HashSet<String>,
    pub server_peers: HashMap<String, ServerPeer>,
    pub published_tracks: HashMap<String, Vec<Arc<TrackRemote>>>,
}

pub type Rooms = Arc<RwLock<HashMap<String, Room>>>;
