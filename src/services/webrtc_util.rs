use std::sync::Arc;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::RTCPeerConnection;

use crate::state::{ AppState, TrackSource };

pub async fn create_webrtc_api() -> Result<Arc<webrtc::api::API>, Box<dyn std::error::Error>> {
    // In webrtc 0.17.1, SettingEngine is private and embedded in APIBuilder
    // No need to configure it separately—APIBuilder handles defaults
    let api = APIBuilder::new().build();
    Ok(Arc::new(api))
}

pub async fn create_server_peer_connection(
    state: AppState,
    room_id: String,
    user_id: &str,
    is_publisher: bool
) -> Arc<RTCPeerConnection> {
    let mut media_engine = MediaEngine::default();
    media_engine.register_default_codecs().expect("Failed to register default codecs");

    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut media_engine).expect(
        "Failed to register default interceptors"
    );

    let api = APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build();

    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec![
                format!("stun:{}", state.turn_config.server),
                format!("turn:{}?transport=udp", state.turn_config.server)
            ],
            username: format!("sfu_user_{}", user_id),
            credential: state.turn_config.auth_secret.clone(),
            ..Default::default()
        }],
        ..Default::default()
    };

    let pc = Arc::new(
        api.new_peer_connection(config).await.expect("Failed to create PeerConnection")
    );
    if is_publisher {
        let state_clone = state.clone();
        let room_id_clone = room_id.clone();
        let user_id_clone = user_id.to_string();

        pc.on_track(
            Box::new(move |track, _receiver, _transceiver| {
                let state = state_clone.clone();
                let room_id = room_id_clone.clone();
                let user_id = user_id_clone.clone();

                Box::pin(async move {
                    let remote_track = track.clone();

                    let source = match remote_track.kind().to_string().as_str() {
                        "audio" => TrackSource::Audio,
                        "video" => TrackSource::Camera,
                        _ => TrackSource::Camera,
                    };

                    {
                        let mut rooms = state.rooms.write().await;

                        if let Some(room) = rooms.get_mut(&room_id) {
                            room.published_tracks
                                .entry(user_id.clone())
                                .or_default()
                                .push(remote_track.clone());
                        }
                    }

                    let subscriber_pcs = {
                        let rooms = state.rooms.read().await;

                        if let Some(room) = rooms.get(&room_id) {
                            room.server_peers
                                .iter()
                                .filter(|(uid, _)| **uid != user_id)
                                .map(|(uid, peer)| { (uid.clone(), peer.subscriber_pc.clone()) })
                                .collect::<Vec<_>>()
                        } else {
                            vec![]
                        }
                    };

                    for (subscriber_id, subscriber_pc) in subscriber_pcs {
                        if
                            let Err(err) = state.track_repository.add_forwarder(
                                &state,
                                &room_id,
                                &user_id,
                                &subscriber_id,
                                subscriber_pc,
                                source.clone(),
                                remote_track.clone()
                            ).await
                        {
                            log::error!("Forwarding failed to {}: {:?}", subscriber_id, err);
                        }
                    }
                })
            })
        );
    }
    pc
}
