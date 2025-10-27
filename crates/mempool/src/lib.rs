
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use types::{Tx, Batch};
use once_cell::sync::Lazy;
use prometheus::{IntGauge, Histogram, register_int_gauge, register_histogram};

static MEMPOOL_SIZE: Lazy<IntGauge> = Lazy::new(|| register_int_gauge!("mempool_queue_len", "current tx queue length").unwrap());
static MEMPOOL_FLUSH_LAT: Lazy<Histogram> = Lazy::new(|| register_histogram!("mempool_flush_seconds", "time between flushes").unwrap());

#[derive(Clone)]
pub struct MempoolHandle { tx: mpsc::Sender<Tx> }

impl MempoolHandle {
    pub async fn enqueue(&self, txi: Tx) -> anyhow::Result<()> {
        self.tx.send(txi).await.map_err(|e| anyhow::anyhow!(e.to_string()))
    }
}

pub fn spawn_mempool(_flush_ms: u64, _max_batch_len: usize, _to_consensus: mpsc::Sender<Batch>) -> (MempoolHandle, mpsc::Receiver<Tx>) {
    let (tx, rx) = mpsc::channel::<Tx>(64_000);
    (MempoolHandle { tx }, rx)
}

pub async fn run_mempool(
    mut from_clients: mpsc::Receiver<Tx>,
    to_consensus: mpsc::Sender<Batch>,
    flush_ms: u64,
    max_batch_len: usize,
    mut from_p2p: Option<mpsc::Receiver<Tx>>,
) {
    let mut ticker = time::interval(Duration::from_millis(flush_ms));
    let mut cur: Vec<Tx> = Vec::with_capacity(max_batch_len);
    let mut batch_id: u64 = 1;
    let mut last_flush = std::time::Instant::now();

    loop {
        MEMPOOL_SIZE.set(cur.len() as i64);
        tokio::select! {
            maybe_tx = from_clients.recv() => {
                match maybe_tx {
                    Some(tx) => {
                        cur.push(tx);
                        if cur.len() >= max_batch_len {
                            let out = Batch { id: batch_id, txs: std::mem::take(&mut cur) };
                            batch_id += 1;
                            if let Err(e) = to_consensus.send(out).await { eprintln!("mempool -> consensus send error: {e}"); break; }
                            MEMPOOL_FLUSH_LAT.observe(last_flush.elapsed().as_secs_f64()); last_flush = std::time::Instant::now();
                        }
                    },
                    None => break,
                }
            }
            from_gossip = async { if let Some(rx) = &mut from_p2p { rx.recv().await } else { None } } => {
                if let Some(tx) = from_gossip { cur.push(tx); }
            }
            _ = ticker.tick() => {
                if !cur.is_empty() {
                    let out = Batch { id: batch_id, txs: std::mem::take(&mut cur) };
                    batch_id += 1;
                    if let Err(e) = to_consensus.send(out).await { eprintln!("mempool -> consensus send error: {e}"); break; }
                    MEMPOOL_FLUSH_LAT.observe(last_flush.elapsed().as_secs_f64()); last_flush = std::time::Instant::now();
                }
            }
        }
    }
}
