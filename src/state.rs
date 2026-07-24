use std::{ collections::HashMap, sync::{ Arc } };

use axum::extract::ws::Message;
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;
use webrtc::{
    peer_connection::RTCPeerConnection,
    track::{ track_local::track_local_static_rtp::TrackLocalStaticRTP, track_remote::TrackRemote },
};
use crate::socket::{
    handlers::rtc_signalling::{ ForwarderConfig, SFUError, SFUResult, TrackForwarder },
    room_manager::{ ClientSender, Rooms },
};

#[derive(Clone, Debug, Serialize)]
pub enum TrackSource {
    Camera,
    Screen,
    Audio,
}

#[derive(Clone, Serialize)]
pub struct TrackDescriptor {
    pub id: String,
    pub publisher_id: String,
    pub source: TrackSource, // camera | screen | audio
}
//  TURN CONFIGURATION
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

// TRACK REPOSITORY: MANAGES ALL FORWARDERS
#[derive(Clone)]
pub struct TrackRepository {
    /// Map: (room_id, sender_id) → Vec<Forwarder>
    forwarders: Arc<RwLock<HashMap<(String, String, String), Arc<TrackForwarder>>>>,
    config: ForwarderConfig,
}

impl TrackRepository {
    pub fn new(config: ForwarderConfig) -> Self {
        Self {
            forwarders: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Add a forwarder for a specific room/sender/recipient combination
    pub async fn add_forwarder(
        &self,
        state: &AppState,
        room_id: &str,
        sender_id: &str,
        subscriber_id: &str,
        recipient_pc: Arc<RTCPeerConnection>,
        source: TrackSource,
        remote_track: Arc<TrackRemote>
    ) -> SFUResult<Arc<TrackForwarder>> {
        let codec = remote_track.codec();

        let local_track = Arc::new(
            TrackLocalStaticRTP::new(
                codec.capability,
                format!("forward-{}", Uuid::new_v4()),
                format!("stream-{}", Uuid::new_v4())
            )
        );

        recipient_pc
            .add_track(local_track.clone()).await
            .map_err(|e| SFUError::TrackForwardingFailed(e.to_string()))?;

        /*
        IMPORTANT

        Adding a track requires renegotiation.
    */

        let offer = recipient_pc
            .create_offer(None).await
            .map_err(|e| SFUError::ConnectionError(e.to_string()))?;

        recipient_pc
            .set_local_description(offer.clone()).await
            .map_err(|e| SFUError::ConnectionError(e.to_string()))?;

        let descriptor = TrackDescriptor {
            id: Uuid::new_v4().to_string(),
            publisher_id: sender_id.to_string(),
            source,
        };

        let msg = Message::Text(
            json!({
                    "type":"SUB_OFFER",
                    "payload":offer.sdp,
                    "track": descriptor
                })
                .to_string()
                .into()
        );

        Self::send_to_user(state, room_id, subscriber_id, msg).await;

        let forwarder = Arc::new(
            TrackForwarder::new(
                local_track,
                remote_track,
                sender_id.to_string(),
                subscriber_id.to_string(),
                self.config.clone()
            )
        );

        forwarder.start().await?;

        self.forwarders
            .write().await
            .insert(
                (room_id.to_string(), sender_id.to_string(), subscriber_id.to_string()),
                forwarder.clone()
            );

        Ok(forwarder)
    }

    /// Remove all forwarders for a sender
    pub async fn remove_publisher_forwarders(&self, room_id: &str, publisher_id: &str) {
        let mut forwarders = self.forwarders.write().await;

        let keys: Vec<_> = forwarders
            .keys()
            .filter(|(rid, pid, _)| { rid == room_id && pid == publisher_id })
            .cloned()
            .collect();

        for key in keys {
            if let Some(forwarder) = forwarders.remove(&key) {
                log::info!("Removing publisher forwarder {} -> {}", key.1, key.2);

                forwarder.stop().await;
            }
        }
    }
    pub async fn remove_subscriber_forwarders(&self, room_id: &str, subscriber_id: &str) {
        let mut forwarders = self.forwarders.write().await;

        let keys: Vec<_> = forwarders
            .keys()
            .filter(|(rid, _, sid)| { rid == room_id && sid == subscriber_id })
            .cloned()
            .collect();

        for key in keys {
            if let Some(forwarder) = forwarders.remove(&key) {
                log::info!("Removing subscriber forwarder {} -> {}", key.1, key.2);

                forwarder.stop().await;
            }
        }
    }

    /// Get metrics for all forwarders
    pub async fn get_all_metrics(&self) -> serde_json::Value {
        let forwarders = self.forwarders.read().await;
        let mut metrics = Vec::new();

        for ((room_id, publisher_id, subscriber_id), forwarder) in forwarders.iter() {
            metrics.push(
                json!({
                    "room_id": room_id,
                    "publisher_id": publisher_id,
                    "subscriber_id": subscriber_id,
                    "stats": forwarder.metrics()
        })
            );
        }

        json!({
            "total_forwarders": metrics.len(),
            "forwarders": metrics
        })
    }

    async fn send_to_user(state: &AppState, room_id: &str, user_id: &str, message: Message) {
        let rooms = state.rooms.read().await;

        if let Some(room) = rooms.get(room_id) {
            if let Some((sid, _)) = room.sessions.iter().find(|(_, uid)| uid.as_str() == user_id) {
                if let Some(sender) = room.senders.get(sid) {
                    let _ = sender.send(message);
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub rooms: Rooms,
    pub db: PgPool,
    pub turn_config: TurnConfig,
    pub watchers: RoomWatchers,
    pub track_repository: Arc<TrackRepository>,
}
