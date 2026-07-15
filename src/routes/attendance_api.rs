use axum::{ Json, extract::{ Path, Query, State }, http::StatusCode };
use chrono::{ DateTime, Utc };

use crate::{
    services::pagination::{
        AttendanceListResponse,
        AttendanceQuery,
        AttendanceRecord,
        DetailedParticipantInfo,
        DetailedParticipantResponse,
        ErrorResponse,
        PaginationMeta,
        PaginationQuery,
        ParticipantSessionInfo,
        ParticipantStats,
        ParticipantStatsResponse,
        RoomSession,
        RoomSessionResponse,
    },
    state::AppState,
};

pub async fn get_attendance(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(q): Query<AttendanceQuery>
) -> Result<Json<AttendanceListResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Validate pagination
    let page = q.page.max(1);
    let limit = q.limit.clamp(1, 100);
    let offset = (page - 1) * limit;

    // Get total count first
    let count_result: Result<(i64,), _> = sqlx
        ::query_as("SELECT COUNT(*) FROM participants WHERE room_id = $1")
        .bind(&room_id)
        .fetch_one(&state.db).await;

    let total_count = match count_result {
        Ok((count,)) => count,
        Err(e) => {
            eprintln!("get_attendance count error: {:?}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to fetch attendance records".to_string(),
                    code: "ATTENDANCE_FETCH_FAILED".to_string(),
                }),
            ));
        }
    };

    let query_str =
        r#"
        SELECT 
            p.id as user_id,
            p.name,
            p.first_joined_at as joined_at,
            p.last_seen as left_at,
            EXTRACT(EPOCH FROM (p.last_seen - p.first_joined_at))::bigint as duration_seconds,
            (SELECT COUNT(DISTINCT ps.id) FROM participant_sessions ps WHERE ps.user_id = p.id AND ps.room_id = $1)::int as session_count,
            CASE WHEN p.last_seen > NOW() - INTERVAL '5 minutes' THEN true ELSE false END as is_active
        FROM participants p
        WHERE p.room_id = $1
        ORDER BY p.first_joined_at DESC
        LIMIT $2 OFFSET $3
    "#;

    match
        sqlx
            ::query_as::<_, (String, String, DateTime<Utc>, DateTime<Utc>, i64, i32, bool)>(
                query_str
            )
            .bind(&room_id)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&state.db).await
    {
        Ok(rows) => {
            let room_name: String = sqlx
                ::query_scalar("SELECT name FROM rooms WHERE id = $1")
                .bind(&room_id)
                .fetch_optional(&state.db).await
                .unwrap_or(None)
                .unwrap_or_else(|| "Unknown Room".to_string());

            let active_count = rows
                .iter()
                .filter(|(_, _, _, _, _, _, active)| *active)
                .count() as i32;

            let records: Vec<AttendanceRecord> = rows
                .into_iter()
                .map(
                    |(
                        user_id,
                        name,
                        joined_at,
                        left_at,
                        duration_seconds,
                        session_count,
                        is_active,
                    )| {
                        AttendanceRecord {
                            user_id,
                            name,
                            joined_at,
                            left_at: Some(left_at),
                            duration_seconds: Some(duration_seconds),
                            session_count,
                            is_active,
                        }
                    }
                )
                .collect();

            let pagination = PaginationMeta::new(page, limit, total_count);

            Ok(
                Json(AttendanceListResponse {
                    room_id: room_id.clone(),
                    room_name,
                    total_participants: total_count as i32,
                    active_participants: active_count,
                    records,
                    pagination,
                    fetched_at: Utc::now(),
                })
            )
        }
        Err(e) => {
            eprintln!("get_attendance error: {:?}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to fetch attendance records".to_string(),
                    code: "ATTENDANCE_FETCH_FAILED".to_string(),
                }),
            ))
        }
    }
}

