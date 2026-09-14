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
