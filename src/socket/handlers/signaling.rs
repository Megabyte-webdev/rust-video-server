use axum::extract::ws::Message;
use serde_json::Value;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

use crate::state::AppState;

pub async fn handle_signaling(state: &AppState, room_id: &str, sender_id: &str, raw_msg: &str) {
    let Ok(value) = serde_json::from_str::<Value>(raw_msg) else {
        log::error!("Invalid signaling JSON string received");
        return;
    };

    let msg_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let target_user_id = value.get("target").and_then(|v| v.as_str());

    // SFU-specific signaling message types sent by the client SDK
    match msg_type {
        "PUB_OFFER" => {
            if let Some(sdp_str) = value.get("payload").and_then(|v| v.as_str()) {
                if let Ok(offer_sdp) = RTCSessionDescription::offer(sdp_str.to_string()) {
                    handle_sfu_pub_offer(state, room_id, sender_id, offer_sdp).await;
                }
            }
            return;
        }
        "SUB_ANSWER" => {
            if let Some(sdp_str) = value.get("payload").and_then(|v| v.as_str()) {
                if let Ok(answer_sdp) = RTCSessionDescription::answer(sdp_str.to_string()) {
                    handle_sfu_sub_answer(state, room_id, sender_id, answer_sdp).await;
                }
            }
            return;
        }
        "PUB_ICE" => {
            if let Some(candidate_str) = value.get("payload").and_then(|v| v.as_str()) {
                handle_sfu_ice(state, room_id, sender_id, candidate_str, true).await;
            }
            return;
        }
        "SUB_ICE" => {
            if let Some(candidate_str) = value.get("payload").and_then(|v| v.as_str()) {
                handle_sfu_ice(state, room_id, sender_id, candidate_str, false).await;
            }
            return;
        }
        _ => {}
    }

    // Direct P2P signaling relay fallback (Legacy Mesh / Targeted messages)
    let rooms = state.rooms.read().await;
    if let Some(room) = rooms.get(room_id) {
        let mut enriched = value.clone();
        enriched["sender"] = serde_json::json!(sender_id);
        let outbound = Message::Text(serde_json::to_string(&enriched).unwrap().into());

        if let Some(tid) = target_user_id {
            if let Some((sid, _)) = room.sessions.iter().find(|(_, uid)| uid.as_str() == tid) {
                if let Some(sender) = room.senders.get(sid) {
                    let _ = sender.send(outbound);
                }
            }
        }
    }
}

/// Handles incoming publisher offer from the client, responds with SFU_PUB_ANSWER
async fn handle_sfu_pub_offer(
    state: &AppState,
    room_id: &str,
    sender_id: &str,
    offer: RTCSessionDescription
) {
    let pc = {
        let rooms = state.rooms.read().await;

        rooms
            .get(room_id)
            .and_then(|r| r.server_peers.get(sender_id))
            .map(|sp| sp.publisher_pc.clone())
    };

    if let Some(pc) = pc {
        if let Err(e) = pc.set_remote_description(offer).await {
            log::error!(
                "[SFU] Failed to set remote description for publisher {}: {:?}",
                sender_id,
                e
            );
            return;
        }

        match pc.create_answer(None).await {
            Ok(answer) => {
                if pc.set_local_description(answer.clone()).await.is_ok() {
                    let msg = Message::Text(
                        serde_json::json!({
                    "type":"PUB_ANSWER",
                    "payload":answer.sdp
                })
                            .to_string()
                            .into()
                    );

                    let rooms = state.rooms.read().await;

                    if let Some(room) = rooms.get(room_id) {
                        if
                            let Some((sid, _)) = room.sessions
                                .iter()
                                .find(|(_, uid)| uid.as_str() == sender_id)
                        {
                            if let Some(tx) = room.senders.get(sid) {
                                let _ = tx.send(msg);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("Publisher answer failed {:?}", e);
            }
        }
    }
}

/// Handles incoming subscriber answer from the client for SFU-initiated downstream offers
async fn handle_sfu_sub_answer(
    state: &AppState,
    room_id: &str,
    sender_id: &str,
    answer: RTCSessionDescription
) {
    let pc = {
        let rooms = state.rooms.read().await;
        rooms
            .get(room_id)
            .and_then(|r| r.server_peers.get(sender_id))
            .map(|sp| sp.subscriber_pc.clone())
    };

    if let Some(pc) = pc {
        if let Err(e) = pc.set_remote_description(answer).await {
            log::error!(
                "[SFU] Failed to set remote description for subscriber {}: {:?}",
                sender_id,
                e
            );
        }
    }
}

/// Routes ICE Candidates to the correct Publisher or Subscriber PeerConnection
async fn handle_sfu_ice(
    state: &AppState,
    room_id: &str,
    sender_id: &str,
    candidate_str: &str,
    is_publisher: bool
) {
    let pc = {
        let rooms = state.rooms.read().await;

        rooms
            .get(room_id)
            .and_then(|r| r.server_peers.get(sender_id))
            .map(|sp| {
                if is_publisher { sp.publisher_pc.clone() } else { sp.subscriber_pc.clone() }
            })
    };

    if let Some(pc) = pc {
        let init = webrtc::ice_transport::ice_candidate::RTCIceCandidateInit {
            candidate: candidate_str.to_string(),
            ..Default::default()
        };
        if let Err(e) = pc.add_ice_candidate(init).await {
            log::error!("[SFU] Failed to add ICE candidate for {}: {:?}", sender_id, e);
        }
    }
}
