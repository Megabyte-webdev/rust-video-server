use chrono::NaiveDateTime;
use serde::{ Serialize, Deserialize };

#[derive(sqlx::FromRow, Serialize, Deserialize, Debug, Clone)]
pub struct Room {
    pub id: String,
    pub title: Option<String>,
    pub created_by: Option<String>,
}

#[derive(sqlx::FromRow, Serialize, Deserialize, Debug, Clone)]
pub struct Participant {
    pub id: String,
    pub room_id: String,
    pub joined_at: Option<NaiveDateTime>,
    pub left_at: Option<NaiveDateTime>,
}
