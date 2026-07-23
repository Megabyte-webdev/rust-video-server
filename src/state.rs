use std::{ collections::HashMap, sync::{ Arc } };

use serde_json::json;
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;
use webrtc::{
    peer_connection::RTCPeerConnection,
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::{ track_local::track_local_static_rtp::TrackLocalStaticRTP, track_remote::TrackRemote },
};
use crate::socket::{
    handlers::rtc_signalling::{ ForwarderConfig, SFUError, SFUResult, TrackForwarder },
    room_manager::{ ClientSender, Rooms },
};

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

// TRACK REPOSITORY: MANAGES ALL FORWARDERS
#[derive(Clone)]
pub struct TrackRepository {
    /// Map: (room_id, sender_id) → Vec<Forwarder>
    forwarders: Arc<RwLock<HashMap<(String, String), Vec<Arc<TrackForwarder>>>>>,
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
        room_id: &str,
        sender_id: &str,
        recipient_pc: Arc<RTCPeerConnection>,
        remote_track: Arc<TrackRemote>
    ) -> SFUResult<Arc<TrackForwarder>> {
        let track_kind = remote_track.kind().to_string();

        // Create codec capability
        let codec = match track_kind.as_str() {
            "audio" =>
                RTCRtpCodecCapability {
                    mime_type: "audio/opus".to_string(),
                    clock_rate: 48000,
                    channels: 2,
                    sdp_fmtp_line: String::new(),
                    rtcp_feedback: vec![],
                },
            "video" =>
                RTCRtpCodecCapability {
                    mime_type: "video/VP8".to_string(),
                    clock_rate: 90000,
                    channels: 0,
                    sdp_fmtp_line: String::new(),
                    rtcp_feedback: vec![],
                },
            _ => {
                return Err(SFUError::CodecMismatch(format!("Unknown track kind: {}", track_kind)));
            }
        };

        // Create local forwarding track with correct signature
        let track_id = format!("fwd-{}-{}", sender_id, Uuid::new_v4());
        let stream_id = format!("stream-{}", Uuid::new_v4());

        let local_track = Arc::new(TrackLocalStaticRTP::new(codec, track_id, stream_id));

        // Add to peer connection
        recipient_pc
            .add_track(local_track.clone()).await
            .map_err(|e| SFUError::TrackForwardingFailed(format!("Failed to add track: {}", e)))?;

        log::info!(
            "✅ Added forwarding track for {} → {} ({})",
            sender_id,
            Uuid::new_v4(),
            track_kind
        );

        // Create forwarder
        let forwarder = Arc::new(
            TrackForwarder::new(
                local_track,
                remote_track,
                sender_id.to_string(),
                self.config.clone()
            )
        );

        // Start forwarding
        forwarder.start().await?;

        // Store forwarder
        let key = (room_id.to_string(), sender_id.to_string());
        self.forwarders.write().await.entry(key).or_insert_with(Vec::new).push(forwarder.clone());

        Ok(forwarder)
    }

    /// Remove all forwarders for a sender
    pub async fn remove_sender_forwarders(&self, room_id: &str, sender_id: &str) {
        let key = (room_id.to_string(), sender_id.to_string());

        if let Some(forwarders) = self.forwarders.write().await.remove(&key) {
            log::info!(
                "Removing {} forwarders for {} in room {}",
                forwarders.len(),
                sender_id,
                room_id
            );

            for forwarder in forwarders {
                forwarder.stop().await;
            }
        }
    }

    /// Get metrics for all forwarders
    pub async fn get_all_metrics(&self) -> serde_json::Value {
        let forwarders = self.forwarders.read().await;
        let mut metrics = Vec::new();

        for ((room_id, sender_id), forwarder_list) in forwarders.iter() {
            for (idx, forwarder) in forwarder_list.iter().enumerate() {
                metrics.push(
                    json!({
                    "room_id": room_id,
                    "sender_id": sender_id,
                    "forwarder_index": idx,
                    "stats": forwarder.metrics()
                })
                );
            }
        }

        json!({
            "total_forwarders": metrics.len(),
            "forwarders": metrics
        })
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
