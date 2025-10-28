use anyhow::Result;
use axum::{
    routing::{get, post},
    extract::{Path, State},
    http::StatusCode,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::info;

use types::Receipt;

/// Minimal transfer request the RPC accepts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferReq {
    pub from: String,
    pub to: String,
    #[serde(default = "one")]
    pub amount: u64,
}

fn one() -> u64 { 1 }

/// API trait the node must implement.
#[async_trait::async_trait]
pub trait NodeApi: Send + Sync + 'static + Clone {
    async fn submit_transfer(&self, t: TransferReq) -> Result<Receipt>;
    async fn get_balance(&self, addr: String) -> Result<u64>;
}

#[derive(Clone)]
struct RpcState<A: NodeApi> {
    api: A,
}

pub async fn serve<A, E>(addr: &str, api: A, _executor: E) -> Result<()>
where
    A: NodeApi,
    E: Send + Sync + 'static,
{
    let state = RpcState { api };

    // Use closures so axum can infer Handler bounds on 0.8 cleanly.
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/transfer", post(|State(state): State<RpcState<A>>, Json(req): Json<TransferReq>| async move {
            match state.api.submit_transfer(req).await {
                Ok(r) => Ok::<_, (StatusCode, String)>(Json(r)),
                Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
            }
        }))
        .route("/balance/{addr}", get(|State(state): State<RpcState<A>>, Path(addr): Path<String>| async move {
            match state.api.get_balance(addr).await {
                Ok(b) => Ok::<_, (StatusCode, String)>(Json(b)),
                Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
            }
        }))
        .with_state(state);

    info!("rpc: listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
