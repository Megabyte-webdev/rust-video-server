use axum::extract::ws::Message;
use serde_json::json;

use crate::{
    services::attendance_service::AttendanceService,
    socket::{ events::log_leave, handlers::broadcast_presence::broadcast_room_presence },
    state::AppState,
};

pub async fn handle_leave(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    name: String,
    session_id: &str
) {
    println!("HANDLE_LEAVE CALLED for user {}", user_id);

    let mut recipients = vec![];
    let still_connected;

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
            broadcast_room_presence(state, room_id).await;

            still_connected = room.sessions.values().any(|uid| uid == user_id);
            if !still_connected {
                room.participants.remove(user_id);
            }

            // cleanup pending
            let to_remove: Vec<_> = room.pending_requests
                .iter()
                .filter(|(_, req)| req.user_id == user_id)
                .map(|(id, _)| id.clone())
                .collect();

            for id in to_remove {
                room.pending_requests.remove(&id);
            }

            // cleanup approved
            // room.approved_users.remove(user_id);

            if room.sessions.is_empty() {
                println!("📭 Room {} empty, removing", room_id);
                rooms.remove(room_id);
            }
        } else {
            println!("Room {} not found in memory", room_id);
            return;
        }
    }

    // ---------------- DB TRANSACTION ----------------
    let mut tx_db = match state.db.begin().await {
        Ok(t) => t,
        Err(_) => {
            println!("Failed to start DB transaction");
            return;
        }
    };
    if let Err(e) = log_leave(&mut tx_db, room_id, user_id, session_id, &name).await {
        eprintln!("Failed to log leave event: {:?}", e);
        let _ = tx_db.rollback().await;
        return;
    }

    if
        let Err(e) = sqlx
            ::query(
                r#"
        UPDATE room_sessions
        SET ended_at = NOW()
        WHERE id = $1
        "#
            )
            .bind(session_id)
            .execute(&mut *tx_db).await
    {
        eprintln!("Failed to update room_sessions: {:?}", e);
        let _ = tx_db.rollback().await;
        return;
    }

    if
        let Err(e) = sqlx
            ::query(
                r#"
        UPDATE participant_sessions
        SET left_at = NOW(), last_seen = NOW()
        WHERE id = $1
        "#
            )
            .bind(session_id)
            .execute(&mut *tx_db).await
    {
        eprintln!("Failed participant_sessions update: {:?}", e);
        let _ = tx_db.rollback().await;
        return;
    }

    if let Err(e) = AttendanceService::mark_leave(&state.db, room_id, user_id).await {
        eprintln!("Failed attendance update: {:?}", e);
        let _ = tx_db.rollback().await;
        return;
    }
    if let Err(e) = tx_db.commit().await {
        eprintln!("Transaction commit failed: {:?}", e);
        return;
    }

    // ONLY BROADCAST IF USER TRULY LEFT
    if !still_connected {
        // ← Add this gate
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

        println!("USER_LEFT broadcast for {}", user_id);
    } else {
        println!(
            "Stale session {} removed for {}, but user still has active sessions",
            session_id,
            user_id
        );
    }

    println!("LEAVE COMPLETE for user {}", user_id);
}
