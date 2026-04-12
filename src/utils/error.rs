pub fn log_error<T, E: std::fmt::Debug>(result: Result<T, E>, context: &str) -> Option<T> {
    match result {
        Ok(val) => Some(val),
        Err(err) => {
            eprintln!("❌ ERROR [{}]: {:?}", context, err);
            None
        }
    }
}
