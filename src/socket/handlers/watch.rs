use axum::extract::ws::Message;

use crate::{ socket::room_manager::ClientSender, state::AppState };

pub async fn handle_watch_room(state: &AppState, room_id: &str, user_id: &str, tx: ClientSender) {
    let rooms = state.rooms.read().await;

    let Some(room) = rooms.get(room_id) else {
        let _ = tx.send(
            Message::Text(
                serde_json::json!({
                "type": "ROOM_PRESENCE_UPDATE",
                "room_id": room_id,
                "active": false,
                "count": 0,
                "participants": [],
                "canJoin": false,
                "approved": false,
                "isHost": false
            })
                    .to_string()
                    .into()
            )
        );

        return;
    };

    let is_host = room.host_id.as_deref() == Some(user_id);

    let is_approved = room.approved_users.contains(user_id);

    let can_join = room.is_open.unwrap_or(false) || is_host || is_approved;

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

    let payload = Message::Text(
        serde_json::json!({
            "type": "ROOM_PRESENCE_UPDATE",
            "room_id": room_id,
            "active": !room.sessions.is_empty(),
            "count": participants.len(),
            "isHost": is_host,
            "approved": is_approved,
            "canJoin": can_join,
            "participants": participants
        })
            .to_string()
            .into()
    );

    let _ = tx.send(payload);
}
