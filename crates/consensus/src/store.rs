
use super::{QuorumCert, TimeoutCert};
use std::path::{Path, PathBuf};

pub trait QcTcStore: Send + Sync {
    fn load_high_qc(&self) -> Option<QuorumCert>;
    fn save_high_qc(&self, qc: &QuorumCert);
    fn load_high_tc(&self) -> Option<TimeoutCert>;
    fn save_high_tc(&self, tc: &TimeoutCert);
}

pub struct FileStore { dir: PathBuf }
impl FileStore {
    pub fn new<P: AsRef<Path>>(dir: P) -> Self { std::fs::create_dir_all(dir.as_ref()).ok(); Self { dir: dir.as_ref().to_path_buf() } }
    fn qc_path(&self) -> PathBuf { self.dir.join("high_qc.bin") }
    fn tc_path(&self) -> PathBuf { self.dir.join("high_tc.bin") }
}
impl QcTcStore for FileStore {
    fn load_high_qc(&self) -> Option<QuorumCert> {
        let p = self.qc_path(); std::fs::read(&p).ok().and_then(|bytes| bincode::deserialize(&bytes).ok())
    }
    fn save_high_qc(&self, qc: &QuorumCert) { let _ = std::fs::write(self.qc_path(), bincode::serialize(qc).unwrap()); }
    fn load_high_tc(&self) -> Option<TimeoutCert> {
        let p = self.tc_path(); std::fs::read(&p).ok().and_then(|bytes| bincode::deserialize(&bytes).ok())
    }
    fn save_high_tc(&self, tc: &TimeoutCert) { let _ = std::fs::write(self.tc_path(), bincode::serialize(tc).unwrap()); }
}
