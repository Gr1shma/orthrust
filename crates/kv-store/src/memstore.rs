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
#[derive(Serialize, Deserialize, Default)]
pub struct MemStore {
    data: BTreeMap<String, String>,
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

    fn contains(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    fn keys(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }

    fn clear(&mut self) {
        self.data.clear();
    }

    fn scan(&self, prefix: &str) -> Vec<(String, String)> {
        self.data
            .range(prefix.to_string()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
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
            Command::Clear => {
                self.data.clear();
                Ok(None)
            }
            Command::SetMany { entries } => {
                for (k, v) in entries {
                    self.data.insert(k, v);
                }
                Ok(None)
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
    fn contains_key() {
        let mut store = MemStore::new();
        assert!(!store.contains("k"));
        store.set("k", "v".to_string());
        assert!(store.contains("k"));
    }

    #[test]
    fn keys_returns_ordered_list() {
        let mut store = MemStore::new();
        store.set("b", "2".to_string());
        store.set("a", "1".to_string());
        store.set("c", "3".to_string());

        assert_eq!(store.keys(), vec!["a", "b", "c"]);
    }

    #[test]
    fn clear_removes_all_keys() {
        let mut store = MemStore::new();
        store.set("a", "1".to_string());
        store.set("b", "2".to_string());

        store.clear();
        assert!(store.keys().is_empty());
        assert!(!store.contains("a"));
    }

    #[test]
    fn scan_returns_matching_prefix() {
        let mut store = MemStore::new();
        store.set("user:1", "alice".to_string());
        store.set("user:2", "bob".to_string());
        store.set("post:1", "hello".to_string());

        let users = store.scan("user:");
        assert_eq!(
            users,
            vec![
                ("user:1".to_string(), "alice".to_string()),
                ("user:2".to_string(), "bob".to_string())
            ]
        );

        let posts = store.scan("post:");
        assert_eq!(posts, vec![("post:1".to_string(), "hello".to_string())]);

        let empty = store.scan("missing:");
        assert!(empty.is_empty());
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
    fn apply_clear_command() {
        let mut store = MemStore::new();
        store.set("k1", "v1".to_string());
        store.set("k2", "v2".to_string());

        let res = store.apply(Command::Clear).unwrap();
        assert_eq!(res, None);
        assert!(!store.contains("k1"));
        assert!(!store.contains("k2"));
    }

    #[test]
    fn apply_set_many_command() {
        let mut store = MemStore::new();
        let entries = vec![
            ("k1".to_string(), "v1".to_string()),
            ("k2".to_string(), "v2".to_string()),
        ];
        let res = store.apply(Command::SetMany { entries }).unwrap();
        assert_eq!(res, None);
        assert_eq!(store.get("k1").unwrap(), "v1");
        assert_eq!(store.get("k2").unwrap(), "v2");
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
