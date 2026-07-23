use std::sync::{ Arc, atomic::{ AtomicU64, Ordering } };
use tokio::sync::{ Mutex };
use tokio::task::JoinHandle;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::track::track_remote::TrackRemote;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use serde_json::json;
use thiserror::Error;
use std::time::{ SystemTime, UNIX_EPOCH };
use uuid::Uuid;

// ERROR TYPES
#[derive(Error, Debug)]
pub enum SFUError {
    #[error("Track forwarding failed: {0}")] TrackForwardingFailed(String),
    #[error("RTP read error: {0}")] RtpReadError(String),
    #[error("RTP write error: {0}")] RtpWriteError(String),
    #[error("Room not found: {0}")] RoomNotFound(String),
    #[error("Peer not found: {0}")] PeerNotFound(String),
    #[error("Track not found: {0}")] TrackNotFound(String),
    #[error("Codec mismatch: {0}")] CodecMismatch(String),
    #[error("Connection error: {0}")] ConnectionError(String),
    #[error("Resource exhausted: {0}")] ResourceExhausted(String),
}

pub type SFUResult<T> = Result<T, SFUError>;

// METRICS & MONITORING

#[derive(Clone)]
pub struct ForwarderMetrics {
    packets_received: Arc<AtomicU64>,
    packets_forwarded: Arc<AtomicU64>,
    bytes_forwarded: Arc<AtomicU64>,
    packets_dropped: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
    created_at: i64,
}

impl ForwarderMetrics {
    pub fn new() -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;

