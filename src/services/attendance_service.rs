use sqlx::PgPool;

pub struct AttendanceService;

impl AttendanceService {
    // ---------------- JOIN ----------------
    pub async fn mark_join(
        db: &PgPool,
        room_id: &str,
        user_id: &str,
        name: &str
    ) -> Result<(), sqlx::Error> {
        match
            sqlx
                ::query(
                    r#"
            INSERT INTO meeting_attendance (
                room_id,
                user_id,
                name,
                first_joined_at,
                session_started_at,
                last_left_at,
                reconnect_count,
                status,
                total_active_seconds
            )
            VALUES (
                $1, $2, $3,
                NOW(),
                NOW(),
                NULL,
                1,
                'active',
                0
            )
            ON CONFLICT (room_id, user_id)
            DO UPDATE SET
                reconnect_count = meeting_attendance.reconnect_count + 1,
                session_started_at = NOW(),
                status = 'active'
            "#
                )
                .bind(room_id)
                .bind(user_id)
                .bind(name)
                .execute(db).await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                eprintln!("Attendance insert failed: {:#?}", e);
                Err(e)
            }
        }
    }

    // ---------------- LEAVE ----------------
    pub async fn mark_leave(db: &PgPool, room_id: &str, user_id: &str) -> Result<(), sqlx::Error> {
        sqlx
            ::query(
                r#"
            UPDATE meeting_attendance
            SET
                last_left_at = NOW(),
                status = 'left',
                total_active_seconds =
                    COALESCE(total_active_seconds, 0)
                    + COALESCE(
                        EXTRACT(EPOCH FROM (NOW() - session_started_at))::BIGINT,
                        0
                    )
            WHERE room_id = $1 AND user_id = $2
            "#
            )
            .bind(room_id)
            .bind(user_id)
            .execute(db).await?;

        Ok(())
    }

    // ---------------- HEARTBEAT / ACTIVE ----------------
    pub async fn mark_active(db: &PgPool, room_id: &str, user_id: &str) -> Result<(), sqlx::Error> {
        sqlx
            ::query(
                r#"
            UPDATE meeting_attendance
            SET
                status = 'active'
            WHERE room_id = $1 AND user_id = $2
            "#
            )
            .bind(room_id)
            .bind(user_id)
            .execute(db).await?;

        Ok(())
    }
}
