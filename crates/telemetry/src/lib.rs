
use tracing_subscriber::{fmt, EnvFilter};
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt::Subscriber::builder().with_env_filter(filter).with_ansi(true).init();
}
