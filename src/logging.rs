use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init(level: &str) {
    let filter = match level.to_lowercase().as_str() {
        "trace" => "trace".to_string(),
        "debug" => "debug".to_string(),
        "info" | "" => "info".to_string(),
        "warn" | "warning" => "warn".to_string(),
        "error" => "error".to_string(),
        other => other.to_string(),
    };
    let default = format!("scout_core={},tower_http=info,info", filter);
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_target(true).compact())
        .init();
}
