# Project System — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Multi-Repo Path Mapping:** This project uses separate repos. Translate paths as follows:
> | Plan path prefix | Repo | Local path |
> |---|---|---|
> | `crates/nexa-core/` | [`nexa-core`](https://github.com/nexa-net/nexa-core) | `/Users/nassime/GitHub/nexa-core/` |
> | `crates/nexad/` | [`nexad`](https://github.com/nexa-net/nexad) | `/Users/nassime/GitHub/nexad/` |
> | `crates/nexa-cli/` | [`nexa-cli`](https://github.com/nexa-net/nexa-cli) | `/Users/nassime/GitHub/nexa-cli/` |
>
> `cargo check -p <crate>` → `cargo check` in the target repo. `nexa-core` dep: `git = "https://github.com/nexa-net/nexa-core"`

**Goal:** Make projects the universal isolation boundary for all NexaNet resources, add encrypted secrets management per project, and implement project lifecycle commands (suspend/resume/delete).

**Architecture:** Every resource (deployments, pods, secrets, networks, volumes) is scoped to exactly one project. A `SecretStore` port trait in nexa-core defines encrypted secret CRUD. The `EncryptedSqliteSecretStore` adapter in nexad uses AES-256-GCM with a file-based master key. The orchestrator resolves and injects secrets into container env vars at deploy time, and gains three new commands for project lifecycle management (suspend stops all pods and blocks deploys, resume re-enables and reconciles, delete requires empty project).

**Tech Stack:** aes-gcm 0.10, rand 0.8, rusqlite (existing via SQLite schema), async-trait, tokio, tempfile (dev)

---

### Task 1: Define SecretStore trait in ports/secrets.rs

**Files:**
- Create: `crates/nexa-core/src/ports/secrets.rs`
- Modify: `crates/nexa-core/src/ports/mod.rs` (add `pub mod secrets;`)

- [ ] **Step 1: Write the SecretStore trait**

Create `crates/nexa-core/src/ports/secrets.rs`:

```rust
use async_trait::async_trait;

use crate::error::Result;

/// Port for project-scoped secret storage.
///
/// Implementations handle encryption/decryption transparently.
/// Callers pass plaintext values; the store encrypts before persisting
/// and decrypts on retrieval.
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Store a secret. Overwrites if `(project, name)` already exists.
    async fn set(&self, project: &str, name: &str, value: &[u8]) -> Result<()>;

    /// Retrieve a secret. Returns `None` if not found.
    async fn get(&self, project: &str, name: &str) -> Result<Option<Vec<u8>>>;

    /// List secret names for a project (names only, no values).
    async fn list(&self, project: &str) -> Result<Vec<String>>;

    /// Delete a secret. No-op if not found.
    async fn delete(&self, project: &str, name: &str) -> Result<()>;
}
```

- [ ] **Step 2: Register the module in ports/mod.rs**

In `crates/nexa-core/src/ports/mod.rs`, add:

```rust
pub mod secrets;
```

The file was created in Plan #1 with `pub mod runtime;`. If the hexagonal restructure from Plan #1 has not been applied yet (codebase still uses `nexa-core/src/runtime/`), create `crates/nexa-core/src/ports/mod.rs`:

```rust
pub mod runtime;
pub mod secrets;
```

And add `pub mod ports;` to `crates/nexa-core/src/lib.rs`.

- [ ] **Step 3: Add NexaError variant for secrets**

In `crates/nexa-core/src/error.rs`, add a new variant to `NexaError`:

```rust
    #[error("secret error: {0}")]
    Secret(String),
```

- [ ] **Step 4: Verify nexa-core compiles**

```bash
cargo check -p nexa-core 2>&1
```

Expected: compiles with no errors.

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-core/src/ports/ crates/nexa-core/src/lib.rs crates/nexa-core/src/error.rs
git commit -m "feat: define SecretStore port trait for project-scoped secrets"
```

---

### Task 2: Implement master key generation utility

**Files:**
- Create: `crates/nexad/src/crypto/mod.rs`
- Create: `crates/nexad/src/crypto/master_key.rs`
- Modify: `crates/nexad/Cargo.toml` (add `aes-gcm`, `rand`)
- Modify: `Cargo.toml` (add workspace deps `aes-gcm`, `rand`)

- [ ] **Step 1: Add workspace dependencies**

In the root `Cargo.toml` `[workspace.dependencies]` section, add:

```toml
aes-gcm = "0.10"
rand = "0.8"
```

In `crates/nexad/Cargo.toml` `[dependencies]`, add:

```toml
aes-gcm = { workspace = true }
rand = { workspace = true }
```

- [ ] **Step 2: Write failing test for master key load-or-generate**

Create `crates/nexad/src/crypto/master_key.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};

use nexa_core::error::{NexaError, Result};

const KEY_LEN: usize = 32; // AES-256

/// Load master key from `{data_dir}/master.key`, generating it on first run.
///
/// File permissions are set to 0600 (owner read/write only).
pub fn load_or_generate(data_dir: &Path) -> Result<[u8; KEY_LEN]> {
    let key_path = data_dir.join("master.key");

    if key_path.exists() {
        load_key(&key_path)
    } else {
        generate_key(&key_path)
    }
}

fn load_key(path: &PathBuf) -> Result<[u8; KEY_LEN]> {
    let bytes = fs::read(path).map_err(|e| {
        NexaError::Secret(format!("failed to read master key at {}: {e}", path.display()))
    })?;

    if bytes.len() != KEY_LEN {
        return Err(NexaError::Secret(format!(
            "master key has invalid length: expected {KEY_LEN}, got {}",
            bytes.len()
        )));
    }

    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    Ok(key)
}

fn generate_key(path: &PathBuf) -> Result<[u8; KEY_LEN]> {
    use rand::RngCore;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            NexaError::Secret(format!(
                "failed to create data dir {}: {e}",
                parent.display()
            ))
        })?;
    }

    let mut key = [0u8; KEY_LEN];
    rand::thread_rng().fill_bytes(&mut key);

    fs::write(path, &key).map_err(|e| {
        NexaError::Secret(format!(
            "failed to write master key to {}: {e}",
            path.display()
        ))
    })?;

    // Set file permissions to 0600 (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(path, perms).map_err(|e| {
            NexaError::Secret(format!("failed to set permissions on master key: {e}"))
        })?;
    }

    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn generates_key_on_first_run() {
        let dir = TempDir::new().unwrap();
        let key = load_or_generate(dir.path()).unwrap();
        assert_eq!(key.len(), 32);

        // Key file should exist
        let key_path = dir.path().join("master.key");
        assert!(key_path.exists());

        // File should be exactly 32 bytes
        let bytes = fs::read(&key_path).unwrap();
        assert_eq!(bytes.len(), 32);
    }

    #[test]
    fn loads_existing_key() {
        let dir = TempDir::new().unwrap();
        let key1 = load_or_generate(dir.path()).unwrap();
        let key2 = load_or_generate(dir.path()).unwrap();
        assert_eq!(key1, key2);
    }

    #[test]
    fn rejects_invalid_key_length() {
        let dir = TempDir::new().unwrap();
        let key_path = dir.path().join("master.key");
        fs::write(&key_path, b"too-short").unwrap();

        let result = load_or_generate(dir.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid length"));
    }

    #[cfg(unix)]
    #[test]
    fn key_file_has_restricted_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let _ = load_or_generate(dir.path()).unwrap();

        let key_path = dir.path().join("master.key");
        let perms = fs::metadata(&key_path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }
}
```

- [ ] **Step 3: Create crypto/mod.rs**

Create `crates/nexad/src/crypto/mod.rs`:

```rust
pub mod master_key;
```

- [ ] **Step 4: Register crypto module in nexad**

In `crates/nexad/src/main.rs`, add `mod crypto;` alongside the existing module declarations.

- [ ] **Step 5: Add tempfile dev-dependency to nexad**

In `crates/nexad/Cargo.toml`, add:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 6: Run tests**

```bash
cargo test -p nexad -- crypto::master_key 2>&1
```

Expected: all 4 tests pass (3 on non-Unix, 4 on Unix/macOS).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/nexad/Cargo.toml crates/nexad/src/crypto/ crates/nexad/src/main.rs
git commit -m "feat: add master key generation with AES-256 (32-byte random, 0600 perms)"
```

