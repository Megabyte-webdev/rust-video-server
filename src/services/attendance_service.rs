use sqlx::PgPool;

use crate::utils::error::log_error;

pub struct AttendanceService;

impl AttendanceService {
    pub async fn mark_join(db: &PgPool, room_id: &str, user_id: &str) -> Result<(), sqlx::Error> {
        sqlx
            ::query(
                r#"
            INSERT INTO meeting_attendance (
                room_id,
                user_id,
                first_joined_at,
                last_left_at,
                reconnect_count,
                status
            )
            VALUES ($1, $2, NOW(), NULL, 1, 'active')
            ON CONFLICT (room_id, user_id)
            DO UPDATE SET
                reconnect_count = meeting_attendance.reconnect_count + 1,
                status = 'active'
            "#
            )
            .bind(room_id)
            .bind(user_id)
            .execute(db).await?;

        Ok(())
    }

    pub async fn mark_leave(
        db: &sqlx::PgPool,
        room_id: &str,
        user_id: &str
    ) -> Result<(), sqlx::Error> {
        sqlx
            ::query(
                r#"
        UPDATE meeting_attendance
        SET
            last_left_at = NOW(),
            status = 'left',
            total_active_seconds =
                COALESCE(total_active_seconds, 0) +
                EXTRACT(EPOCH FROM (NOW() - first_joined_at))::BIGINT
        WHERE room_id = $1 AND user_id = $2
        "#
            )
            .bind(room_id)
            .bind(user_id)
            .execute(db).await?;

        Ok(())
    }

    pub async fn mark_active(db: &PgPool, room_id: &str, user_id: &str) {
        log_error(
            sqlx
                ::query(
                    r#"
            UPDATE meeting_attendance
            SET status = 'active'
            WHERE room_id = $1 AND user_id = $2
            "#
                )
                .bind(room_id)
                .bind(user_id)
                .execute(db).await,
            "Attendace Marking"
        );
    }
}
