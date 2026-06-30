use serde::{ Deserialize, Serialize };
use sqlx::PgPool;
use crate::socket::room_manager::Rooms;

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

#[derive(Clone)]
pub struct AppState {
    pub rooms: Rooms,
    pub db: PgPool,
    pub turn_config: TurnConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum SignalMessage {
    JOIN {
        room_id: String,
        user_id: String,
        sender_name: Option<String>,
    },

    JoinedAck {
        room_id: String,
        user_id: String,
    },

    JoinFailed {
        reason: String,
    },

    OFFER {
        target: String,
        sdp: String,
        room_id: String,
        user_id: String,
    },

    ANSWER {
        target: String,
        sdp: String,
        room_id: String,
        user_id: String,
    },

    ICE {
        target: String,
        candidate: String,
        room_id: String,
        user_id: String,
    },

    UserJoined {
        user_id: String,
        name: String,
    },

    UserLeft {
        user_id: String,
    },
}
