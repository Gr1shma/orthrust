use crate::command::Command;
use crate::error::StoreError;

/// The interface your Raft state machine will program against.
/// Any backend (in-memory, disk, etc.) implements this.
pub trait Store {
    /// Returns the value for a key, or a `KeyNotFound` error.
    fn get(&self, key: &str) -> Result<String, StoreError>;

    /// Inserts or updates a key-value pair.
    fn set(&mut self, key: &str, value: String);

    /// Removes a key. Returns the old value or a `KeyNotFound` error.
    fn delete(&mut self, key: &str) -> Result<String, StoreError>;

    /// Apply a command from the Raft log to this store.
    fn apply(&mut self, cmd: Command) -> Result<Option<String>, StoreError>;
}
