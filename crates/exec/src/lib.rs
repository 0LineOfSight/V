
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use types::{Batch, Receipt, Status, now_ms};
use once_cell::sync::Lazy;
use prometheus::{Histogram, register_histogram};

static EXEC_LATENCY: Lazy<Histogram> = Lazy::new(|| register_histogram!("exec_apply_batch_seconds", "Batch apply duration").unwrap());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState { pub addr: String, pub ver: u64, pub bal: u64, pub last_update_height: u64 }

pub trait Executor: Send + Sync {
    fn balance(&self, addr: &str) -> u64;
    fn apply_batch_blocking(&self, batch: Batch, block_height: u64) -> Vec<Receipt>;
    fn last_height(&self) -> u64;
    fn snapshot(&self) -> Vec<AccountState>;
    fn diff_since(&self, since: u64) -> Vec<AccountState>;
    fn restore(&self, replace: bool, items: Vec<AccountState>);
}

#[derive(Clone)]
pub struct SimpleExecutor {
    pub accounts: Arc<RwLock<HashMap<String, (u64, u64, u64)>>>,
    pub last_height: Arc<RwLock<u64>>,
}
impl SimpleExecutor {
    pub fn new() -> Self {
        let s = Self { accounts: Arc::new(RwLock::new(HashMap::new())), last_height: Arc::new(RwLock::new(0)) };
        s.credit("alice".into(), 1_000_000_000_000, 0);
        s
    }
    fn credit(&self, addr: String, amount: u64, h: u64) {
        let mut w = self.accounts.write();
        let e = w.entry(addr).or_insert((0,0,0));
        e.0 += 1; e.1 = e.1.saturating_add(amount); e.2 = h;
    }
    fn debit(&self, addr: String, amount: u64, h: u64) -> Result<(), String> {
        let mut w = self.accounts.write();
        let e = w.entry(addr.clone()).or_insert((0,0,0));
        if e.1 >= amount { e.0 += 1; e.1 -= amount; e.2 = h; Ok(()) } else { Err(format!("insufficient funds: {}", addr)) }
    }
}
impl Executor for SimpleExecutor {
    fn balance(&self, addr: &str) -> u64 { self.accounts.read().get(addr).map(|(_,b,_)| *b).unwrap_or(0) }
    fn apply_batch_blocking(&self, batch: Batch, block_height: u64) -> Vec<Receipt> {
        let _t = EXEC_LATENCY.start_timer();
        *self.last_height.write() = block_height;
        batch.txs.into_iter().map(|tx| {
            let res = self.debit(tx.transfer.from.clone(), tx.transfer.amount, block_height);
            let status = match res {
                Ok(()) => { self.credit(tx.transfer.to.clone(), tx.transfer.amount, block_height); Status::Committed }
                Err(e) => Status::Rejected(e),
            };
            let latency_ms = now_ms().saturating_sub(tx.submitted_unix_ms);
            Receipt { tx_id: tx.id, status, block_height, latency_ms }
        }).collect()
    }
    fn last_height(&self) -> u64 { *self.last_height.read() }
    fn snapshot(&self) -> Vec<AccountState> {
        self.accounts.read().iter().map(|(a,(v,b,h))| AccountState { addr:a.clone(), ver:*v, bal:*b, last_update_height:*h }).collect()
    }
    fn diff_since(&self, since: u64) -> Vec<AccountState> {
        self.accounts.read().iter().filter_map(|(a,(v,b,h))| if *h > since { Some(AccountState{addr:a.clone(), ver:*v, bal:*b, last_update_height:*h}) } else { None }).collect()
    }
    fn restore(&self, replace: bool, items: Vec<AccountState>) {
        if replace { self.accounts.write().clear(); }
        let mut w = self.accounts.write();
        for it in items { w.insert(it.addr, (it.ver, it.bal, it.last_update_height)); }
    }
}

pub struct BlockStmExecutor { inner: Arc<SimpleExecutor>, max_retries: usize }
impl BlockStmExecutor { pub fn new(inner: Arc<SimpleExecutor>) -> Self { Self { inner, max_retries: 5 } } }
impl Executor for BlockStmExecutor {
    fn balance(&self, addr: &str) -> u64 { self.inner.balance(addr) }
    fn apply_batch_blocking(&self, batch: Batch, block_height: u64) -> Vec<Receipt> {
        let _t = EXEC_LATENCY.start_timer();
        let mut txs = batch.txs;
        *self.inner.last_height.write() = block_height;
        let mut receipts: Vec<Option<Receipt>> = vec![None; txs.len()];
        for _round in 0..self.max_retries {
            let snapshot = self.inner.accounts.read().clone();
            for (i, tx) in txs.iter_mut().enumerate() {
                if receipts[i].is_some() { continue; }
                let from_v = snapshot.get(&tx.transfer.from).map(|(v,b,_)| (*v,*b)).unwrap_or((0,0));
                let to_v = snapshot.get(&tx.transfer.to).map(|(v,b,_)| (*v,*b)).unwrap_or((0,0));
                if from_v.1 < tx.transfer.amount {
                    receipts[i] = Some(Receipt { tx_id: tx.id, status: Status::Rejected("insufficient funds".into()), block_height, latency_ms: now_ms().saturating_sub(tx.submitted_unix_ms) });
                    continue;
                }
                let mut w = self.inner.accounts.write();
                let (ver_f, bal_f, _) = *w.get(&tx.transfer.from).unwrap_or(&(from_v.0, from_v.1, block_height));
                let (ver_t, bal_t, _) = *w.get(&tx.transfer.to).unwrap_or(&(to_v.0, to_v.1, block_height));
                if ver_f == from_v.0 && ver_t == to_v.0 && bal_f >= tx.transfer.amount {
                    w.insert(tx.transfer.from.clone(), (ver_f+1, bal_f - tx.transfer.amount, block_height));
                    w.insert(tx.transfer.to.clone(), (ver_t+1, bal_t + tx.transfer.amount, block_height));
                    receipts[i] = Some(Receipt { tx_id: tx.id, status: Status::Committed, block_height, latency_ms: now_ms().saturating_sub(tx.submitted_unix_ms) });
                }
            }
            if receipts.iter().all(|r| r.is_some()) { break; }
        }
        for i in 0..receipts.len() { if receipts[i].is_none() {
            let tx = &txs[i];
            receipts[i] = Some(Receipt { tx_id: tx.id, status: Status::Rejected("conflict".into()), block_height, latency_ms: now_ms().saturating_sub(tx.submitted_unix_ms) }); } }
        receipts.into_iter().map(|r| r.unwrap()).collect()
    }
    fn last_height(&self) -> u64 { *self.inner.last_height.read() }
    fn snapshot(&self) -> Vec<AccountState> { self.inner.snapshot() }
    fn diff_since(&self, since: u64) -> Vec<AccountState> { self.inner.diff_since(since) }
    fn restore(&self, replace: bool, items: Vec<AccountState>) { self.inner.restore(replace, items) }
}
