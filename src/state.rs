use std::{ collections::HashMap, sync::{ Arc } };

use sqlx::PgPool;
use tokio::sync::RwLock;
use crate::socket::room_manager::{ ClientSender, Rooms };

// ============ TURN CONFIGURATION ============
#[derive(Clone, Debug)]
pub struct TurnConfig {
    pub server: String,
    pub auth_secret: String,
}

impl TurnConfig {
    pub fn from_env() -> Result<Self, String> {
        Ok(TurnConfig {
            server: std::env
                ::var("TURN_SERVER")
                .map_err(|_| "TURN_SERVER environment variable must be set".to_string())?,
            auth_secret: std::env
                ::var("TURN_AUTH_SECRET")
                .map_err(|_| "TURN_AUTH_SECRET environment variable must be set".to_string())?,
        })
    }
}

pub type RoomWatchers = Arc<RwLock<HashMap<String, HashMap<String, ClientSender>>>>;

#[derive(Clone)]
pub struct AppState {
    pub rooms: Rooms,
    pub db: PgPool,
    pub turn_config: TurnConfig,
    pub watchers: RoomWatchers,
}