pub async fn get_participants(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(q): Query<PaginationQuery>
) -> Result<Json<ParticipantStatsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let page = q.page.max(1);
    let limit = q.limit.clamp(1, 100);
    let offset = (page - 1) * limit;

    // Get total count
    let count_result: Result<(i64,), _> = sqlx
        ::query_as("SELECT COUNT(*) FROM participant_sessions WHERE room_id = $1")
        .bind(&room_id)
        .fetch_one(&state.db).await;

    let total_count = match count_result {
        Ok((count,)) => count,
        Err(e) => {
            eprintln!("get_participants count error: {:?}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to fetch participants".to_string(),
                    code: "PARTICIPANTS_FETCH_FAILED".to_string(),
                }),
            ));
        }
    };

    let query_str =
        r#"
        SELECT 
            ps.user_id,
            ps.name,
            ps.id as session_id,
            ps.joined_at,
            ps.last_seen,
            EXTRACT(EPOCH FROM (ps.last_seen - ps.joined_at))::bigint as time_in_room_seconds
        FROM participant_sessions ps
        WHERE ps.room_id = $1
        ORDER BY ps.joined_at ASC
        LIMIT $2 OFFSET $3
    "#;

    match
        sqlx
            ::query_as::<_, (String, String, String, DateTime<Utc>, DateTime<Utc>, Option<i64>)>(
                query_str
            )
            .bind(&room_id)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&state.db).await
    {
        Ok(rows) => {
            let participants: Vec<ParticipantStats> = rows
                .into_iter()
                .map(|(user_id, name, session_id, joined_at, last_seen, duration)| {
                    ParticipantStats {
                        user_id,
                        name,
                        session_id,
                        joined_at,
                        last_seen,
                        time_in_room_seconds: duration,
                    }
                })
                .collect();

            let active_count = participants.len() as i32;
            let pagination = PaginationMeta::new(page, limit, total_count);

            Ok(
                Json(ParticipantStatsResponse {
                    room_id: room_id.clone(),
                    participants,
                    total_count: total_count as i32,
                    active_count,
                    pagination,
                })
            )
        }
        Err(e) => {
            eprintln!("get_participants error: {:?}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to fetch participants".to_string(),
                    code: "PARTICIPANTS_FETCH_FAILED".to_string(),
                }),
            ))
        }
    }
}

pub async fn get_room_sessions(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(q): Query<PaginationQuery>
) -> Result<Json<RoomSessionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let page = q.page.max(1);
    let limit = q.limit.clamp(1, 100);
    let offset = (page - 1) * limit;

    // Get total count
    let count_result: Result<(i64,), _> = sqlx
        ::query_as("SELECT COUNT(*) FROM room_sessions WHERE room_id = $1")
        .bind(&room_id)
        .fetch_one(&state.db).await;

    let total_count = match count_result {
        Ok((count,)) => count,
        Err(e) => {
            eprintln!("get_room_sessions count error: {:?}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to fetch room sessions".to_string(),
                    code: "SESSIONS_FETCH_FAILED".to_string(),
                }),
            ));
        }
    };

    let query_str =
        r#"
        SELECT 
            rs.id as session_id,
            rs.room_id,
            rs.started_at,
            rs.ended_at,
            EXTRACT(EPOCH FROM (COALESCE(rs.ended_at, NOW()) - rs.started_at))::bigint as duration_seconds,
            (SELECT COUNT(DISTINCT user_id) FROM participant_sessions WHERE room_session_id = rs.id)::int as participant_count,
            0::int as peak_concurrent
        FROM room_sessions rs
        WHERE rs.room_id = $1
        ORDER BY rs.started_at DESC
        LIMIT $2 OFFSET $3
    "#;

    match
        sqlx
            ::query_as::<_, (String, String, DateTime<Utc>, Option<DateTime<Utc>>, i64, i32, i32)>(
                query_str
            )
            .bind(&room_id)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&state.db).await
    {
        Ok(rows) => {
            let sessions: Vec<RoomSession> = rows
                .into_iter()
                .map(|(session_id, room_id, started_at, ended_at, duration, pc, peak)| {
                    RoomSession {
                        session_id,
                        room_id,
                        started_at,
                        ended_at,
                        duration_seconds: Some(duration),
                        participant_count: pc,
                        peak_concurrent: peak,
                    }
                })
                .collect();

            let pagination = PaginationMeta::new(page, limit, total_count);

            Ok(
                Json(RoomSessionResponse {
                    room_id: room_id.clone(),
                    sessions,
                    total_sessions: total_count as i32,
                    pagination,
                })
            )
        }
        Err(e) => {
            eprintln!("get_room_sessions error: {:?}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to fetch room sessions".to_string(),
                    code: "SESSIONS_FETCH_FAILED".to_string(),
                }),
            ))
        }
    }
}

