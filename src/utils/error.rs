use axum::extract::ws::Message;
use serde_json::json;

pub fn log_error<T, E: std::fmt::Debug>(result: Result<T, E>, context: &str) -> Option<T> {
    match result {
        Ok(val) => Some(val),
        Err(err) => {
            eprintln!("❌ ERROR [{}]: {:?}", context, err);
            None
        }
    }
}

pub fn error_msg(message: &str) -> Message {
    Message::Text(
        json!({
            "type": "ERROR",
            "message": message
        })
            .to_string()
            .into()
    )
}
