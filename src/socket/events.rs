use serde_json::json;
use sqlx::PgTransaction;

pub async fn log_join(
    tx: &mut PgTransaction<'_>,
    room_id: &str,
    user_id: &str,
    session_id: &str,
    name: &str
) -> Result<(), sqlx::Error> {
    sqlx
        ::query(
            r#"
        INSERT INTO room_events
        (room_id, session_id, user_id, event_type, payload)
        VALUES ($1, $2, $3, 'JOIN', $4)
        "#
        )
        .bind(room_id)
        .bind(session_id)
        .bind(user_id)
        .bind(json!({ "name": name }))
        .execute(&mut **tx).await?;

    Ok(())
}

pub async fn log_leave(
    tx: &mut PgTransaction<'_>,
    room_id: &str,
    user_id: &str,
    session_id: &str,
    name: &str
) -> Result<(), sqlx::Error> {
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
        .bind(json!({ "name": name }))
        .execute(&mut **tx).await?;

    Ok(())
}