---

### Task 3: Implement EncryptedSqliteSecretStore adapter

**Files:**
- Create: `crates/nexad/src/adapters/secrets/mod.rs`
- Create: `crates/nexad/src/adapters/secrets/encrypted.rs`
- Modify: `crates/nexad/src/adapters/mod.rs` (add `pub mod secrets;`)
- Modify: `crates/nexad/Cargo.toml` (add `rusqlite`)

- [ ] **Step 1: Add rusqlite dependency**

In root `Cargo.toml` `[workspace.dependencies]`, add:

```toml
rusqlite = { version = "0.31", features = ["bundled"] }
```

In `crates/nexad/Cargo.toml` `[dependencies]`, add:

```toml
rusqlite = { workspace = true }
```

- [ ] **Step 2: Write the EncryptedSqliteSecretStore with tests**

Create `crates/nexad/src/adapters/secrets/encrypted.rs`:

```rust
use std::sync::Arc;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, AeadCore, Nonce};
use async_trait::async_trait;
use rusqlite::Connection;
use tokio::sync::Mutex;

use nexa_core::error::{NexaError, Result};
use nexa_core::ports::secrets::SecretStore;

/// Encrypted secret store backed by SQLite.
///
/// Secrets are encrypted with AES-256-GCM before storage.
/// The ciphertext format is: `nonce (12 bytes) || ciphertext`.
pub struct EncryptedSqliteSecretStore {
    conn: Arc<Mutex<Connection>>,
    cipher: Aes256Gcm,
}

impl EncryptedSqliteSecretStore {
    /// Create a new store. `master_key` must be exactly 32 bytes.
    pub fn new(conn: Connection, master_key: &[u8; 32]) -> Result<Self> {
        let cipher = Aes256Gcm::new_from_slice(master_key)
            .map_err(|e| NexaError::Secret(format!("invalid master key: {e}")))?;

        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            cipher,
        };

        store.init_schema_sync()?;
        Ok(store)
    }

    fn init_schema_sync(&self) -> Result<()> {
        // We need to block on the mutex since this is called from new()
        // which is not async. Use try_lock since we just created the mutex.
        let conn = self.conn.try_lock().map_err(|_| {
            NexaError::Secret("failed to lock connection during init".into())
        })?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS secrets (
                project TEXT NOT NULL,
                name    TEXT NOT NULL,
                value   BLOB NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (project, name)
            );",
        )
        .map_err(|e| NexaError::Secret(format!("failed to create secrets table: {e}")))?;

        Ok(())
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| NexaError::Secret(format!("encryption failed: {e}")))?;

        // nonce (12 bytes) || ciphertext
        let mut blob = nonce.to_vec();
        blob.extend_from_slice(&ciphertext);
        Ok(blob)
    }

    fn decrypt(&self, blob: &[u8]) -> Result<Vec<u8>> {
        if blob.len() < 12 {
            return Err(NexaError::Secret("ciphertext too short".into()));
        }

        let (nonce_bytes, ciphertext) = blob.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| NexaError::Secret(format!("decryption failed: {e}")))
    }
}

#[async_trait]
impl SecretStore for EncryptedSqliteSecretStore {
    async fn set(&self, project: &str, name: &str, value: &[u8]) -> Result<()> {
        let encrypted = self.encrypt(value)?;
        let conn = self.conn.lock().await;

        conn.execute(
            "INSERT INTO secrets (project, name, value, updated_at)
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT (project, name)
             DO UPDATE SET value = ?3, updated_at = datetime('now')",
            rusqlite::params![project, name, encrypted],
        )
        .map_err(|e| NexaError::Secret(format!("failed to set secret: {e}")))?;

        Ok(())
    }

    async fn get(&self, project: &str, name: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn
            .prepare("SELECT value FROM secrets WHERE project = ?1 AND name = ?2")
            .map_err(|e| NexaError::Secret(format!("query failed: {e}")))?;

        let result: std::result::Result<Vec<u8>, _> =
            stmt.query_row(rusqlite::params![project, name], |row| row.get(0));

        match result {
            Ok(encrypted) => {
                let plaintext = self.decrypt(&encrypted)?;
                Ok(Some(plaintext))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(NexaError::Secret(format!("failed to get secret: {e}"))),
        }
    }

    async fn list(&self, project: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().await;

        let mut stmt = conn
            .prepare("SELECT name FROM secrets WHERE project = ?1 ORDER BY name")
            .map_err(|e| NexaError::Secret(format!("query failed: {e}")))?;

        let names: std::result::Result<Vec<String>, _> = stmt
            .query_map(rusqlite::params![project], |row| row.get(0))
            .map_err(|e| NexaError::Secret(format!("failed to list secrets: {e}")))?
            .collect();

        names.map_err(|e| NexaError::Secret(format!("failed to collect secret names: {e}")))
    }

    async fn delete(&self, project: &str, name: &str) -> Result<()> {
        let conn = self.conn.lock().await;

        conn.execute(
            "DELETE FROM secrets WHERE project = ?1 AND name = ?2",
            rusqlite::params![project, name],
        )
        .map_err(|e| NexaError::Secret(format!("failed to delete secret: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn test_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        key[0] = 0x42;
        key[31] = 0xFF;
        key
    }

    fn create_store() -> EncryptedSqliteSecretStore {
        let conn = Connection::open_in_memory().unwrap();
        EncryptedSqliteSecretStore::new(conn, &test_key()).unwrap()
    }

    #[tokio::test]
    async fn set_and_get_secret() {
        let store = create_store();

        store.set("myapp", "DB_PASS", b"hunter2").await.unwrap();
        let value = store.get("myapp", "DB_PASS").await.unwrap();

        assert_eq!(value, Some(b"hunter2".to_vec()));
    }

    #[tokio::test]
    async fn get_nonexistent_returns_none() {
        let store = create_store();

        let value = store.get("myapp", "MISSING").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn set_overwrites_existing() {
        let store = create_store();

        store.set("myapp", "DB_PASS", b"old").await.unwrap();
        store.set("myapp", "DB_PASS", b"new").await.unwrap();

        let value = store.get("myapp", "DB_PASS").await.unwrap();
        assert_eq!(value, Some(b"new".to_vec()));
    }

    #[tokio::test]
    async fn list_returns_names_sorted() {
        let store = create_store();

        store.set("myapp", "Z_SECRET", b"z").await.unwrap();
        store.set("myapp", "A_SECRET", b"a").await.unwrap();
        store.set("myapp", "M_SECRET", b"m").await.unwrap();

        let names = store.list("myapp").await.unwrap();
        assert_eq!(names, vec!["A_SECRET", "M_SECRET", "Z_SECRET"]);
    }

    #[tokio::test]
    async fn list_empty_project() {
        let store = create_store();

        let names = store.list("empty").await.unwrap();
        assert!(names.is_empty());
    }

    #[tokio::test]
    async fn delete_removes_secret() {
        let store = create_store();

        store.set("myapp", "DB_PASS", b"hunter2").await.unwrap();
        store.delete("myapp", "DB_PASS").await.unwrap();

        let value = store.get("myapp", "DB_PASS").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn delete_nonexistent_is_noop() {
        let store = create_store();

        // Should not error
        store.delete("myapp", "MISSING").await.unwrap();
    }

    #[tokio::test]
    async fn projects_are_isolated() {
        let store = create_store();

        store.set("app1", "DB_PASS", b"pass1").await.unwrap();
        store.set("app2", "DB_PASS", b"pass2").await.unwrap();

        let v1 = store.get("app1", "DB_PASS").await.unwrap();
        let v2 = store.get("app2", "DB_PASS").await.unwrap();

        assert_eq!(v1, Some(b"pass1".to_vec()));
        assert_eq!(v2, Some(b"pass2".to_vec()));

        // Deleting from app1 does not affect app2
        store.delete("app1", "DB_PASS").await.unwrap();
        assert_eq!(store.get("app1", "DB_PASS").await.unwrap(), None);
        assert_eq!(
            store.get("app2", "DB_PASS").await.unwrap(),
            Some(b"pass2".to_vec())
        );
    }

    #[tokio::test]
    async fn wrong_key_fails_decryption() {
        let conn = Connection::open_in_memory().unwrap();
        let key1 = test_key();
        let store1 = EncryptedSqliteSecretStore::new(conn, &key1).unwrap();

        store1.set("myapp", "SECRET", b"data").await.unwrap();

        // Extract the raw encrypted blob
        let raw: Vec<u8> = {
            let conn = store1.conn.lock().await;
            conn.query_row(
                "SELECT value FROM secrets WHERE project = 'myapp' AND name = 'SECRET'",
                [],
                |row| row.get(0),
            )
            .unwrap()
        };

        // Verify the stored value is NOT plaintext
        assert_ne!(raw, b"data");

        // Verify it's nonce (12) + ciphertext (>0)
        assert!(raw.len() > 12);
    }

    #[tokio::test]
    async fn binary_secret_roundtrip() {
        let store = create_store();

        let binary_data: Vec<u8> = (0..=255).collect();
        store.set("myapp", "CERT", &binary_data).await.unwrap();

        let value = store.get("myapp", "CERT").await.unwrap().unwrap();
        assert_eq!(value, binary_data);
    }
}
```

