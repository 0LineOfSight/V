
use serde::{Serialize, Deserialize};
use reed_solomon_erasure::galois_8::ReedSolomon;

pub fn digest(data: &[u8]) -> [u8;32] { *blake3::hash(data).as_bytes() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shard {
    pub index: u32,
    pub k: u32,
    pub m: u32,
    pub bytes: Vec<u8>,
    pub proof: MerkleProof,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    pub root: [u8;32],
    pub index: u32,
    pub path: Vec<[u8;32]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaProof {
    pub ready_signers: Vec<u32>,
    pub merkle_root: [u8;32],
    pub k: u32,
    pub m: u32,
}

fn merkle_hash(left: &[u8;32], right: &[u8;32]) -> [u8;32] {
    let mut h = blake3::Hasher::new(); h.update(left); h.update(right); *h.finalize().as_bytes()
}
fn merkle_root(leaves: &[[u8;32]]) -> [u8;32] {
    if leaves.is_empty() { return [0u8;32]; }
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::new();
        for i in (0..level.len()).step_by(2) {
            let l = level[i];
            let r = if i+1 < level.len() { level[i+1] } else { level[i] };
            next.push(merkle_hash(&l, &r));
        }
        level = next;
    }
    level[0]
}
fn merkle_proof(leaves: &[[u8;32]], idx: usize) -> Vec<[u8;32]> {
    let mut path = Vec::new();
    let mut level = leaves.to_vec();
    let mut i = idx;
    while level.len() > 1 {
        let sib = if i % 2 == 0 { if i+1 < level.len() { level[i+1] } else { level[i] } } else { level[i-1] };
        path.push(sib);
        i = i / 2;
        let mut next = Vec::new();
        for j in (0..level.len()).step_by(2) {
            let l = level[j];
            let r = if j+1 < level.len() { level[j+1] } else { level[j] };
            next.push(merkle_hash(&l, &r));
        }
        level = next;
    }
    path
}
pub fn proof_verify(p: &MerkleProof, leaf: [u8;32]) -> bool {
    let mut cur = leaf; let mut idx = p.index as usize;
    for sib in &p.path {
        let (l, r) = if idx % 2 == 0 { (cur, *sib) } else { (*sib, cur) };
        cur = merkle_hash(&l, &r); idx /= 2;
    }
    cur == p.root
}

pub fn encode(payload: &[u8], k: u32, m: u32) -> anyhow::Result<Vec<Shard>> {
    let rs = ReedSolomon::new(k as usize, m as usize)?;
    let shard_len = ((payload.len() + k as usize - 1) / k as usize).max(1);
    let mut shards: Vec<Vec<u8>> = vec![vec![0u8; shard_len]; (k+m) as usize];
    for i in 0..k as usize {
        let start = i * shard_len; let end = (start + shard_len).min(payload.len());
        if start < end { shards[i][..(end-start)].copy_from_slice(&payload[start..end]); }
    }
    rs.encode(&mut shards)?;
    let leaves: Vec<[u8;32]> = shards.iter().map(|s| digest(s)).collect();
    let root = merkle_root(&leaves);
    let mut out = Vec::new();
    for (i, bytes) in shards.into_iter().enumerate() {
        let proof = MerkleProof { root, index: i as u32, path: merkle_proof(&leaves, i) };
        out.push(Shard { index: i as u32, k, m, bytes, proof });
    }
    Ok(out)
}
