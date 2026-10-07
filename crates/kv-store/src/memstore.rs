use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::command::Command;
use crate::error::StoreError;
use crate::store::Store;

/// Magic bytes + version prefix for on-disk snapshots.
/// Bump `FORMAT_VERSION` whenever the serialized shape changes incompatibly.
const MAGIC: &[u8; 4] = b"ORTH";
const FORMAT_VERSION: u8 = 1;

/// The simplest possible backend: an in-memory BTreeMap.
///
/// `BTreeMap` (not `HashMap`) because serialization must be deterministic:
/// two replicas with identical logical state must produce identical bytes,
/// or Raft snapshot comparison / transfer breaks.
#[derive(Serialize, Deserialize)]
pub struct MemStore {
    data: BTreeMap<String, String>,
}

impl Default for MemStore {
    fn default() -> Self {
        Self {
            data: BTreeMap::new(),
        }
    }
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Serialize to a versioned byte blob.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        out.extend_from_slice(MAGIC);
        out.push(FORMAT_VERSION);
        // postcard::to_allocvec only fails if the serializer is misused
        // (e.g. a map with >2^32 entries, unsupported types). For our shape
        // it's effectively infallible.
        let body = postcard::to_allocvec(self)
            .expect("MemStore serialization is infallible for our types");
        out.extend_from_slice(&body);
        out
    }

    /// Deserialize from bytes previously produced by `to_bytes`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if bytes.len() < 5 {
            return Err("snapshot too short".into());
        }
        if &bytes[..4] != MAGIC {
            return Err("bad magic bytes".into());
        }
        if bytes[4] != FORMAT_VERSION {
            return Err(format!(
                "unsupported format version {} (expected {})",
                bytes[4], FORMAT_VERSION
            )
            .into());
        }
        Ok(postcard::from_bytes(&bytes[5..])?)
    }
}

impl Store for MemStore {
    fn get(&self, key: &str) -> Result<String, StoreError> {
        self.data
            .get(key)
            .cloned()
            .ok_or_else(|| StoreError::KeyNotFound(key.to_string()))
    }

    fn set(&mut self, key: &str, value: String) {
        self.data.insert(key.to_string(), value);
    }

    fn delete(&mut self, key: &str) -> Result<String, StoreError> {
        self.data
            .remove(key)
            .ok_or_else(|| StoreError::KeyNotFound(key.to_string()))
    }

    /// Called by the Raft core when a log entry is committed.
    /// Must be deterministic, every replica runs this with the same
    /// command in the same order and must reach the same state.
    fn apply(&mut self, cmd: Command) -> Result<Option<String>, StoreError> {
        match cmd {
            Command::Set { key, value } => {
                let prev = self.data.insert(key, value);
                Ok(prev)
            }
            Command::Delete { key } => {
                // Ok(None) if the key was absent - NOT an error.
                // Idempotent retries after a client timeout must produce
                // the same result on every replica.
                Ok(self.data.remove(&key))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_missing_key_returns_error() {
        let store = MemStore::new();
        let result = store.get("nope");
        assert!(matches!(result, Err(StoreError::KeyNotFound(_))));
    }

    #[test]
    fn set_and_get() {
        let mut store = MemStore::new();
        store.set("k", "v".to_string());
        assert_eq!(store.get("k").unwrap(), "v");
    }

    #[test]
    fn set_overwrites_existing() {
        let mut store = MemStore::new();
        store.set("k", "first".to_string());
        store.set("k", "second".to_string());
        assert_eq!(store.get("k").unwrap(), "second");
    }

    #[test]
    fn delete_existing_key() {
        let mut store = MemStore::new();
        store.set("k", "v".to_string());

        let removed = store.delete("k").unwrap();
        assert_eq!(removed, "v");

        // And it's actually gone.
        assert!(matches!(store.get("k"), Err(StoreError::KeyNotFound(_))));
    }

    #[test]
    fn delete_missing_key_returns_error() {
        let mut store = MemStore::new();
        let result = store.delete("nope");
        assert!(matches!(result, Err(StoreError::KeyNotFound(_))));
    }

    #[test]
    fn apply_set_command() {
        let mut store = MemStore::new();

        // First Set: no previous value.
        let prev = store
            .apply(Command::Set {
                key: "k".into(),
                value: "v1".into(),
            })
            .unwrap();
        assert_eq!(prev, None);
        assert_eq!(store.get("k").unwrap(), "v1");

        // Second Set: previous value is returned.
        let prev = store
            .apply(Command::Set {
                key: "k".into(),
                value: "v2".into(),
            })
            .unwrap();
        assert_eq!(prev, Some("v1".to_string()));
        assert_eq!(store.get("k").unwrap(), "v2");
    }

    #[test]
    fn apply_delete_command() {
        let mut store = MemStore::new();
        store.set("k", "v".to_string());

        // Deleting an existing key returns the old value.
        let prev = store.apply(Command::Delete { key: "k".into() }).unwrap();
        assert_eq!(prev, Some("v".to_string()));

        // Deleting a missing key is a no-op - Ok(None), NOT an error.
        // This is what makes log replay idempotent across replicas.
        let prev = store.apply(Command::Delete { key: "k".into() }).unwrap();
        assert_eq!(prev, None);
    }

    #[test]
    fn round_trip_serialization() {
        let mut store = MemStore::new();
        store.set("a", "1".to_string());
        store.set("b", "2".to_string());

        let bytes = store.to_bytes();
        let restored = MemStore::from_bytes(&bytes).unwrap();

        assert_eq!(restored.get("a").unwrap(), "1");
        assert_eq!(restored.get("b").unwrap(), "2");
        assert!(matches!(restored.get("c"), Err(StoreError::KeyNotFound(_))));
    }

    #[test]
    fn from_bytes_rejects_bad_magic() {
        let result = MemStore::from_bytes(b"BADx\x01");
        assert!(result.is_err());
    }

    #[test]
    fn from_bytes_rejects_short_input() {
        let result = MemStore::from_bytes(b"OR");
        assert!(result.is_err());
    }
}
