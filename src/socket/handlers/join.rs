use std::collections::HashMap;
use axum::extract::ws::Message;
use serde_json::json;
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    services::attendance_service::AttendanceService,
    socket::{ events::log_join, room_manager::{ ParticipantState, Room } },
    state::AppState,
    utils::error::log_error,
};

pub async fn handle_join(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    name: &str,
    tx: UnboundedSender<Message>,
    incoming_session_id: &str
) {
    println!("🟢 JOIN STARTED: {} -> {}", user_id, room_id);

    // ---------------- CREATE ROOM SESSION ----------------
    let rsid = uuid::Uuid::new_v4().to_string();

    log_error(
        sqlx
            ::query(
                r#"
            INSERT INTO room_sessions (id, room_id, started_at)
            VALUES ($1, $2, NOW())
            ON CONFLICT (id) DO NOTHING
            "#
            )
            .bind(&rsid)
            .bind(room_id)
            .execute(&state.db).await,
        "Insert Room Session"
    );

    AttendanceService::mark_join(&state.db, room_id, user_id).await.ok();

    // ---------------- UPSERT PARTICIPANT SESSION ----------------
    log_error(
        sqlx
            ::query(
                r#"
            INSERT INTO participant_sessions
            (id, user_id, room_id, room_session_id, joined_at, last_seen)
            VALUES ($1, $2, $3, $4, NOW(), NOW())
            ON CONFLICT (id)
            DO UPDATE SET
                last_seen = NOW()
            "#
            )
            .bind(incoming_session_id)
            .bind(user_id)
            .bind(room_id)
            .bind(&rsid)
            .execute(&state.db).await,
        "Insert Participant Session"
    );

    // ---------------- UPSERT PARTICIPANT ----------------
    log_error(
        sqlx
            ::query(
                r#"
            INSERT INTO participants (id, room_id, name, first_joined_at, last_seen)
            VALUES ($1, $2, $3, NOW(), NOW())
            ON CONFLICT (id, room_id)
            DO UPDATE SET
                last_seen = NOW(),
                name = EXCLUDED.name,
                first_joined_at = participants.first_joined_at
            "#
            )
            .bind(user_id)
            .bind(room_id)
            .bind(name)
            .execute(&state.db).await,
        "Insert Participant"
    );

    // ---------------- ROOM LOCK ----------------
    let mut rooms = state.rooms.write().await;

    let room = rooms.entry(room_id.to_string()).or_insert(Room {
        participants: HashMap::new(),
        sessions: HashMap::new(),
        senders: HashMap::new(),
    });

    println!("📌 BEFORE: {:?}", room.participants.keys());

    // ---------------- CLEAN OLD SESSIONS FOR USER ----------------
    let old_sessions: Vec<String> = room.sessions
        .iter()
        .filter(|(_, uid)| *uid == user_id)
        .map(|(sid, _)| sid.clone())
        .collect();

    for sid in old_sessions {
        room.sessions.remove(&sid);
        room.senders.remove(&sid);
    }

    // ---------------- REGISTER NEW SESSION ----------------
    let session_id = incoming_session_id.to_string();

    room.sessions.insert(session_id.clone(), user_id.to_string());

    room.participants.insert(user_id.to_string(), ParticipantState {
        id: user_id.to_string(),
        name: name.to_string(),
        session_id: session_id.clone(),
        last_seen: chrono::Utc::now().timestamp() as u64,
    });

    room.senders.insert(session_id.clone(), tx.clone());

    println!("📌 AFTER: {:?}", room.participants.keys());

    // ---------------- EXISTING USERS ----------------
    let existing: Vec<_> = room.participants
        .values()
        .filter(|p| p.id != user_id)
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "session_id": p.session_id
            })
        })
        .collect();

    let _ = tx.send(
        Message::Text(
            json!({
                "type": "EXISTING_USERS",
                "participants": existing
            })
                .to_string()
                .into()
        )
    );

    // ---------------- BROADCAST JOIN ----------------
    let join_msg = Message::Text(
        json!({
            "type": "USER_JOINED",
            "participant": {
                "id": user_id,
                "name": name,
                "session_id": session_id
            }
        })
            .to_string()
            .into()
    );

    for (sid, sender) in room.senders.iter() {
        if let Some(owner) = room.sessions.get(sid) {
            if owner == user_id {
                continue;
            }

            let _ = sender.send(join_msg.clone());
        }
    }

    log_join(state, room_id, user_id, name, &session_id).await;

    println!("✅ JOIN COMPLETE");
}