pub async fn get_participant_detail(
    State(state): State<AppState>,
    Path((room_id, user_id)): Path<(String, String)>,
    Query(q): Query<PaginationQuery> // ← Add pagination params
) -> Result<Json<DetailedParticipantResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Validate pagination
    let page = q.page.max(1);
    let limit = q.limit.clamp(1, 100);
    let offset = (page - 1) * limit;

    // Get room host_id
    let room_result = sqlx
        ::query_scalar::<_, Option<String>>("SELECT created_by FROM rooms WHERE id = $1")
        .bind(&room_id)
        .fetch_optional(&state.db).await;

    let host_id = match room_result {
        Ok(Some(Some(hid))) => Some(hid),
        Ok(_) => None,
        Err(e) => {
            eprintln!("get_participant_detail room error: {:?}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to fetch room info".to_string(),
                    code: "ROOM_FETCH_FAILED".to_string(),
                }),
            ));
        }
    };

    // Get participant info
    let participant_result = sqlx
        ::query_as::<_, (String, String, String, DateTime<Utc>, DateTime<Utc>, i64)>(
            r#"
            SELECT 
                id,
                room_id,
                name,
                first_joined_at,
                last_seen,
                EXTRACT(EPOCH FROM (last_seen - first_joined_at))::bigint as duration_seconds
            FROM participants 
            WHERE id = $1 AND room_id = $2
            "#
        )
        .bind(&user_id)
        .bind(&room_id)
        .fetch_optional(&state.db).await;

    match participant_result {
        Ok(Some((user_id, room_id, name, joined_at, last_seen, duration_seconds))) => {
            let is_host = host_id.as_deref() == Some(&user_id);

            // Get total session count
            let count_result: Result<(i64,), _> = sqlx
                ::query_as(
                    "SELECT COUNT(*) FROM participant_sessions WHERE user_id = $1 AND room_id = $2"
                )
                .bind(&user_id)
                .bind(&room_id)
                .fetch_one(&state.db).await;

            let total_sessions = match count_result {
                Ok((count,)) => count,
                Err(e) => {
                    eprintln!("get_participant_detail sessions count error: {:?}", e);
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: "Failed to fetch session count".to_string(),
                            code: "SESSIONS_COUNT_FAILED".to_string(),
                        }),
                    ));
                }
            };

            // Get paginated sessions
            let sessions_result = sqlx
                ::query_as::<_, (String, DateTime<Utc>, Option<DateTime<Utc>>)>(
                    r#"
                    SELECT 
                        id,
                        joined_at,
                        last_seen
                    FROM participant_sessions 
                    WHERE user_id = $1 AND room_id = $2
                    ORDER BY joined_at DESC
                    LIMIT $3 OFFSET $4
                    "#
                )
                .bind(&user_id)
                .bind(&room_id)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(&state.db).await;

            let sessions = match sessions_result {
                Ok(session_rows) => {
                    session_rows
                        .into_iter()
                        .map(|(session_id, joined_at, last_seen)| {
                            let duration = last_seen.map(
                                |ls| (ls.timestamp() - joined_at.timestamp()) as i64
                            );
                            ParticipantSessionInfo {
                                session_id,
                                joined_at,
                                left_at: last_seen,
                                duration_seconds: duration,
                            }
                        })
                        .collect()
                }
                Err(e) => {
                    eprintln!("get_participant_detail sessions error: {:?}", e);
                    vec![]
                }
            };

            let pagination = PaginationMeta::new(page, limit, total_sessions);

            Ok(
                Json(DetailedParticipantResponse {
                    data: DetailedParticipantInfo {
                        user_id,
                        room_id,
                        name,
                        is_host,
                        joined_at,
                        last_seen,
                        duration_seconds,
                        sessions,
                    },
                    pagination,
                })
            )
        }
        Ok(None) => {
            Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Participant not found".to_string(),
                    code: "PARTICIPANT_NOT_FOUND".to_string(),
                }),
            ))
        }
        Err(e) => {
            eprintln!("get_participant_detail error: {:?}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to fetch participant details".to_string(),
                    code: "PARTICIPANT_FETCH_FAILED".to_string(),
                }),
            ))
        }
    }
}

pub async fn get_live_participants(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(params): Query<HashMap<String, String>>
) -> Json<serde_json::Value> {
    let user_id = params.get("user_id");
    let rooms = state.rooms.read().await;

    let Some(room) = rooms.get(&room_id) else {
        return Json(
            serde_json::json!({
            "room_id": room_id,
            "active": false,
            "count": 0,
            "canJoin": false,
            "participants": []
        })
        );
    };

    let is_host = user_id.map(|uid| room.host_id.as_deref() == Some(uid)).unwrap_or(false);

    let is_approved = user_id.map(|uid| room.approved_users.contains(uid)).unwrap_or(false);

    let can_join = room.is_open.unwrap_or(false) || is_host || is_approved;

    let participants: Vec<_> = room.participants
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
        .collect();

    Json(
        serde_json::json!({
        "room_id": room_id,
        "active": !room.sessions.is_empty(),
        "count": participants.len(),
        "isHost": is_host,
        "approved": is_approved,
        "canJoin": can_join,
        "participants": participants
    })
    )
}
