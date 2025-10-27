
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use types::{SubmitApi, Transfer, Receipt, Tx, TxId};
use mempool::MempoolHandle;
use exec::Executor;

#[derive(Clone)]
pub struct Node {
    mempool: MempoolHandle,
    waiters: Arc<Mutex<HashMap<TxId, oneshot::Sender<Receipt>>>>,
    tx_timeout_ms: u64,
    executor: Arc<dyn Executor>,
    p2p_publish: Option<mpsc::Sender<Vec<u8>>>,
}

impl Node {
    pub fn new(mempool: MempoolHandle, executor: Arc<dyn Executor>, p2p_publish: Option<mpsc::Sender<Vec<u8>>>) -> Arc<Self> {
        Arc::new(Self {
            mempool,
            waiters: Arc::new(Mutex::new(HashMap::new())),
            tx_timeout_ms: 5_000,
            executor,
            p2p_publish,
        })
    }

    pub fn spawn_commit_listener(self: &Arc<Self>, mut committed_rx: mpsc::Receiver<Receipt>) {
        let me = self.clone();
        tokio::spawn(async move {
            while let Some(r) = committed_rx.recv().await {
                if let Some(tx) = me.waiters.lock().remove(&r.tx_id) {
                    let _ = tx.send(r);
                }
            }
        });
    }

    fn register_waiter(&self, id: TxId) -> oneshot::Receiver<Receipt> {
        let (tx, rx) = oneshot::channel();
        self.waiters.lock().insert(id, tx);
        rx
    }

    pub async fn enqueue_tx(&self, tx: Tx) -> anyhow::Result<()> {
        self.mempool.enqueue(tx).await
    }

    pub fn executor(&self) -> Arc<dyn Executor> { self.executor.clone() }
}

#[async_trait::async_trait]
impl SubmitApi for Node {
    async fn submit_transfer(&self, t: Transfer) -> anyhow::Result<Receipt> {
        let tx = Tx::new(t.clone());
        if let Some(p2p) = &self.p2p_publish { let _ = p2p.send(serde_json::to_vec(&t)?).await; }
        let id = tx.id;
        let rx = self.register_waiter(id);
        self.enqueue_tx(tx).await?;
        match tokio::time::timeout(std::time::Duration::from_millis(self.tx_timeout_ms), rx).await {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(_canceled)) => Err(anyhow::anyhow!("commit channel canceled")),
            Err(_elapsed) => Err(anyhow::anyhow!("timeout waiting for commit")),
        }
    }

    async fn get_balance(&self, addr: String) -> anyhow::Result<u64> {
        Ok(self.executor().balance(&addr))
    }
}
