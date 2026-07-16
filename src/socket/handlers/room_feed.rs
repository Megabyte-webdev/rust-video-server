use axum::extract::ws::Message;
use serde_json::json;

use crate::{
    socket::{ handlers::broadcast_presence::broadcast_room_presence, room_manager::ClientSender },
    state::AppState,
};

pub async fn build_room_presence(
    state: &AppState,
    room_id: &str,
    user_id: &str
) -> Option<Message> {
    let rooms = state.rooms.read().await;
    let room = rooms.get(room_id)?;
    let is_host = room.host_id.as_deref() == Some(user_id);
    let approved = room.approved_users.contains(user_id);
    let can_join = room.is_open.unwrap_or(false) || is_host || approved;
    let total_count = room.participants.len();

    // only send max 5 participants
    let participants = room.participants
        .values()
        .take(5)
        .map(|p| {
            json!({
                    "id": p.id,
                    "name": p.name,
                    "isHost":
                        p.is_host,
                    "isPresenter":
                        p.is_presenter,
                    "micEnabled":
                        p.mic_enabled,
                    "camEnabled":
                        p.cam_enabled
                })
        })
        .collect::<Vec<_>>();

    Some(
        Message::Text(
            json!({
                "type":
                    "ROOM_PRESENCE_UPDATE",
                "room_id":
                    room_id,
                "active":
                    !room.sessions.is_empty(),
                "count":
                    total_count,
                "participants":
                    participants,
                "hasMoreParticipants":
                    total_count > 5,
                "isHost":
                    is_host,
                "approved":
                    approved,
                "canJoin":
                    can_join
            })
                .to_string()
                .into()
        )
    )
}

pub async fn register_room_watcher(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    client: ClientSender
) {
    {
        let mut rooms = state.rooms.write().await;

        let Some(room) = rooms.get_mut(room_id) else {
            return;
        };

        room.watchers.insert(user_id.to_string(), client);
    }

    // notify all watchers about new presence
    broadcast_room_presence(state, room_id).await;
}

pub async fn unregister_room_watcher(state: &AppState, room_id: &str, user_id: &str) {
    {
        let mut rooms = state.rooms.write().await;

        let Some(room) = rooms.get_mut(room_id) else {
            return;
        };

        room.watchers.remove(user_id);
    }

    // notify remaining watchers
    broadcast_room_presence(state, room_id).await;
}
