
use parking_lot::RwLock;
use std::collections::HashMap;

pub trait Kv: Send + Sync + 'static {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>>;
    fn put(&self, key: &[u8], value: Vec<u8>);
    fn delete(&self, key: &[u8]);
}

#[derive(Default)]
pub struct InMemoryKv { inner: RwLock<HashMap<Vec<u8>, Vec<u8>>> }
impl InMemoryKv { pub fn new() -> Self { Self { inner: RwLock::new(HashMap::new()) } } }
impl Kv for InMemoryKv {
    fn get(&self, key: &[u8]) -> Option<Vec<u8>> { self.inner.read().get(key).cloned() }
    fn put(&self, key: &[u8], value: Vec<u8>) { self.inner.write().insert(key.to_vec(), value); }
    fn delete(&self, key: &[u8]) { self.inner.write().remove(key); }
}
