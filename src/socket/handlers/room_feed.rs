use axum::extract::ws::Message;

use crate::socket::room_manager::Room;

pub fn build_room_presence(room: &Room, room_id: &str, user_id: &str) -> Message {
    let is_host = room.host_id.as_deref() == Some(user_id);

    let approved = room.approved_users.contains(user_id);

    let can_join = room.is_open.unwrap_or(false) || is_host || approved;

    let participants = room.participants
        .values()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "isHost": p.is_host,
                "isPresenter": p.is_presenter,
                "micEnabled": p.mic_enabled,
                "camEnabled": p.cam_enabled
            })
        })
        .collect::<Vec<_>>();

    Message::Text(
        serde_json::json!({
            "type": "ROOM_PRESENCE_UPDATE",
            "room_id": room_id,

            "active": !room.sessions.is_empty(),
            "count": participants.len(),

            "isHost": is_host,
            "approved": approved,
            "canJoin": can_join,

            "participants": participants
        })
            .to_string()
            .into()
    )
}
