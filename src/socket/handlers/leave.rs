use axum::extract::ws::Message;
use serde_json::json;

use crate::{
    services::attendance_service::AttendanceService,
    socket::events::log_leave,
    state::AppState,
    utils::error::log_error,
};

pub async fn handle_leave(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    name: String,
    session_id: &str
) {
    println!("🔥 HANDLE_LEAVE CALLED");

    let recipients;

    {
        let mut rooms = state.rooms.write().await;

        if let Some(room) = rooms.get_mut(room_id) {
            let still_connected = room.sessions
                .iter()
                .any(|(sid, uid)| sid != session_id && uid == user_id);

            recipients = room.sessions
                .iter()
                .filter(|(sid, uid)| *sid != session_id && *uid != user_id)
                .filter_map(|(sid, _)| room.senders.get(sid))
                .cloned()
                .collect::<Vec<_>>();

            room.sessions.remove(session_id);
            room.senders.remove(session_id);

            if !still_connected {
                room.participants.remove(user_id);
            }

            room.senders.retain(|sid, _| room.sessions.contains_key(sid));

            if room.senders.is_empty() {
                println!("🧹 REMOVING EMPTY ROOM");
                rooms.remove(room_id);
            }
        } else {
            return;
        }
    }

    // ---------------- BROADCAST ----------------
    let leave_msg = Message::Text(
        json!({
            "type": "USER_LEFT",
            "participant": {
                "id": user_id,
                "name": name,
                "session_id": session_id
            }
        })
            .to_string()
            .into()
    );

    for tx in recipients {
        let _ = tx.send(leave_msg.clone());
    }

    // ---------------- DB EVENTS ----------------
    log_error(
        sqlx
            ::query(
                r#"
            INSERT INTO room_events
            (room_id, session_id, user_id, event_type, payload)
            VALUES ($1, $2, $3, 'LEAVE', $4)
            "#
            )
            .bind(room_id)
            .bind(session_id)
            .bind(user_id)
            .bind(json!({
            "name": name,
            "reason": "socket_closed"
        }))
            .execute(&state.db).await,
        "Leaving:ROOM_EVENT_UPDATE"
    );

    log_error(
        sqlx
            ::query(
                r#"
            UPDATE room_sessions
            SET ended_at = NOW()
            WHERE id = $1
            "#
            )
            .bind(session_id)
            .execute(&state.db).await,
        "Leaving:ROOM_SESSION_UPDATE"
    );

    log_error(
        sqlx
            ::query(
                r#"
            UPDATE participant_sessions
            SET left_at = NOW(),
                last_seen = NOW()
            WHERE id = $1
            "#
            )
            .bind(session_id)
            .execute(&state.db).await,
        "Leaving:PARTICIPANT_SESSION_UPDATE"
    );

    AttendanceService::mark_leave(&state.db, room_id, user_id).await.ok();

    log_leave(state, room_id, user_id, name, session_id).await;
}
