
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};
use serde::{Serialize, Deserialize};
use types::Batch;
use da::{DaProof, encode as da_encode, MerkleProof, proof_verify};

pub mod store;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct View(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signed { pub voter: u32, pub sig: crypto::Sig }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumCert {
    pub view: u64,
    pub root: [u8;32],
    pub voters: Vec<u32>,
    pub sigs: Vec<Signed>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutCert { pub view: u64, pub sigs: Vec<Signed> }

use net_quic::{QuicEvent, NetOut};
use once_cell::sync::Lazy;
use prometheus::{IntCounter, Histogram, register_int_counter, register_histogram};
use bincode;

static PROPOSALS_SENT: Lazy<IntCounter> = Lazy::new(|| register_int_counter!("consensus_proposals_sent_total", "Proposals sent").unwrap());
static VOTES_SENT: Lazy<IntCounter> = Lazy::new(|| register_int_counter!("consensus_votes_sent_total", "Votes sent").unwrap());
static QCS_FORMED: Lazy<IntCounter> = Lazy::new(|| register_int_counter!("consensus_qcs_formed_total", "QCs formed").unwrap());
static COMMITS: Lazy<IntCounter> = Lazy::new(|| register_int_counter!("consensus_commits_total", "Blocks committed").unwrap());
static NEWVIEWS_SENT: Lazy<IntCounter> = Lazy::new(|| register_int_counter!("consensus_newviews_sent_total", "NewViews sent").unwrap());
static TIMEOUTS_SENT: Lazy<IntCounter> = Lazy::new(|| register_int_counter!("consensus_timeouts_sent_total", "Timeouts sent").unwrap());
static PROPOSAL_TO_COMMIT: Lazy<Histogram> = Lazy::new(|| register_histogram!("consensus_proposal_to_commit_seconds", "proposal->commit duration").unwrap());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusMsg {
    RbcShard { sender: u32, root: [u8;32], shard_index: u32, bytes: Vec<u8>, proof: MerkleProof },
    RbcEcho { sender: u32, root: [u8;32], sig: crypto::Sig },
    RbcReady { sender: u32, root: [u8;32], sig: crypto::Sig },
    Proposal { view: u64, proposer: u32, root: [u8;32], da_proof: DaProof, high_qc: Option<QuorumCert>, sig: crypto::Sig },
    Vote { view: u64, voter: u32, root: [u8;32], sig: crypto::Sig },
    NewView { view: u64, voter: u32, high_qc: Option<QuorumCert>, tc: Option<TimeoutCert>, sig: crypto::Sig },
    Timeout { view: u64, voter: u32, sig: crypto::Sig },
}

#[derive(Debug, Clone)]
pub struct Validator {
    pub id: u32,
    pub addr: std::net::SocketAddr,
    pub pubkey: crypto::PubKey,
}

#[derive(Clone)]
pub struct Validators { pub self_id: u32, pub nodes: Vec<Validator> }
impl Validators {
    pub fn len(&self) -> usize { self.nodes.len() }
    pub fn f(&self) -> usize { (self.len().saturating_sub(1)) / 3 }
    pub fn quorum(&self) -> usize { (2 * self.f()) + 1 }
    pub fn leader_for(&self, view: u64) -> &Validator { let idx = ((view - 1) as usize) % self.nodes.len(); &self.nodes[idx] }
    pub fn peers(&self) -> impl Iterator<Item=&Validator> { self.nodes.iter().filter(move |v| v.id != self.self_id) }
    pub fn get_pub(&self, id: u32) -> Option<&crypto::PubKey> { self.nodes.iter().find(|v| v.id==id).map(|v| &v.pubkey) }
}

#[derive(Default)]
struct RbcState {
    shards: std::collections::HashMap<[u8;32], Vec<(u32, Vec<u8>)>>,
    echo: std::collections::HashMap<[u8;32], std::collections::HashSet<u32>>,
    ready: std::collections::HashMap<[u8;32], std::collections::HashSet<u32>>,
    payloads: std::collections::HashMap<[u8;32], Vec<u8>>,
}

impl RbcState {
    fn push_shard(&mut self, root: [u8;32], idx: u32, bytes: Vec<u8>) { self.shards.entry(root).or_default().push((idx, bytes)); }
    fn try_reconstruct(&mut self, root: [u8;32], k: u32, m: u32) -> bool {
        if self.payloads.contains_key(&root) { return true; }
        if let Some(vec) = self.shards.get(&root) {
            if (vec.len() as u32) >= k {
                let max_idx = (k+m) as usize;
                let mut shards: Vec<Option<Vec<u8>>> = vec![None; max_idx];
                for (idx, bytes) in vec { if (*idx as usize) < max_idx { shards[*idx as usize] = Some(bytes.clone()); } }
                if let Ok(payload) = reed_solomon_erasure::galois_8::ReedSolomon::new(k as usize, m as usize).and_then(|rs| {
                    let mut shards_clone = shards.clone(); rs.reconstruct(&mut shards_clone)?;
                    let mut out = Vec::new(); for i in 0..(k as usize) { out.extend_from_slice(shards_clone[i].as_ref().unwrap()); }
                    while out.last().copied() == Some(0) { out.pop(); } Ok::<Vec<u8>, reed_solomon_erasure::Error>(out)
                }) { self.payloads.insert(root, payload); return true; }
            }
        } false
    }
    fn has_payload(&self, root: &[u8;32]) -> bool { self.payloads.contains_key(root) }
    fn get_payload(&self, root: &[u8;32]) -> Option<&Vec<u8>> { self.payloads.get(root) }
}

pub struct KeySet {
    pub my_sk: crypto::SecretKey,
    pub my_pk: crypto::PubKey,
    pub pks: std::collections::HashMap<u32, crypto::PubKey>,
}
impl KeySet {
    fn sign(&self, bytes: &[u8]) -> crypto::Sig { crypto::sign(&self.my_sk, bytes) }
    fn verify(&self, voter: u32, bytes: &[u8], sig: &crypto::Sig) -> bool {
        self.pks.get(&voter).map(|pk| crypto::verify(pk, bytes, sig)).unwrap_or(false)
    }
}
fn sign_bytes(tag: &str, data: &[u8]) -> Vec<u8> { let mut v = Vec::new(); v.extend_from_slice(tag.as_bytes()); v.extend_from_slice(blake3::hash(data).as_bytes()); v }

pub async fn run_hotstuff_quic(
    mut from_mempool: mpsc::Receiver<Batch>,
    mut to_exec: mpsc::Sender<(Batch, u64)>,
    pacemaker_ms: u64,
    net_out: mpsc::Sender<NetOut>,
    mut net_in: mpsc::Receiver<QuicEvent>,
    validators: Validators,
    keys: KeySet,
    qc_store: Option<std::sync::Arc<dyn store::QcTcStore>>,
) {
    let mut height: u64 = 1;
    let mut view: u64 = 1;
    let timeout = Duration::from_millis(pacemaker_ms);
    let mut pending_root: Option<[u8;32]> = None;
    let mut votes_ed: std::collections::HashMap<u32, crypto::Sig> = std::collections::HashMap::new();
    let mut rbc = RbcState::default();
    let f = validators.f();
    let quorum = validators.quorum();
    let mut prop_start: std::collections::HashMap<[u8;32], std::time::Instant> = std::collections::HashMap::new();
    let k: u32 = 2; let m: u32 = 1;

    let mut high_qc: Option<QuorumCert> = qc_store.as_ref().and_then(|s| s.load_high_qc());

    loop {
        tokio::select! {
            maybe = from_mempool.recv() => {
                if let Some(batch) = maybe {
                    let payload = bincode::serialize(&batch).expect("serialize batch");
                    let shards = da_encode(&payload, k, m).expect("encode");
                    let root = shards[0].proof.root;
                    for s in &shards {
                        let msg = ConsensusMsg::RbcShard { sender: validators.self_id, root, shard_index: s.index, bytes: s.bytes.clone(), proof: s.proof.clone() };
                        broadcast(&net_out, &validators, &msg).await;
                    }
                    let echo_bytes = sign_bytes("RBC_ECHO", &root);
                    let echo_sig = keys.sign(&echo_bytes);
                    let echo = ConsensusMsg::RbcEcho { sender: validators.self_id, root, sig: echo_sig };
                    broadcast(&net_out, &validators, &echo).await;
                    if validators.leader_for(view).id == validators.self_id { pending_root = Some(root); }
                } else { break; }
            }
            _ = sleep(timeout) => {
                let to_bytes = sign_bytes("TIMEOUT", &view.to_le_bytes());
                let to = ConsensusMsg::Timeout { view, voter: validators.self_id, sig: keys.sign(&to_bytes) };
                broadcast(&net_out, &validators, &to).await; TIMEOUTS_SENT.inc();
                view += 1;
                let nv_bytes = sign_bytes("NEWVIEW", &view.to_le_bytes());
                let nv = ConsensusMsg::NewView { view, voter: validators.self_id, high_qc: high_qc.clone(), tc: None, sig: keys.sign(&nv_bytes) };
                broadcast(&net_out, &validators, &nv).await; NEWVIEWS_SENT.inc();
            }
            maybe_ev = net_in.recv() => {
                match maybe_ev {
                    Some(QuicEvent::Received { data, .. }) => {
                        if let Ok(msg) = bincode::deserialize::<ConsensusMsg>(&data) {
                            match msg {
                                ConsensusMsg::RbcShard { root, bytes, proof, .. } => {
                                    if proof_verify(&proof, da::digest(&bytes)) {
                                        rbc.push_shard(root, proof.index, bytes);
                                        let _ = rbc.try_reconstruct(root, k, m);
                                    }
                                }
                                ConsensusMsg::RbcEcho { sender, root, sig } => {
                                    let b = sign_bytes("RBC_ECHO", &root);
                                    if keys.verify(sender, &b, &sig) {
                                        let e = rbc.echo.entry(root).or_default(); e.insert(sender);
                                        if e.len() >= validators.len() - f {
                                            let r_bytes = sign_bytes("RBC_READY", &root);
                                            let r_sig = keys.sign(&r_bytes);
                                            let ready = ConsensusMsg::RbcReady { sender: validators.self_id, root, sig: r_sig };
                                            broadcast(&net_out, &validators, &ready).await;
                                        }
                                    }
                                }
                                ConsensusMsg::RbcReady { sender, root, sig } => {
                                    let b = sign_bytes("RBC_READY", &root);
                                    if keys.verify(sender, &b, &sig) {
                                        let __has_payload = rbc.has_payload(&root);
                    let e = rbc.ready.entry(root).or_default(); e.insert(sender);
                                        if e.len() >= (2*f + 1) && __has_payload {
                                            if let Some(pr) = pending_root {
                                                if pr == root && validators.leader_for(view).id == validators.self_id {
                                                    let da_proof = DaProof { ready_signers: e.iter().copied().collect(), merkle_root: root, k, m };
                                                    let prop_bytes = sign_bytes("PROPOSAL", &[view.to_le_bytes().as_slice(), &root].concat());
                                                    let prop = ConsensusMsg::Proposal { view, proposer: validators.self_id, root, da_proof, high_qc: high_qc.clone(), sig: keys.sign(&prop_bytes) };
                                                    broadcast(&net_out, &validators, &prop).await; PROPOSALS_SENT.inc();
                                                    prop_start.insert(root, std::time::Instant::now());
                                                }
                                            }
                                        }
                                    }
                                }
                                ConsensusMsg::Proposal { view: v, proposer, root, sig, .. } => {
                                    let prop_bytes = sign_bytes("PROPOSAL", &[v.to_le_bytes().as_slice(), &root].concat());
                                    if let Some(pk) = validators.get_pub(proposer) {
                                        if crypto::verify(pk, &prop_bytes, &sig) {
                                            view = v;
                                            if rbc.has_payload(&root) {
                                                let vote_bytes = sign_bytes("VOTE", &[view.to_le_bytes().as_slice(), &root].concat());
                                                let vote = ConsensusMsg::Vote { view, voter: validators.self_id, root, sig: keys.sign(&vote_bytes) };
                                                send_to(&net_out, validators.leader_for(view).addr, &vote).await; VOTES_SENT.inc();
                                            }
                                        }
                                    }
                                }
                                ConsensusMsg::Vote { view: v, voter, root, sig } => {
                                    let vote_bytes = sign_bytes("VOTE", &[v.to_le_bytes().as_slice(), &root].concat());
                                    if keys.verify(voter, &vote_bytes, &sig) {
                                        if validators.leader_for(v).id == validators.self_id && v == view {
                                            votes_ed.insert(voter, sig);
                                            if votes_ed.len() >= quorum {
                                                let qc = QuorumCert { view, root, voters: votes_ed.keys().copied().collect(), sigs: votes_ed.iter().map(|(id,s)| Signed{voter:*id, sig:s.clone()}).collect() };
                                                if let Some(store) = qc_store.as_ref() { store.save_high_qc(&qc); }
                                                QCS_FORMED.inc();
                                                if let Some(payload) = rbc.get_payload(&root) {
                                                    if let Ok(batch) = bincode::deserialize::<Batch>(payload) {
                                                        if let Err(e) = to_exec.send((batch, height)).await { warn!("consensus -> exec send error: {e}"); }
                                                        else { COMMITS.inc(); height += 1;
                                                            if let Some(start) = prop_start.remove(&root) { PROPOSAL_TO_COMMIT.observe(start.elapsed().as_secs_f64()); }
                                                        }
                                                    }
                                                }
                                                votes_ed.clear(); pending_root=None; view += 1;
                                            }
                                        }
                                    }
                                }
                                ConsensusMsg::NewView { view: v, .. } => { if v > view { view = v; } }
                                ConsensusMsg::Timeout { .. } => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    info!("consensus loop ended");
}

async fn broadcast(net_out: &mpsc::Sender<NetOut>, validators: &Validators, msg: &ConsensusMsg) {
    let bytes = bincode::serialize(msg).expect("serialize");
    for v in validators.peers() { let _ = net_out.send(NetOut { addr: v.addr, data: bytes.clone() }).await; }
}
async fn send_to(net_out: &mpsc::Sender<NetOut>, addr: std::net::SocketAddr, msg: &ConsensusMsg) {
    let bytes = bincode::serialize(msg).expect("serialize");
    let _ = net_out.send(NetOut { addr, data: bytes }).await;
}