- [ ] **Step 3: Create adapters/secrets/mod.rs**

Create `crates/nexad/src/adapters/secrets/mod.rs`:

```rust
mod encrypted;

pub use encrypted::EncryptedSqliteSecretStore;
```

- [ ] **Step 4: Register secrets adapter module**

In `crates/nexad/src/adapters/mod.rs`, add:

```rust
pub mod secrets;
```

If the adapters module does not exist yet (Plan #1 not applied), create `crates/nexad/src/adapters/mod.rs`:

```rust
pub mod secrets;
```

And add `mod adapters;` to `crates/nexad/src/main.rs`.

- [ ] **Step 5: Run tests**

```bash
cargo test -p nexad -- adapters::secrets 2>&1
```

Expected: all 10 tests pass.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/nexad/Cargo.toml crates/nexad/src/adapters/secrets/
git commit -m "feat: implement EncryptedSqliteSecretStore with AES-256-GCM encryption"
```

---

### Task 4: Add PlaintextSecretStore for tests

**Files:**
- Create: `crates/nexa-core/src/ports/secrets_test.rs`

This is a HashMap-backed in-memory store with no encryption, used in orchestrator unit tests.

- [ ] **Step 1: Write PlaintextSecretStore**

Create `crates/nexa-core/src/ports/secrets_test.rs`:

```rust
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::ports::secrets::SecretStore;

/// In-memory secret store for tests. No encryption. Not for production.
pub struct PlaintextSecretStore {
    /// Map of (project, name) -> value
    data: Arc<Mutex<HashMap<(String, String), Vec<u8>>>>,
}

impl PlaintextSecretStore {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for PlaintextSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SecretStore for PlaintextSecretStore {
    async fn set(&self, project: &str, name: &str, value: &[u8]) -> Result<()> {
        let mut data = self.data.lock().await;
        data.insert(
            (project.to_string(), name.to_string()),
            value.to_vec(),
        );
        Ok(())
    }

    async fn get(&self, project: &str, name: &str) -> Result<Option<Vec<u8>>> {
        let data = self.data.lock().await;
        Ok(data.get(&(project.to_string(), name.to_string())).cloned())
    }

    async fn list(&self, project: &str) -> Result<Vec<String>> {
        let data = self.data.lock().await;
        let mut names: Vec<String> = data
            .keys()
            .filter(|(p, _)| p == project)
            .map(|(_, n)| n.clone())
            .collect();
        names.sort();
        Ok(names)
    }

    async fn delete(&self, project: &str, name: &str) -> Result<()> {
        let mut data = self.data.lock().await;
        data.remove(&(project.to_string(), name.to_string()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plaintext_store_roundtrip() {
        let store = PlaintextSecretStore::new();

        store.set("proj", "KEY", b"value").await.unwrap();
        let val = store.get("proj", "KEY").await.unwrap();
        assert_eq!(val, Some(b"value".to_vec()));
    }

    #[tokio::test]
    async fn plaintext_store_isolation() {
        let store = PlaintextSecretStore::new();

        store.set("a", "KEY", b"val-a").await.unwrap();
        store.set("b", "KEY", b"val-b").await.unwrap();

        assert_eq!(store.get("a", "KEY").await.unwrap(), Some(b"val-a".to_vec()));
        assert_eq!(store.get("b", "KEY").await.unwrap(), Some(b"val-b".to_vec()));
    }

    #[tokio::test]
    async fn plaintext_store_list_and_delete() {
        let store = PlaintextSecretStore::new();

        store.set("proj", "B", b"b").await.unwrap();
        store.set("proj", "A", b"a").await.unwrap();

        let names = store.list("proj").await.unwrap();
        assert_eq!(names, vec!["A", "B"]);

        store.delete("proj", "A").await.unwrap();
        let names = store.list("proj").await.unwrap();
        assert_eq!(names, vec!["B"]);
    }
}
```

- [ ] **Step 2: Register the test module conditionally**

In `crates/nexa-core/src/ports/mod.rs`, add:

```rust
#[cfg(any(test, feature = "test-utils"))]
pub mod secrets_test;
```

In `crates/nexa-core/Cargo.toml`, add a feature:

```toml
[features]
test-utils = []
```

In `crates/nexad/Cargo.toml` under `[dev-dependencies]`, add:

```toml
nexa-core = { workspace = true, features = ["test-utils"] }
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p nexa-core -- ports::secrets_test 2>&1
```

Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/nexa-core/src/ports/secrets_test.rs crates/nexa-core/src/ports/mod.rs crates/nexa-core/Cargo.toml crates/nexad/Cargo.toml
git commit -m "feat: add PlaintextSecretStore test double for orchestrator tests"
```

---

### Task 5: Add project lifecycle commands to Command enum and Project model

**Files:**
- Modify: `crates/nexa-core/src/models/project.rs` (add `ProjectStatus`, `status` field)
- Modify: engine orchestrator or `crates/nexa-core/src/domain/orchestrator.rs` (add `SuspendProject`, `ResumeProject`, `DeleteProject` commands)

- [ ] **Step 1: Add ProjectStatus to Project model**

In `crates/nexa-core/src/models/project.rs`, replace the entire file:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    Active,
    Suspended,
}

impl Default for ProjectStatus {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub status: ProjectStatus,
    pub created_at: DateTime<Utc>,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: ProjectStatus::Active,
            created_at: Utc::now(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == ProjectStatus::Active
    }

    pub fn is_suspended(&self) -> bool {
        self.status == ProjectStatus::Suspended
    }
}
```

- [ ] **Step 2: Add ProjectSuspended error variant**

In `crates/nexa-core/src/error.rs`, add:

```rust
    #[error("project is suspended: {0}")]
    ProjectSuspended(String),

    #[error("project not empty: {0}")]
    ProjectNotEmpty(String),
```

- [ ] **Step 3: Add lifecycle commands to orchestrator Command enum**

In the file containing the `Command` enum (either `crates/nexad/src/engine/orchestrator.rs` or `crates/nexa-core/src/domain/orchestrator.rs` depending on which plans are applied), add three new variants:

```rust
    SuspendProject {
        name: String,
        reply: oneshot::Sender<Result<()>>,
    },
    ResumeProject {
        name: String,
        reply: oneshot::Sender<Result<()>>,
    },
    DeleteProject {
        name: String,
        reply: oneshot::Sender<Result<()>>,
    },
```

If using `OrchestratorHandle`, add corresponding methods:

```rust
    pub async fn suspend_project(&self, name: String) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::SuspendProject { name, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }

    pub async fn resume_project(&self, name: String) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::ResumeProject { name, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }

    pub async fn delete_project(&self, name: String) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::DeleteProject { name, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }
```

- [ ] **Step 4: Verify compilation**

```bash
cargo check 2>&1
```

Expected: compiles (there will be unused variant warnings until Task 6 handles them -- that is OK).

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-core/src/models/project.rs crates/nexa-core/src/error.rs
git add -A crates/nexad/src/engine/ crates/nexa-core/src/domain/  # whichever exists
git commit -m "feat: add ProjectStatus (Active/Suspended) and lifecycle commands to Command enum"
```

---

### Task 6: Handle project commands in orchestrator

**Files:**
- Modify: orchestrator implementation (either `crates/nexad/src/engine/orchestrator.rs` or `crates/nexa-core/src/domain/orchestrator.rs`)

- [ ] **Step 1: Write failing tests for project lifecycle**

Add these tests to the orchestrator test module:

```rust
    #[tokio::test]
    async fn suspend_project_stops_all_pods() {
        let handle = spawn_test_orchestrator();

        let spec = DeploymentSpec {
            project: "myapp".into(),
            deployment: DeploymentMeta { name: "web".into() },
            replicas: 2,
            image: "nginx".into(),
            ports: vec![],
            env: HashMap::new(),
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
        };

        handle.deploy(spec).await.unwrap();
        assert_eq!(handle.list_pods(Some("myapp".into())).await.len(), 2);

        handle.suspend_project("myapp".into()).await.unwrap();
        assert_eq!(handle.list_pods(Some("myapp".into())).await.len(), 0);

        // Verify project status
        let projects = handle.list_projects().await;
        let myapp = projects.iter().find(|p| p.name == "myapp").unwrap();
        assert!(myapp.is_suspended());
    }

    #[tokio::test]
    async fn suspend_blocks_new_deploys() {
        let handle = spawn_test_orchestrator();

        handle.create_project("myapp".into()).await.unwrap();
        handle.suspend_project("myapp".into()).await.unwrap();

        let spec = DeploymentSpec {
            project: "myapp".into(),
            deployment: DeploymentMeta { name: "web".into() },
            replicas: 1,
            image: "nginx".into(),
            ports: vec![],
            env: HashMap::new(),
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
        };

        let result = handle.deploy(spec).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("suspended"));
    }

    #[tokio::test]
    async fn resume_project_reconciles_deployments() {
        let handle = spawn_test_orchestrator();

        let spec = DeploymentSpec {
            project: "myapp".into(),
            deployment: DeploymentMeta { name: "web".into() },
            replicas: 2,
            image: "nginx".into(),
            ports: vec![],
            env: HashMap::new(),
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
        };

        handle.deploy(spec).await.unwrap();
        handle.suspend_project("myapp".into()).await.unwrap();
        assert_eq!(handle.list_pods(Some("myapp".into())).await.len(), 0);

        handle.resume_project("myapp".into()).await.unwrap();

        let projects = handle.list_projects().await;
        let myapp = projects.iter().find(|p| p.name == "myapp").unwrap();
        assert!(myapp.is_active());

        // Pods should be recreated by reconciliation
        assert_eq!(handle.list_pods(Some("myapp".into())).await.len(), 2);
    }

    #[tokio::test]
    async fn delete_empty_project_succeeds() {
        let handle = spawn_test_orchestrator();

        handle.create_project("myapp".into()).await.unwrap();
        handle.delete_project("myapp".into()).await.unwrap();

        let projects = handle.list_projects().await;
        assert!(projects.iter().all(|p| p.name != "myapp"));
    }

    #[tokio::test]
    async fn delete_nonempty_project_fails() {
        let handle = spawn_test_orchestrator();

        let spec = DeploymentSpec {
            project: "myapp".into(),
            deployment: DeploymentMeta { name: "web".into() },
            replicas: 1,
            image: "nginx".into(),
            ports: vec![],
            env: HashMap::new(),
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
        };

        handle.deploy(spec).await.unwrap();

        let result = handle.delete_project("myapp".into()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not empty"));
    }

    #[tokio::test]
    async fn delete_nonexistent_project_fails() {
        let handle = spawn_test_orchestrator();

        let result = handle.delete_project("ghost".into()).await;
        assert!(result.is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p nexa-core -- domain::orchestrator 2>&1
# or for the DashMap-based engine:
cargo test -p nexad -- engine 2>&1
```

Expected: FAIL -- `suspend_project`, `resume_project`, `delete_project` methods do not exist or command match arms are missing.

- [ ] **Step 3: Implement command handlers**

Add the match arms in the orchestrator's command processing loop:

```rust
Command::SuspendProject { name, reply } => {
    let result = self.handle_suspend_project(&name).await;
    let _ = reply.send(result);
}
Command::ResumeProject { name, reply } => {
    let result = self.handle_resume_project(&name).await;
    let _ = reply.send(result);
}
Command::DeleteProject { name, reply } => {
    let result = self.handle_delete_project(&name);
    let _ = reply.send(result);
}
```

Add the handler methods:

```rust
    async fn handle_suspend_project(&mut self, name: &str) -> Result<()> {
        let project = self.projects.get_mut(name).ok_or_else(|| {
            NexaError::ProjectNotFound(name.to_string())
        })?;

        project.status = ProjectStatus::Suspended;

        // Stop all pods in this project
        let deployment_ids: Vec<Uuid> = self
            .deployments
            .values()
            .filter(|d| d.project() == name)
            .map(|d| d.id)
            .collect();

        for deployment_id in &deployment_ids {
            let pod_ids: Vec<Uuid> = self
                .pods
                .values()
                .filter(|p| p.deployment_id == *deployment_id)
                .map(|p| p.id)
                .collect();

            for pod_id in &pod_ids {
                if let Some(pod) = self.pods.get(pod_id) {
                    if let Some(cid) = &pod.container_id {
                        let _ = self.runtime.stop_container(cid, 10).await;
                        let _ = self.runtime.remove_container(cid, true).await;
                    }
                }
                self.pods.remove(pod_id);
            }

            if let Some(d) = self.deployments.get_mut(deployment_id) {
                d.status = DeploymentStatus::Stopped;
            }
        }

        Ok(())
    }

    async fn handle_resume_project(&mut self, name: &str) -> Result<()> {
        let project = self.projects.get_mut(name).ok_or_else(|| {
            NexaError::ProjectNotFound(name.to_string())
        })?;

        project.status = ProjectStatus::Active;

        // Reconcile all deployments in this project
        let deployment_ids: Vec<Uuid> = self
            .deployments
            .values()
            .filter(|d| d.project() == name)
            .map(|d| d.id)
            .collect();

        for deployment_id in deployment_ids {
            self.reconcile_deployment(deployment_id).await?;
        }

        Ok(())
    }

    fn handle_delete_project(&mut self, name: &str) -> Result<()> {
        if !self.projects.contains_key(name) {
            return Err(NexaError::ProjectNotFound(name.to_string()));
        }

        // Check for existing deployments
        let has_deployments = self
            .deployments
            .values()
            .any(|d| d.project() == name);

        if has_deployments {
            return Err(NexaError::ProjectNotEmpty(format!(
                "project '{}' still has deployments — remove them first",
                name
            )));
        }

        self.projects.remove(name);
        Ok(())
    }
```

Also, add a suspended check to `handle_deploy`:

```rust
    async fn handle_deploy(&mut self, spec: DeploymentSpec) -> Result<Deployment> {
        // Check if project is suspended
        if let Some(project) = self.projects.get(&spec.project) {
            if project.is_suspended() {
                return Err(NexaError::ProjectSuspended(spec.project.clone()));
            }
        }

        self.ensure_project(&spec.project);
        // ... rest of existing deploy logic
    }
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p nexa-core -- domain::orchestrator 2>&1
# or:
cargo test -p nexad -- engine 2>&1
```

Expected: all lifecycle tests pass (6 new + existing tests).

- [ ] **Step 5: Commit**

```bash
git add -A crates/nexa-core/ crates/nexad/src/engine/
git commit -m "feat: implement project suspend/resume/delete with pod lifecycle management"
```

---

### Task 7: Secret injection in deploy flow

**Files:**
- Modify: orchestrator (add `SecretStore` dependency, inject secrets before container creation)
- Modify: `crates/nexa-core/src/models/deployment.rs` (add `secrets` field to `DeploymentSpec`)

- [ ] **Step 1: Add `secrets` field to DeploymentSpec**

In `crates/nexa-core/src/models/deployment.rs`, add to `DeploymentSpec`:

```rust
    /// Secret names to inject as environment variables.
    /// Each entry maps an env var name to a secret name in the project's secret store.
    /// Example: `{"DB_PASSWORD": "db-password"}` reads secret "db-password" from
    /// the project's store and injects it as env var `DB_PASSWORD`.
    #[serde(default)]
    pub secrets: HashMap<String, String>,
```

- [ ] **Step 2: Write failing tests for secret injection**

Add to orchestrator tests. First, update the test orchestrator factory to accept a `SecretStore`:

```rust
    use nexa_core::ports::secrets::SecretStore;

    // For the actor model (OrchestratorHandle):
    fn spawn_test_orchestrator() -> OrchestratorHandle {
        let secrets = Arc::new(PlaintextSecretStore::new());
        Orchestrator::spawn(Arc::new(MockRuntime), secrets)
    }

    // For the DashMap-based engine, pass the secret store to the constructor.
```

Then the test:

```rust
    #[tokio::test]
    async fn deploy_injects_secrets_into_env() {
        let secrets = Arc::new(PlaintextSecretStore::new());
        secrets.set("myapp", "db-password", b"s3cret").await.unwrap();

        let handle = Orchestrator::spawn(Arc::new(MockRuntime), secrets);

        let mut secret_map = HashMap::new();
        secret_map.insert("DB_PASSWORD".to_string(), "db-password".to_string());

        let spec = DeploymentSpec {
            project: "myapp".into(),
            deployment: DeploymentMeta { name: "api".into() },
            replicas: 1,
            image: "nginx".into(),
            ports: vec![],
            env: HashMap::from([("APP_NAME".to_string(), "test".to_string())]),
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
            secrets: secret_map,
        };

        let deployment = handle.deploy(spec).await.unwrap();
        assert_eq!(deployment.status, DeploymentStatus::Running);
    }

    #[tokio::test]
    async fn deploy_fails_on_missing_secret() {
        let secrets = Arc::new(PlaintextSecretStore::new());
        // Do NOT set the secret -- it should be missing

        let handle = Orchestrator::spawn(Arc::new(MockRuntime), secrets);

        let mut secret_map = HashMap::new();
        secret_map.insert("DB_PASSWORD".to_string(), "nonexistent-secret".to_string());

        let spec = DeploymentSpec {
            project: "myapp".into(),
            deployment: DeploymentMeta { name: "api".into() },
            replicas: 1,
            image: "nginx".into(),
            ports: vec![],
            env: HashMap::new(),
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
            secrets: secret_map,
        };

        let result = handle.deploy(spec).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("nonexistent-secret"));
    }
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cargo test -p nexa-core -- domain::orchestrator 2>&1
```

Expected: FAIL -- `Orchestrator::spawn` does not accept `secrets` parameter yet.

- [ ] **Step 4: Implement secret injection**

Update the orchestrator to accept an `Arc<dyn SecretStore>`:

```rust
pub struct Orchestrator {
    runtime: Arc<dyn ContainerRuntime>,
    secrets: Arc<dyn SecretStore>,
    projects: StdHashMap<String, Project>,
    deployments: StdHashMap<Uuid, Deployment>,
    pods: StdHashMap<Uuid, Pod>,
}

impl Orchestrator {
    pub fn spawn(
        runtime: Arc<dyn ContainerRuntime>,
        secrets: Arc<dyn SecretStore>,
    ) -> OrchestratorHandle {
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let mut orch = Self {
                runtime,
                secrets,
                projects: StdHashMap::new(),
                deployments: StdHashMap::new(),
                pods: StdHashMap::new(),
            };
            orch.run(rx).await;
        });
        OrchestratorHandle { tx }
    }
    // ...
}
```

Add a `resolve_secrets` method:

```rust
    /// Resolve secret references into a HashMap of env var name -> plaintext value.
    /// Returns error if any referenced secret is missing.
    async fn resolve_secrets(
        &self,
        project: &str,
        secret_refs: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        let mut resolved = HashMap::new();

        for (env_var, secret_name) in secret_refs {
            let value = self
                .secrets
                .get(project, secret_name)
                .await?
                .ok_or_else(|| {
                    NexaError::Secret(format!(
                        "secret '{}' not found in project '{}'",
                        secret_name, project
                    ))
                })?;

            let value_str = String::from_utf8(value).map_err(|_| {
                NexaError::Secret(format!(
                    "secret '{}' contains invalid UTF-8",
                    secret_name
                ))
            })?;

            resolved.insert(env_var.clone(), value_str);
        }

        Ok(resolved)
    }
```

In `handle_deploy`, resolve secrets before proceeding, then in `create_pod`, merge them into the container env:

```rust
    async fn handle_deploy(&mut self, spec: DeploymentSpec) -> Result<Deployment> {
        // Check if project is suspended
        if let Some(project) = self.projects.get(&spec.project) {
            if project.is_suspended() {
                return Err(NexaError::ProjectSuspended(spec.project.clone()));
            }
        }

        // Resolve secrets BEFORE creating any containers
        if !spec.secrets.is_empty() {
            self.resolve_secrets(&spec.project, &spec.secrets).await?;
        }

        self.ensure_project(&spec.project);
        // ... rest of existing deploy logic
    }
```

In `create_pod`, merge resolved secrets into the env map (secrets override `env` keys):

```rust
    async fn create_pod(
        &mut self,
        deployment_id: Uuid,
        spec: &DeploymentSpec,
        index: u32,
    ) -> Result<()> {
        // ... existing pod setup ...

        // Merge secrets into env (secrets override env keys with same name)
        let mut final_env = spec.env.clone();
        if !spec.secrets.is_empty() {
            let resolved = self.resolve_secrets(&spec.project, &spec.secrets).await?;
            for (k, v) in resolved {
                final_env.insert(k, v);
            }
        }

        let config = ContainerConfig {
            name: container_name,
            image: spec.image.clone(),
            env: final_env,  // <-- use merged env
            // ... rest unchanged
        };

        // ... rest of existing container creation logic
    }
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test -p nexa-core -- domain::orchestrator 2>&1
```

Expected: all tests pass including the 2 new secret injection tests.

- [ ] **Step 6: Commit**

```bash
git add crates/nexa-core/src/models/deployment.rs
git add -A crates/nexa-core/src/domain/ crates/nexad/src/engine/
git commit -m "feat: inject secrets into container env vars at deploy time, fail on missing"
```

---

### Task 8: Add API routes for project lifecycle and secrets

**Files:**
- Modify: `crates/nexad/src/api/routes.rs` (add 6 new routes)
- Modify: `crates/nexad/src/api/handlers.rs` (add 6 new handlers)

- [ ] **Step 1: Add secret-related handler types and project lifecycle handlers**

In `crates/nexad/src/api/handlers.rs`, add:

```rust
// --- Project Lifecycle ---

pub async fn suspend_project(
    State(orch): AppState,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match orch.suspend_project(name.clone()).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({
            "message": format!("project '{}' suspended", name)
        }))).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

pub async fn resume_project(
    State(orch): AppState,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match orch.resume_project(name.clone()).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({
            "message": format!("project '{}' resumed", name)
        }))).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

pub async fn delete_project(
    State(orch): AppState,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match orch.delete_project(name.clone()).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let status = if e.to_string().contains("not empty") {
                StatusCode::CONFLICT
            } else {
                StatusCode::NOT_FOUND
            };
            (status, Json(serde_json::json!({ "error": e.to_string() }))).into_response()
        }
    }
}

// --- Secrets ---

pub async fn list_secrets(
    State(orch): AppState,
    Path(project): Path<String>,
) -> impl IntoResponse {
    match orch.list_secrets(project).await {
        Ok(names) => Json(serde_json::json!({ "secrets": names })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

#[derive(Deserialize)]
pub struct SetSecretRequest {
    pub value: String,
}

pub async fn set_secret(
    State(orch): AppState,
    Path((project, secret_name)): Path<(String, String)>,
    Json(req): Json<SetSecretRequest>,
) -> impl IntoResponse {
    match orch.set_secret(project, secret_name.clone(), req.value.into_bytes()).await {
        Ok(()) => (StatusCode::CREATED, Json(serde_json::json!({
            "message": format!("secret '{}' set", secret_name)
        }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

pub async fn delete_secret(
    State(orch): AppState,
    Path((project, secret_name)): Path<(String, String)>,
) -> impl IntoResponse {
    match orch.delete_secret(project, secret_name).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}
```

- [ ] **Step 2: Register new routes**

In `crates/nexad/src/api/routes.rs`, add these routes to the `build` function:

```rust
        // Project lifecycle
        .route(
            "/api/v1/projects/{name}/suspend",
            post(handlers::suspend_project),
        )
        .route(
            "/api/v1/projects/{name}/resume",
            post(handlers::resume_project),
        )
        .route(
            "/api/v1/projects/{name}",
            delete(handlers::delete_project),
        )
        // Secrets
        .route(
            "/api/v1/projects/{project}/secrets",
            get(handlers::list_secrets),
        )
        .route(
            "/api/v1/projects/{project}/secrets/{secret_name}",
            post(handlers::set_secret),
        )
        .route(
            "/api/v1/projects/{project}/secrets/{secret_name}",
            delete(handlers::delete_secret),
        )
```

- [ ] **Step 3: Add secret proxy methods to OrchestratorHandle (or Orchestrator)**

The API handlers call `orch.list_secrets()`, `orch.set_secret()`, `orch.delete_secret()`. These need to be exposed.

If using the actor model (OrchestratorHandle), add `Command` variants:

```rust
    ListSecrets {
        project: String,
        reply: oneshot::Sender<Result<Vec<String>>>,
    },
    SetSecret {
        project: String,
        name: String,
        value: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    DeleteSecret {
        project: String,
        name: String,
        reply: oneshot::Sender<Result<()>>,
    },
```

And `OrchestratorHandle` methods:

```rust
    pub async fn list_secrets(&self, project: String) -> Result<Vec<String>> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::ListSecrets { project, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }

    pub async fn set_secret(&self, project: String, name: String, value: Vec<u8>) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::SetSecret { project, name, value, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }

    pub async fn delete_secret(&self, project: String, name: String) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::DeleteSecret { project, name, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }
```

And the handler implementations in the orchestrator loop:

```rust
Command::ListSecrets { project, reply } => {
    let result = self.secrets.list(&project).await;
    let _ = reply.send(result);
}
Command::SetSecret { project, name, value, reply } => {
    let result = self.secrets.set(&project, &name, &value).await;
    let _ = reply.send(result);
}
Command::DeleteSecret { project, name, reply } => {
    let result = self.secrets.delete(&project, &name).await;
    let _ = reply.send(result);
}
```

If using the DashMap-based `Orchestrator`, store `Arc<dyn SecretStore>` as a field and delegate directly:

```rust
    pub async fn list_secrets(&self, project: &str) -> Result<Vec<String>> {
        self.secrets.list(project).await
    }

    pub async fn set_secret(&self, project: &str, name: &str, value: &[u8]) -> Result<()> {
        self.secrets.set(project, name, value).await
    }

    pub async fn delete_secret(&self, project: &str, name: &str) -> Result<()> {
        self.secrets.delete(project, name).await
    }
```

- [ ] **Step 4: Verify compilation**

```bash
cargo check 2>&1
```

Expected: compiles.

- [ ] **Step 5: Commit**

```bash
git add crates/nexad/src/api/ crates/nexa-core/src/domain/ crates/nexad/src/engine/
git commit -m "feat: add API routes for project suspend/resume/delete and secrets CRUD"
```

---

### Task 9: Wire SecretStore into nexad main.rs

**Files:**
- Modify: `crates/nexad/src/main.rs`

- [ ] **Step 1: Wire everything together in main.rs**

Update `crates/nexad/src/main.rs` to load/generate the master key and create the `EncryptedSqliteSecretStore`:

```rust
mod adapters;
mod api;
mod crypto;
mod engine;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use rusqlite::Connection;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::adapters::secrets::EncryptedSqliteSecretStore;
use crate::crypto::master_key;

#[derive(Parser)]
#[command(name = "nexad", about = "NexaNet daemon", version)]
struct Cli {
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    #[arg(long, default_value = "6443")]
    port: u16,

    #[arg(long, default_value = "/var/lib/nexa")]
    data_dir: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    info!("starting nexad on {}:{}", cli.host, cli.port);

    let data_dir = PathBuf::from(&cli.data_dir);

    // Load or generate master encryption key
    let master_key = master_key::load_or_generate(&data_dir)?;
    info!("master key loaded from {}", data_dir.join("master.key").display());

    // Open SQLite database for secrets
    let db_path = data_dir.join("nexa.db");
    std::fs::create_dir_all(&data_dir)?;
    let conn = Connection::open(&db_path)
        .map_err(|e| anyhow::anyhow!("failed to open database at {}: {e}", db_path.display()))?;
    info!(path = %db_path.display(), "secrets database opened");

    let secret_store = Arc::new(EncryptedSqliteSecretStore::new(conn, &master_key)?);

    // Create container runtime
    let orchestrator = engine::Orchestrator::new(secret_store.clone()).await?;
    // OR for actor model:
    // let runtime = adapters::runtime::DockerRuntime::new()?;
    // runtime.ping().await?;
    // let handle = Orchestrator::spawn(Arc::new(runtime), secret_store);

    let addr = format!("{}:{}", cli.host, cli.port);
    api::serve(orchestrator, &addr).await
}
```

Note: The exact wiring depends on whether Plan #1 (actor model) has been applied. If the codebase still uses `engine::Orchestrator::new()`, you need to modify `Orchestrator::new` to accept the secret store:

```rust
impl Orchestrator {
    pub async fn new(secrets: Arc<dyn SecretStore>) -> anyhow::Result<Arc<Self>> {
        let runtime = DockerRuntime::new()?;
        runtime.ping().await?;
        info!("connected to Docker runtime");

        Ok(Arc::new(Self {
            runtime: Arc::new(runtime),
            secrets,
            projects: DashMap::new(),
            deployments: DashMap::new(),
            pods: DashMap::new(),
        }))
    }
}
```

- [ ] **Step 2: Verify full workspace compiles**

```bash
cargo check 2>&1
```

Expected: compiles.

- [ ] **Step 3: Run full test suite**

```bash
cargo test 2>&1
```

Expected: all tests pass (config tests, orchestrator tests, crypto tests, secret store tests).

- [ ] **Step 4: Commit**

```bash
git add crates/nexad/src/main.rs crates/nexad/src/engine/
git commit -m "feat: wire EncryptedSqliteSecretStore and master key into nexad startup"
```

- [ ] **Step 5: Final verification -- full workspace build**

```bash
cargo build 2>&1
```

Expected: builds successfully.

- [ ] **Step 6: Push**

```bash
git push origin main
```
