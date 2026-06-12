use axum::extract::ws::Message;
use serde_json::json;

use crate::{
    services::attendance_service::AttendanceService,
    state::AppState,
    utils::error::error_msg,
};
pub async fn handle_leave(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    name: String,
    session_id: &str
) {
    println!("🔥 HANDLE_LEAVE CALLED");

    let mut recipients = vec![];

    // ---------------- MEMORY LOCK ----------------
    {
        let mut rooms = state.rooms.write().await;

        if let Some(room) = rooms.get_mut(room_id) {
            recipients = room.senders
                .iter()
                .filter_map(|(sid, tx)| {
                    let owner = room.sessions.get(sid)?;
                    if sid == session_id || owner == user_id {
                        return None;
                    }
                    Some(tx.clone())
                })
                .collect();

            room.sessions.remove(session_id);
            room.senders.remove(session_id);

            let still_connected = room.sessions.values().any(|uid| uid == user_id);

            if !still_connected {
                room.participants.remove(user_id);
            }

            if room.sessions.is_empty() {
                rooms.remove(room_id);
            }
        } else {
            return;
        }
    }

    // ---------------- DB TRANSACTION ----------------
    let mut tx_db = match state.db.begin().await {
        Ok(t) => t,
        Err(_) => {
            return;
        }
    };

    let _ = sqlx
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
        .bind(json!({ "name": name }))
        .execute(&mut *tx_db).await;

    let _ = sqlx
        ::query("UPDATE room_sessions SET ended_at = NOW() WHERE id = $1")
        .bind(session_id)
        .execute(&mut *tx_db).await;

    let _ = sqlx
        ::query(
            r#"
        UPDATE participant_sessions
        SET left_at = NOW(), last_seen = NOW()
        WHERE id = $1
        "#
        )
        .bind(session_id)
        .execute(&mut *tx_db).await;

    if let Err(_) = AttendanceService::mark_leave(&state.db, room_id, user_id).await {
        return;
    }
    let _ = tx_db.commit().await;

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

    println!("✅ LEAVE COMPLETE");
}
