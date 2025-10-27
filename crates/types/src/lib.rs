
use serde::{Serialize, Deserialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub type TxId = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub nonce: u64,
    #[serde(default)]
    pub payload: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tx {
    pub id: TxId,
    pub transfer: Transfer,
    pub submitted_unix_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Batch {
    pub id: u64,
    pub txs: Vec<Tx>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    Committed,
    Rejected(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub tx_id: TxId,
    pub status: Status,
    pub block_height: u64,
    pub latency_ms: u128,
}

impl Tx {
    pub fn new(transfer: Transfer) -> Self {
        let id = make_tx_id(&transfer);
        let submitted_unix_ms = now_ms();
        Self { id, transfer, submitted_unix_ms }
    }
}

pub fn make_tx_id(t: &Transfer) -> TxId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(t.from.as_bytes());
    hasher.update(t.to.as_bytes());
    hasher.update(&t.amount.to_le_bytes());
    hasher.update(&t.nonce.to_le_bytes());
    if let Some(p) = &t.payload { hasher.update(p); }
    *hasher.finalize().as_bytes()
}

pub fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

#[async_trait::async_trait]
pub trait SubmitApi: Send + Sync {
    async fn submit_transfer(&self, t: Transfer) -> anyhow::Result<Receipt>;
    async fn get_balance(&self, addr: String) -> anyhow::Result<u64>;
}
