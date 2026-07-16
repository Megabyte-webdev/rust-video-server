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
        let mut watchers = state.watchers.write().await;

        watchers.entry(room_id.to_string()).or_default().insert(user_id.to_string(), client);
    }

    // If the room already exists, send an update.
    broadcast_room_presence(state, room_id).await;
}

pub async fn unregister_room_watcher(state: &AppState, room_id: &str, user_id: &str) {
    let should_remove_room = {
        let mut watchers = state.watchers.write().await;
        let Some(room_watchers) = watchers.get_mut(room_id) else {
            return;
        };
        room_watchers.remove(user_id);
        room_watchers.is_empty()
    };

    if should_remove_room {
        let mut watchers = state.watchers.write().await;
        watchers.remove(room_id);
    }
}
