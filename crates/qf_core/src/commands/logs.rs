use serde_json::Value;
use utils::Error;

pub async fn log(
    cause: String,
    component: String,
    location: String,
    log_level: String,
    message: String,
    context: Option<Value>,
) -> Result<(), Error> {
    let error = Error::new(component, message, location)
        .with_context(context.unwrap_or_default())
        .with_cause(cause)
        .set_log_level(utils::LogLevel::from_str(&log_level));

    error.log("log.log");
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogLine {
    pub level: String,
    pub line: String,
}

/// The newest `limit` cached log lines, oldest first (spec §20 L3). `limit` is clamped to 1..=2000.
pub async fn log_tail(limit: i64) -> Result<Vec<LogLine>, Error> {
    let limit = limit.clamp(1, 2000) as usize;
    Ok(utils::tail(limit)
        .into_iter()
        .map(|(level, line)| LogLine { level: level.prefix().to_string(), line })
        .collect())
}
