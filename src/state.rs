use std::collections::HashMap;
use axum::extract::ws::Message;
use tokio::sync::RwLock;
use sqlx::PgPool;

pub type Rooms = std::sync::Arc<RwLock<HashMap<String, Room>>>;

#[derive(Clone)]
pub struct AppState {
    pub rooms: Rooms,
    pub db: PgPool,
}

#[derive(Clone)]
pub struct Room {
    pub participants: HashMap<String, RoomParticipant>,
    pub senders: HashMap<String, tokio::sync::mpsc::UnboundedSender<Message>>,
}

#[derive(Clone)]
pub struct RoomParticipant {
    pub id: String,
    pub name: String,
}