        Self {
            packets_received: Arc::new(AtomicU64::new(0)),
            packets_forwarded: Arc::new(AtomicU64::new(0)),
            bytes_forwarded: Arc::new(AtomicU64::new(0)),
            packets_dropped: Arc::new(AtomicU64::new(0)),
            errors: Arc::new(AtomicU64::new(0)),
            created_at: now,
        }
    }

    pub fn record_received(&self) {
        self.packets_received.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_forwarded(&self, bytes: usize) {
        self.packets_forwarded.fetch_add(1, Ordering::Relaxed);
        self.bytes_forwarded.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn record_dropped(&self) {
        self.packets_dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get_stats(&self) -> serde_json::Value {
        json!({
            "packets_received": self.packets_received.load(Ordering::Relaxed),
            "packets_forwarded": self.packets_forwarded.load(Ordering::Relaxed),
            "bytes_forwarded": self.bytes_forwarded.load(Ordering::Relaxed),
            "packets_dropped": self.packets_dropped.load(Ordering::Relaxed),
            "errors": self.errors.load(Ordering::Relaxed),
            "uptime_seconds": (SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64) - self.created_at,
        })
    }
}

// TRACK FORWARDER: CONVERTS TrackRemote → TrackLocal

pub struct TrackForwarder {
    local_track: Arc<TrackLocalStaticRTP>,
    remote_track: Arc<TrackRemote>,

    publisher_id: String,
    subscriber_id: String,

    track_kind: String,
    metrics: ForwarderMetrics,
    forwarding_task: Mutex<Option<JoinHandle<()>>>,
    config: ForwarderConfig,
}

#[derive(Clone)]
pub struct ForwarderConfig {
    /// Max packets to buffer before dropping
    pub max_buffer: usize,
    /// Enable stats logging every N packets
    pub stats_interval: usize,
    /// Timeout for reads (helps detect dead connections)
    pub read_timeout_ms: u64,
}

impl Default for ForwarderConfig {
    fn default() -> Self {
        Self {
            max_buffer: 100,
            stats_interval: 1000,
            read_timeout_ms: 5000,
        }
    }
}

impl TrackForwarder {
    /// Create a new forwarder that bridges remote → local track
    pub fn new(
        local_track: Arc<TrackLocalStaticRTP>,
        remote_track: Arc<TrackRemote>,
        publisher_id: String,
        subscriber_id: String,
        config: ForwarderConfig
    ) -> Self {
        let track_kind = remote_track.kind().to_string();
        log::info!("Creating forwarder {} -> {} ({})", publisher_id, subscriber_id, track_kind);
        Self {
            local_track,
            remote_track,
            publisher_id,
            subscriber_id,
            track_kind,
            metrics: ForwarderMetrics::new(),
            forwarding_task: Mutex::new(None),
            config,
        }
    }

    /// Start the background RTP forwarding task
    pub async fn start(&self) -> SFUResult<()> {
        let remote_track = self.remote_track.clone();
        let local_track = self.local_track.clone();
        let metrics = self.metrics.clone();
        let publisher_id = self.publisher_id.clone();
        let subscriber_id = self.subscriber_id.clone();
        let track_kind = self.track_kind.clone();
        let config = self.config.clone();

        let task = tokio::spawn(async move {
            if
                let Err(e) = Self::forwarding_loop(
                    remote_track,
                    local_track,
                    metrics,
                    publisher_id,
                    subscriber_id,
                    track_kind,
                    config
                ).await
            {
                log::error!("Forwarding loop error: {}", e);
            }
        });

        *self.forwarding_task.lock().await = Some(task);
        Ok(())
    }

    /// Background loop that reads from remote and writes to local
    async fn forwarding_loop(
        remote_track: Arc<TrackRemote>,
        local_track: Arc<TrackLocalStaticRTP>,
        metrics: ForwarderMetrics,
        publisher_id: String,
        subscriber_id: String,
        track_kind: String,
        config: ForwarderConfig
    ) -> SFUResult<()> {
        log::info!(
            "📤 Forwarding loop started for {} {} ({})",
            publisher_id,
            subscriber_id,
            track_kind
        );

        let mut packet_count = 0;
        let mut error_count = 0;
        const MAX_CONSECUTIVE_ERRORS: u32 = 10;

        loop {
            // Read RTP packets from remote track
            let packet = match
                tokio::time::timeout(
                    std::time::Duration::from_millis(config.read_timeout_ms),
                    remote_track.read_rtp()
                ).await
            {
                Ok(Ok((packet, _))) => packet,
                Ok(Err(_)) => {
                    // Remote track ended
                    log::info!(
                        "⚠️  Remote track ended for publisher {} subscriber {} after packets {}",
                        publisher_id,
                        subscriber_id,
                        packet_count
                    );
                    break;
                }
                Err(_) => {
                    // Timeout - connection may be dead
                    error_count += 1;
                    if error_count >= MAX_CONSECUTIVE_ERRORS {
                        log::warn!(
                            "❌ Multiple timeouts for {} {} - closing connection",
                            publisher_id,
                            subscriber_id
                        );
                        return Err(SFUError::RtpReadError("Too many read timeouts".to_string()));
                    }
                    continue;
                }
            };

            // Reset error count on successful read
            error_count = 0;
            metrics.record_received();

            // Forward packet
            let packet_size = packet.payload.len();

            match local_track.write_rtp(&packet).await {
                Ok(_) => {
                    packet_count += 1;
                    metrics.record_forwarded(packet_size);

                    // Log stats periodically
                    if packet_count % config.stats_interval == 0 {
                        log::debug!(
                            "📊 {} [{}/{}] Forwarded {} packets ({} bytes)",
                            publisher_id,
                            subscriber_id,
                            track_kind,
                            packet_count,
                            metrics.bytes_forwarded.load(Ordering::Relaxed)
                        );
                    }
                }
                Err(e) => {
                    metrics.record_dropped();
                    metrics.record_error();
                    log::warn!(
                        "❌ Failed to forward packet for {} {}: {}",
                        publisher_id,
                        subscriber_id,
                        e
                    );

                    // Don't fail on individual packet errors - just skip
                    // This is important for resilience
                }
            }
        }

        log::info!(
            "✅ Forwarding completed for {} {} (total: {} packets)",
            publisher_id,
            subscriber_id,
            packet_count
        );
        Ok(())
    }

    /// Stop forwarding and cleanup
    pub async fn stop(&self) {
        if let Some(task) = self.forwarding_task.lock().await.take() {
            log::info!("Stopping forwarder for {} {}", self.publisher_id, self.subscriber_id);
            task.abort();
        }
    }

    /// Get current metrics
    pub fn metrics(&self) -> serde_json::Value {
        self.metrics.get_stats()
    }
}

// CONFIGURATION FOR PRODUCTION DEPLOYMENT

pub struct SFUConfig {
    /// Max forwarders per peer (limit for single sender)
    pub max_forwarders_per_peer: usize,
    /// Max concurrent rooms
    pub max_concurrent_rooms: usize,
    /// Max peers per room
    pub max_peers_per_room: usize,
    /// Track forwarder configuration
    pub forwarder_config: ForwarderConfig,
    /// Enable metrics collection
    pub enable_metrics: bool,
    /// Periodic metrics log interval (seconds)
    pub metrics_log_interval_secs: u64,
}

impl Default for SFUConfig {
    fn default() -> Self {
        Self {
            max_forwarders_per_peer: 4, // Max 4 concurrent streams per participant
            max_concurrent_rooms: 1000,
            max_peers_per_room: 100,
            forwarder_config: ForwarderConfig::default(),
            enable_metrics: true,
            metrics_log_interval_secs: 30,
        }
    }
}

// HELPER: CREATE FORWARDING TRACK

/// Utility to create a track ready for forwarding
pub fn create_forwarding_track(
    kind: &str,
    track_id: String
) -> SFUResult<Arc<TrackLocalStaticRTP>> {
    let codec = match kind {
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
            return Err(SFUError::CodecMismatch(format!("Unsupported track kind: {}", kind)));
        }
    };

    let stream_id = format!("stream-{}", Uuid::new_v4());
    Ok(Arc::new(TrackLocalStaticRTP::new(codec, track_id, stream_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = ForwarderMetrics::new();
        metrics.record_received();
        metrics.record_forwarded(100);

        let stats = metrics.get_stats();
        assert_eq!(stats["packets_received"], 1);
        assert_eq!(stats["packets_forwarded"], 1);
        assert_eq!(stats["bytes_forwarded"], 100);
    }

    #[test]
    fn test_config_defaults() {
        let config = SFUConfig::default();
        assert_eq!(config.max_peers_per_room, 100);
        assert_eq!(config.max_concurrent_rooms, 1000);
    }
}
