use serde::{ Deserialize, Serialize };
use chrono::{ DateTime, Utc };

#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    #[serde(default = "default_page")]
    pub page: u32,

    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_page() -> u32 {
    1
}

fn default_limit() -> u32 {
    25
}

#[derive(Debug, Serialize)]
pub struct PaginationMeta {
    pub page: u32,
    pub limit: u32,
    pub total: i64,
    pub total_pages: u32,
    pub has_next: bool,
    pub has_previous: bool,
}

impl PaginationMeta {
    pub fn new(page: u32, limit: u32, total: i64) -> Self {
        let total_pages = (((total as u32) + limit - 1) / limit).max(1);
        let has_next = page < total_pages;
        let has_previous = page > 1;

        Self {
            page,
            limit,
            total,
            total_pages,
            has_next,
            has_previous,
        }
    }
}

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

#[derive(Debug, Deserialize)]
pub struct AttendanceQuery {
    #[serde(default)]
    pub _include_inactive: bool,

    #[serde(default)]
    pub _sort_by: Option<String>,

    #[serde(default = "default_page")]
    pub page: u32,

    #[serde(default = "default_limit")]
    pub limit: u32,
}

#[derive(Serialize)]
pub struct AttendanceListResponse {
    pub room_id: String,
    pub room_name: String,
    pub total_participants: i32,
    pub active_participants: i32,
    pub records: Vec<AttendanceRecord>,
    pub pagination: PaginationMeta,
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
    pub time_in_room_seconds: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ParticipantStatsResponse {
    pub room_id: String,
    pub participants: Vec<ParticipantStats>,
    pub total_count: i32,
    pub active_count: i32,
    pub pagination: PaginationMeta,
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
    pub pagination: PaginationMeta,
}

// DETAILED PARTICIPANT INFO

#[derive(Debug, Serialize)]
pub struct DetailedParticipantInfo {
    pub user_id: String,
    pub room_id: String,
    pub name: String,
    pub is_host: bool,
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
    pub pagination: PaginationMeta,
}

// ERROR RESPONSE

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: String,
}
