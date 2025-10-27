use axum::{Router, routing::get};
use anyhow::Result;

pub async fn run(addr: &str) -> Result<()> {
    let app = Router::new().route("/healthz", get(|| async { "ok" }));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Compatibility wrapper for older call sites that pass extra params we don't need.
pub async fn serve<T1, T2>(addr: &str, _api: T1, _executor: T2) -> Result<()> {
    run(addr).await
}
