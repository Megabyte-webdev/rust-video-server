use axum::{ extract::{ Path, State, Query }, http::StatusCode, Json };
use serde::{ Deserialize, Serialize };
use chrono::{ DateTime, Utc };

use crate::state::AppState;

// ATTENDANCE RECORDS

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AttendanceRecord {
    pub user_id: String,
    pub name: String,
    pub joined_at: DateTime<Utc>,
    pub left_at: Option<DateTime<Utc>>,
    pub duration_seconds: Option<i64>,
    pub session_count: i32,
    pub is_active: bool,
}

#[derive(Debug, Serialize)]
pub struct AttendanceListResponse {
    pub room_id: String,
    pub room_name: String,
    pub total_participants: i32,
    pub active_participants: i32,
    pub records: Vec<AttendanceRecord>,
    pub fetched_at: DateTime<Utc>,
}

// PARTICIPANT STATS

#[derive(Debug, Serialize, Clone)]
pub struct ParticipantStats {
    pub user_id: String,
    pub name: String,
    pub session_id: String,
    pub joined_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub time_in_room_seconds: i64,
    pub camera_enabled: bool,
    pub mic_enabled: bool,
    pub screen_share_count: i32,
    pub screen_share_duration_seconds: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ParticipantStatsResponse {
    pub room_id: String,
    pub participants: Vec<ParticipantStats>,
    pub total_count: i32,
    pub active_count: i32,
}

// ROOM SESSION DATA

#[derive(Debug, Serialize, Clone)]
pub struct RoomSession {
    pub session_id: String,
    pub room_id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_seconds: Option<i64>,
    pub participant_count: i32,
    pub peak_concurrent: i32,
}

#[derive(Debug, Serialize)]
pub struct RoomSessionResponse {
    pub room_id: String,
    pub sessions: Vec<RoomSession>,
    pub total_sessions: i32,
}

// DETAILED PARTICIPANT INFO

#[derive(Debug, Serialize)]
pub struct DetailedParticipantInfo {
    pub user_id: String,
    pub name: String,
    pub is_host: bool,
    pub is_presenter: bool,
    pub camera_stream_id: Option<String>,
    pub screen_share_stream_id: Option<String>,
    pub joined_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub duration_seconds: i64,
    pub sessions: Vec<ParticipantSessionInfo>,
}

#[derive(Debug, Serialize)]
pub struct ParticipantSessionInfo {
    pub session_id: String,
    pub joined_at: DateTime<Utc>,
    pub left_at: Option<DateTime<Utc>>,
    pub duration_seconds: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct DetailedParticipantResponse {
    pub data: DetailedParticipantInfo,
}

// QUERY FILTERS

#[derive(Debug, Deserialize)]
pub struct AttendanceQuery {
    #[serde(default)]
    pub include_inactive: bool,
    #[serde(default)]
    pub sort_by: Option<String>, // "duration", "name", "joined_at"
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: String,
}

// ENDPOINTS

/// Get all attendance records for a room
pub async fn get_attendance(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Query(q): Query<AttendanceQuery>
) -> Result<Json<AttendanceListResponse>, (StatusCode, Json<ErrorResponse>)> {
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
    "#;

    match
        sqlx
            ::query_as::<_, (String, String, DateTime<Utc>, DateTime<Utc>, i64, i32, bool)>(
                query_str
            )
            .bind(&room_id)
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

            Ok(
                Json(AttendanceListResponse {
                    room_id: room_id.clone(),
                    room_name,
                    total_participants: records.len() as i32,
                    active_participants: active_count,
                    records,
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

/// Get current participant list with stats
pub async fn get_participants(
    State(state): State<AppState>,
    Path(room_id): Path<String>
) -> Result<Json<ParticipantStatsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let query_str =
        r#"
        SELECT 
            ps.user_id,
            ps.name,
            ps.id as session_id,
            ps.joined_at,
            ps.last_seen,
            EXTRACT(EPOCH FROM (ps.last_seen - ps.joined_at))::bigint as time_in_room_seconds,
            true as camera_enabled,
            true as mic_enabled,
            0::int as screen_share_count,
            NULL::bigint as screen_share_duration_seconds
        FROM participant_sessions ps
        WHERE ps.room_id = $1
        AND ps.last_seen > NOW() - INTERVAL '30 minutes'
        ORDER BY ps.joined_at ASC
    "#;

    match
        sqlx
            ::query_as::<
                _,
                (
                    String,
                    String,
                    String,
                    DateTime<Utc>,
                    DateTime<Utc>,
                    i64,
                    bool,
                    bool,
                    i32,
                    Option<i64>,
                )
            >(query_str)
            .bind(&room_id)
            .fetch_all(&state.db).await
    {
        Ok(rows) => {
            let active_count = rows.len() as i32;

            let participants: Vec<ParticipantStats> = rows
                .into_iter()
                .map(
                    |(
                        user_id,
                        name,
                        session_id,
                        joined_at,
                        last_seen,
                        duration,
                        cam_enabled,
                        mic_enabled,
                        ss_count,
                        ss_duration,
                    )| {
                        ParticipantStats {
                            user_id,
                            name,
                            session_id,
                            joined_at,
                            last_seen,
                            time_in_room_seconds: duration,
                            camera_enabled: cam_enabled,
                            mic_enabled: mic_enabled,
                            screen_share_count: ss_count,
                            screen_share_duration_seconds: ss_duration,
                        }
                    }
                )
                .collect();

            Ok(
                Json(ParticipantStatsResponse {
                    room_id: room_id.clone(),
                    participants,
                    total_count: active_count,
                    active_count,
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

/// Get room session history
pub async fn get_room_sessions(
    State(state): State<AppState>,
    Path(room_id): Path<String>
) -> Result<Json<RoomSessionResponse>, (StatusCode, Json<ErrorResponse>)> {
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
    "#;

    match
        sqlx
            ::query_as::<_, (String, String, DateTime<Utc>, Option<DateTime<Utc>>, i64, i32, i32)>(
                query_str
            )
            .bind(&room_id)
            .fetch_all(&state.db).await
    {
        Ok(rows) => {
            let sessions: Vec<RoomSession> = rows
                .clone()
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

            Ok(
                Json(RoomSessionResponse {
                    room_id: room_id.clone(),
                    sessions,
                    total_sessions: rows.len() as i32,
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

/// Get detailed info for a specific participant
pub async fn get_participant_detail(
    State(state): State<AppState>,
    Path((room_id, user_id)): Path<(String, String)>
) -> Result<Json<DetailedParticipantResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Get participant info
    let participant_result = sqlx
        ::query_as::<_, (String, String, bool)>(
            "SELECT id, name, first_joined_at FROM participants WHERE id = $1 AND room_id = $2"
        )
        .bind(&user_id)
        .bind(&room_id)
        .fetch_optional(&state.db).await;

    match participant_result {
        Ok(Some((user_id, name, _))) => {
            // Get sessions
            let sessions_result = sqlx
                ::query_as::<_, (String, DateTime<Utc>, Option<DateTime<Utc>>)>(
                    r#"
                SELECT id, joined_at, NULL::timestamp as left_at 
                FROM participant_sessions 
                WHERE user_id = $1 AND room_id = $2
                ORDER BY joined_at ASC
                "#
                )
                .bind(&user_id)
                .bind(&room_id)
                .fetch_all(&state.db).await;

            match sessions_result {
                Ok(session_rows) => {
                    let sessions: Vec<ParticipantSessionInfo> = session_rows
                        .into_iter()
                        .map(|(session_id, joined_at, left_at)| {
                            let duration = left_at.map(
                                |lt| (lt.timestamp() - joined_at.timestamp()) as i64
                            );
                            ParticipantSessionInfo {
                                session_id,
                                joined_at,
                                left_at,
                                duration_seconds: duration,
                            }
                        })
                        .collect();

                    let total_duration: i64 = sessions
                        .iter()
                        .filter_map(|s| s.duration_seconds)
                        .sum();

                    Ok(
                        Json(DetailedParticipantResponse {
                            data: DetailedParticipantInfo {
                                user_id,
                                name,
                                is_host: false,
                                is_presenter: false,
                                camera_stream_id: None,
                                screen_share_stream_id: None,
                                joined_at: sessions
                                    .first()
                                    .map(|s| s.joined_at)
                                    .unwrap_or_else(Utc::now),
                                last_seen: sessions
                                    .last()
                                    .map(|s| s.left_at.unwrap_or_else(Utc::now))
                                    .unwrap_or_else(Utc::now),
                                duration_seconds: total_duration,
                                sessions,
                            },
                        })
                    )
                }
                Err(e) => {
                    eprintln!("get_participant_detail sessions error: {:?}", e);
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: "Failed to fetch participant sessions".to_string(),
                            code: "PARTICIPANT_SESSIONS_FETCH_FAILED".to_string(),
                        }),
                    ))
                }
            }
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
