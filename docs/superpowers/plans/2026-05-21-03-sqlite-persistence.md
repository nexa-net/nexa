# State Persistence (SQLite) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Multi-Repo Path Mapping:** This project uses separate repos. Translate paths as follows:
> | Plan path prefix | Repo | Local path |
> |---|---|---|
> | `crates/nexa-core/` | [`nexa-core`](https://github.com/nexa-net/nexa-core) | `/Users/nassime/GitHub/nexa-core/` |
> | `crates/nexad/` | [`nexad`](https://github.com/nexa-net/nexad) | `/Users/nassime/GitHub/nexad/` |
> | `crates/nexa-cli/` | [`nexa-cli`](https://github.com/nexa-net/nexa-cli) | `/Users/nassime/GitHub/nexa-cli/` |
> | `crates/nexa-proxy/` | [`nexa-proxy`](https://github.com/nexa-net/nexa-proxy) | `/Users/nassime/GitHub/nexa-proxy/` |
>
> `cargo check -p <crate>` → `cargo check` in the target repo. `nexa-core` dep: `git = "https://github.com/nexa-net/nexa-core"`

**Goal:** Persist all orchestrator state (projects, deployments, pods) to SQLite so nexad survives restarts without losing cluster state.

**Architecture:** A `StateStore` port trait is defined in `nexa-core/src/ports/state.rs`, keeping the domain pure. An `SqliteStore` adapter in `nexad/src/adapters/state/sqlite.rs` implements it using sqlx with compile-time-checked migrations. An `InMemoryStore` in `nexa-core` serves unit tests. The orchestrator receives `Arc<dyn StateStore>` at spawn time, writes to it after every mutation, and loads from it on startup. A reconciliation pass on startup detects stale pods by querying the container runtime.

**Tech Stack:** sqlx 0.8 (runtime-tokio, sqlite), chrono, uuid, serde_json, async-trait, tokio

---

### Task 1: Add sqlx workspace dependency and create migration files

**Files:**
- Modify: `Cargo.toml` (workspace)
- Modify: `crates/nexad/Cargo.toml`
- Create: `crates/nexad/migrations/20260521000001_initial_schema.sql`

- [ ] **Step 1: Add sqlx to workspace dependencies**

In `Cargo.toml` (workspace root), add to `[workspace.dependencies]`:

```toml
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
```

Full `[workspace.dependencies]` section becomes:

```toml
[workspace.dependencies]
nexa-core = { path = "crates/nexa-core" }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
anyhow = "1"
thiserror = "2"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
tonic = "0.12"
prost = "0.13"
bollard = "0.18"
clap = { version = "4", features = ["derive"] }
axum = "0.8"
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "trace"] }
reqwest = { version = "0.12", features = ["json"] }
async-trait = "0.1"
futures = "0.3"
dashmap = "6"
tokio-stream = "0.1"
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
```

- [ ] **Step 2: Add sqlx to nexad Cargo.toml**

In `crates/nexad/Cargo.toml`, add under `[dependencies]`:

```toml
sqlx = { workspace = true }
```

- [ ] **Step 3: Create migration directory and initial migration file**

```bash
mkdir -p crates/nexad/migrations
```

Create `crates/nexad/migrations/20260521000001_initial_schema.sql`:

```sql
-- Initial NexaNet state schema

CREATE TABLE IF NOT EXISTS projects (
    name        TEXT PRIMARY KEY,
    status      TEXT NOT NULL DEFAULT 'active',
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS deployments (
    id          TEXT PRIMARY KEY,
    project     TEXT NOT NULL REFERENCES projects(name),
    name        TEXT NOT NULL,
    spec_json   TEXT NOT NULL,
    status      TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    UNIQUE(project, name)
);

CREATE TABLE IF NOT EXISTS pods (
    id              TEXT PRIMARY KEY,
    deployment_id   TEXT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    project         TEXT NOT NULL,
    deployment_name TEXT NOT NULL,
    replica_index   INTEGER NOT NULL,
    container_id    TEXT,
    status          TEXT NOT NULL,
    image           TEXT NOT NULL,
    restart_count   INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS secrets (
    project     TEXT NOT NULL,
    name        TEXT NOT NULL,
    value_enc   BLOB NOT NULL,
    nonce       BLOB NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    PRIMARY KEY (project, name)
);
```

- [ ] **Step 4: Verify workspace resolves sqlx**

Run: `cargo check -p nexad 2>&1 | head -20`
Expected: compiles (sqlx is available but unused, warnings OK)

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/nexad/Cargo.toml crates/nexad/migrations/
git commit -m "build: add sqlx workspace dependency and initial SQLite migration"
```

---

### Task 2: Update domain models — add ProjectStatus and restart_count

**Files:**
- Modify: `crates/nexa-core/src/domain/models/project.rs`
- Modify: `crates/nexa-core/src/domain/models/pod.rs`

- [ ] **Step 1: Write failing test for ProjectStatus**

Add test to `crates/nexa-core/src/domain/models/project.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_project_has_active_status() {
        let p = Project::new("test");
        assert_eq!(p.status, ProjectStatus::Active);
    }

    #[test]
    fn project_status_serializes_lowercase() {
        let json = serde_json::to_string(&ProjectStatus::Suspended).unwrap();
        assert_eq!(json, "\"suspended\"");
    }

    #[test]
    fn project_status_roundtrips() {
        let active: ProjectStatus = serde_json::from_str("\"active\"").unwrap();
        assert_eq!(active, ProjectStatus::Active);
        let suspended: ProjectStatus = serde_json::from_str("\"suspended\"").unwrap();
        assert_eq!(suspended, ProjectStatus::Suspended);
    }
}
```

Run: `cargo test -p nexa-core -- domain::models::project 2>&1`
Expected: FAIL — `ProjectStatus` does not exist

- [ ] **Step 2: Implement ProjectStatus and update Project**

Replace `crates/nexa-core/src/domain/models/project.rs` with:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    Active,
    Suspended,
}

impl std::fmt::Display for ProjectStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectStatus::Active => write!(f, "active"),
            ProjectStatus::Suspended => write!(f, "suspended"),
        }
    }
}

impl std::str::FromStr for ProjectStatus {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "active" => Ok(ProjectStatus::Active),
            "suspended" => Ok(ProjectStatus::Suspended),
            other => Err(format!("unknown project status: {other}")),
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_project_has_active_status() {
        let p = Project::new("test");
        assert_eq!(p.status, ProjectStatus::Active);
    }

    #[test]
    fn project_status_serializes_lowercase() {
        let json = serde_json::to_string(&ProjectStatus::Suspended).unwrap();
        assert_eq!(json, "\"suspended\"");
    }

    #[test]
    fn project_status_roundtrips() {
        let active: ProjectStatus = serde_json::from_str("\"active\"").unwrap();
        assert_eq!(active, ProjectStatus::Active);
        let suspended: ProjectStatus = serde_json::from_str("\"suspended\"").unwrap();
        assert_eq!(suspended, ProjectStatus::Suspended);
    }
}
```

Run: `cargo test -p nexa-core -- domain::models::project 2>&1`
Expected: 3 tests pass

- [ ] **Step 3: Write failing test for Pod restart_count**

Add test to `crates/nexa-core/src/domain/models/pod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pod_has_zero_restart_count() {
        let pod = Pod::new(
            Uuid::new_v4(),
            "proj",
            "deploy",
            0,
            "nginx:latest",
        );
        assert_eq!(pod.restart_count, 0);
    }
}
```

Run: `cargo test -p nexa-core -- domain::models::pod 2>&1`
Expected: FAIL — `restart_count` field does not exist on Pod

- [ ] **Step 4: Add restart_count to Pod struct**

In `crates/nexa-core/src/domain/models/pod.rs`, add the field to the `Pod` struct and update `Pod::new()`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pod {
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub project: String,
    pub deployment_name: String,
    pub replica_index: u32,
    pub container_id: Option<String>,
    pub status: PodStatus,
    pub image: String,
    pub restart_count: u32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PodStatus {
    Pending,
    Creating,
    Running,
    Stopping,
    Stopped,
    Failed,
    Restarting,
}

impl Pod {
    pub fn new(
        deployment_id: Uuid,
        project: &str,
        deployment_name: &str,
        replica_index: u32,
        image: &str,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            deployment_id,
            project: project.to_string(),
            deployment_name: deployment_name.to_string(),
            replica_index,
            container_id: None,
            status: PodStatus::Pending,
            image: image.to_string(),
            restart_count: 0,
            created_at: Utc::now(),
        }
    }

    pub fn container_name(&self) -> String {
        format!(
            "nexa-{}-{}-{}",
            self.project, self.deployment_name, self.replica_index
        )
    }
}

impl std::fmt::Display for PodStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PodStatus::Pending => write!(f, "Pending"),
            PodStatus::Creating => write!(f, "Creating"),
            PodStatus::Running => write!(f, "Running"),
            PodStatus::Stopping => write!(f, "Stopping"),
            PodStatus::Stopped => write!(f, "Stopped"),
            PodStatus::Failed => write!(f, "Failed"),
            PodStatus::Restarting => write!(f, "Restarting"),
        }
    }
}

impl std::str::FromStr for PodStatus {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(PodStatus::Pending),
            "creating" => Ok(PodStatus::Creating),
            "running" => Ok(PodStatus::Running),
            "stopping" => Ok(PodStatus::Stopping),
            "stopped" => Ok(PodStatus::Stopped),
            "failed" => Ok(PodStatus::Failed),
            "restarting" => Ok(PodStatus::Restarting),
            other => Err(format!("unknown pod status: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pod_has_zero_restart_count() {
        let pod = Pod::new(
            Uuid::new_v4(),
            "proj",
            "deploy",
            0,
            "nginx:latest",
        );
        assert_eq!(pod.restart_count, 0);
    }
}
```

Run: `cargo test -p nexa-core -- domain::models::pod 2>&1`
Expected: 1 test passes

- [ ] **Step 5: Also add FromStr for DeploymentStatus**

In `crates/nexa-core/src/domain/models/deployment.rs`, add after the `DeploymentStatus` enum:

```rust
impl std::fmt::Display for DeploymentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentStatus::Pending => write!(f, "pending"),
            DeploymentStatus::Running => write!(f, "running"),
            DeploymentStatus::Degraded => write!(f, "degraded"),
            DeploymentStatus::Stopped => write!(f, "stopped"),
            DeploymentStatus::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for DeploymentStatus {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "pending" => Ok(DeploymentStatus::Pending),
            "running" => Ok(DeploymentStatus::Running),
            "degraded" => Ok(DeploymentStatus::Degraded),
            "stopped" => Ok(DeploymentStatus::Stopped),
            "failed" => Ok(DeploymentStatus::Failed),
            other => Err(format!("unknown deployment status: {other}")),
        }
    }
}
```

- [ ] **Step 6: Verify full nexa-core compiles and tests pass**

Run: `cargo test -p nexa-core 2>&1`
Expected: all tests pass (model tests + orchestrator tests + config tests)

- [ ] **Step 7: Commit**

```bash
git add crates/nexa-core/src/domain/models/
git commit -m "feat: add ProjectStatus, Pod restart_count, and Display/FromStr for status enums"
```

---

### Task 3: Define StateStore port trait

**Files:**
- Create: `crates/nexa-core/src/ports/state.rs`
- Modify: `crates/nexa-core/src/ports/mod.rs`

- [ ] **Step 1: Create the StateStore trait**

Create `crates/nexa-core/src/ports/state.rs`:

```rust
use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::models::{
    Deployment, DeploymentStatus, Pod, Project, ProjectStatus,
};
use crate::error::Result;

/// Port for persisting orchestrator state.
///
/// Implementations must be safe to share across threads (Send + Sync).
/// All methods return `Result<T>` using the crate-level error type.
#[async_trait]
pub trait StateStore: Send + Sync {
    // ── Projects ──────────────────────────────────────────────

    /// Insert a new project. Errors if the project name already exists.
    async fn insert_project(&self, project: &Project) -> Result<()>;

    /// Retrieve a project by name.
    async fn get_project(&self, name: &str) -> Result<Option<Project>>;

    /// List all projects.
    async fn list_projects(&self) -> Result<Vec<Project>>;

    /// Update a project's status (e.g. Active -> Suspended).
    async fn update_project_status(&self, name: &str, status: ProjectStatus) -> Result<()>;

    /// Delete a project by name. Does NOT cascade — caller must remove
    /// deployments/pods first.
    async fn delete_project(&self, name: &str) -> Result<()>;

    // ── Deployments ───────────────────────────────────────────

    /// Insert a new deployment.
    async fn insert_deployment(&self, deployment: &Deployment) -> Result<()>;

    /// Get a deployment by project + name.
    async fn get_deployment(&self, project: &str, name: &str) -> Result<Option<Deployment>>;

    /// List deployments, optionally filtered by project.
    async fn list_deployments(&self, project: Option<&str>) -> Result<Vec<Deployment>>;

    /// Update a full deployment (spec, status, updated_at).
    async fn update_deployment(&self, deployment: &Deployment) -> Result<()>;

    /// Delete a deployment by id.
    async fn delete_deployment(&self, id: &Uuid) -> Result<()>;

    // ── Pods ──────────────────────────────────────────────────

    /// Insert a new pod.
    async fn insert_pod(&self, pod: &Pod) -> Result<()>;

    /// List pods, optionally filtered by project.
    async fn list_pods(&self, project: Option<&str>) -> Result<Vec<Pod>>;

    /// Update a full pod record (status, container_id, restart_count).
    async fn update_pod(&self, pod: &Pod) -> Result<()>;

    /// Delete a pod by id.
    async fn delete_pod(&self, id: &Uuid) -> Result<()>;

    /// List all pods belonging to a specific deployment.
    async fn pods_by_deployment(&self, deployment_id: &Uuid) -> Result<Vec<Pod>>;
}
```

- [ ] **Step 2: Register the module in ports/mod.rs**

Update `crates/nexa-core/src/ports/mod.rs` (which currently contains `pub mod runtime;`):

```rust
pub mod runtime;
pub mod state;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p nexa-core 2>&1`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add crates/nexa-core/src/ports/
git commit -m "feat: define StateStore port trait for persistence"
```

---

### Task 4: Implement InMemoryStore (for tests)

**Files:**
- Create: `crates/nexa-core/src/ports/state_memory.rs`
- Modify: `crates/nexa-core/src/ports/mod.rs`

- [ ] **Step 1: Write failing test for InMemoryStore**

Create `crates/nexa-core/src/ports/state_memory.rs` with tests first:

```rust
use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::models::*;
use crate::error::{NexaError, Result};
use super::state::StateStore;

/// In-memory state store for tests. All data lives in `Mutex<HashMap>`s.
pub struct InMemoryStore {
    projects: Mutex<HashMap<String, Project>>,
    deployments: Mutex<HashMap<Uuid, Deployment>>,
    pods: Mutex<HashMap<Uuid, Pod>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            projects: Mutex::new(HashMap::new()),
            deployments: Mutex::new(HashMap::new()),
            pods: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl StateStore for InMemoryStore {
    // ── Projects ──────────────────────────────────────────────

    async fn insert_project(&self, project: &Project) -> Result<()> {
        let mut map = self.projects.lock().unwrap();
        if map.contains_key(&project.name) {
            return Err(NexaError::InvalidSpec(format!(
                "project '{}' already exists",
                project.name
            )));
        }
        map.insert(project.name.clone(), project.clone());
        Ok(())
    }

    async fn get_project(&self, name: &str) -> Result<Option<Project>> {
        let map = self.projects.lock().unwrap();
        Ok(map.get(name).cloned())
    }

    async fn list_projects(&self) -> Result<Vec<Project>> {
        let map = self.projects.lock().unwrap();
        Ok(map.values().cloned().collect())
    }

    async fn update_project_status(&self, name: &str, status: ProjectStatus) -> Result<()> {
        let mut map = self.projects.lock().unwrap();
        match map.get_mut(name) {
            Some(p) => {
                p.status = status;
                Ok(())
            }
            None => Err(NexaError::ProjectNotFound(name.to_string())),
        }
    }

    async fn delete_project(&self, name: &str) -> Result<()> {
        let mut map = self.projects.lock().unwrap();
        map.remove(name);
        Ok(())
    }

    // ── Deployments ───────────────────────────────────────────

    async fn insert_deployment(&self, deployment: &Deployment) -> Result<()> {
        let mut map = self.deployments.lock().unwrap();
        map.insert(deployment.id, deployment.clone());
        Ok(())
    }

    async fn get_deployment(&self, project: &str, name: &str) -> Result<Option<Deployment>> {
        let map = self.deployments.lock().unwrap();
        let found = map
            .values()
            .find(|d| d.project() == project && d.name() == name)
            .cloned();
        Ok(found)
    }

    async fn list_deployments(&self, project: Option<&str>) -> Result<Vec<Deployment>> {
        let map = self.deployments.lock().unwrap();
        let result = map
            .values()
            .filter(|d| match project {
                Some(p) => d.project() == p,
                None => true,
            })
            .cloned()
            .collect();
        Ok(result)
    }

    async fn update_deployment(&self, deployment: &Deployment) -> Result<()> {
        let mut map = self.deployments.lock().unwrap();
        map.insert(deployment.id, deployment.clone());
        Ok(())
    }

    async fn delete_deployment(&self, id: &Uuid) -> Result<()> {
        let mut map = self.deployments.lock().unwrap();
        map.remove(id);
        Ok(())
    }

    // ── Pods ──────────────────────────────────────────────────

    async fn insert_pod(&self, pod: &Pod) -> Result<()> {
        let mut map = self.pods.lock().unwrap();
        map.insert(pod.id, pod.clone());
        Ok(())
    }

    async fn list_pods(&self, project: Option<&str>) -> Result<Vec<Pod>> {
        let map = self.pods.lock().unwrap();
        let result = map
            .values()
            .filter(|p| match project {
                Some(proj) => p.project == proj,
                None => true,
            })
            .cloned()
            .collect();
        Ok(result)
    }

    async fn update_pod(&self, pod: &Pod) -> Result<()> {
        let mut map = self.pods.lock().unwrap();
        map.insert(pod.id, pod.clone());
        Ok(())
    }

    async fn delete_pod(&self, id: &Uuid) -> Result<()> {
        let mut map = self.pods.lock().unwrap();
        map.remove(id);
        Ok(())
    }

    async fn pods_by_deployment(&self, deployment_id: &Uuid) -> Result<Vec<Pod>> {
        let map = self.pods.lock().unwrap();
        let result = map
            .values()
            .filter(|p| p.deployment_id == *deployment_id)
            .cloned()
            .collect();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn insert_and_get_project() {
        let store = InMemoryStore::new();
        let project = Project::new("myapp");

        store.insert_project(&project).await.unwrap();
        let fetched = store.get_project("myapp").await.unwrap();

        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().name, "myapp");
    }

    #[tokio::test]
    async fn duplicate_project_errors() {
        let store = InMemoryStore::new();
        let project = Project::new("myapp");

        store.insert_project(&project).await.unwrap();
        let result = store.insert_project(&project).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn update_project_status() {
        let store = InMemoryStore::new();
        let project = Project::new("myapp");
        store.insert_project(&project).await.unwrap();

        store
            .update_project_status("myapp", ProjectStatus::Suspended)
            .await
            .unwrap();

        let fetched = store.get_project("myapp").await.unwrap().unwrap();
        assert_eq!(fetched.status, ProjectStatus::Suspended);
    }

    #[tokio::test]
    async fn insert_and_list_deployments() {
        let store = InMemoryStore::new();
        let spec = DeploymentSpec {
            project: "myapp".into(),
            deployment: DeploymentMeta { name: "api".into() },
            replicas: 2,
            image: "nginx:latest".into(),
            ports: vec![],
            env: std::collections::HashMap::new(),
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
        };
        let deployment = Deployment::from_spec(spec);

        store.insert_deployment(&deployment).await.unwrap();

        let all = store.list_deployments(None).await.unwrap();
        assert_eq!(all.len(), 1);

        let filtered = store.list_deployments(Some("myapp")).await.unwrap();
        assert_eq!(filtered.len(), 1);

        let empty = store.list_deployments(Some("other")).await.unwrap();
        assert_eq!(empty.len(), 0);
    }

    #[tokio::test]
    async fn insert_and_query_pods() {
        let store = InMemoryStore::new();
        let deployment_id = Uuid::new_v4();

        let pod = Pod::new(deployment_id, "myapp", "api", 0, "nginx:latest");
        store.insert_pod(&pod).await.unwrap();

        let by_deployment = store.pods_by_deployment(&deployment_id).await.unwrap();
        assert_eq!(by_deployment.len(), 1);
        assert_eq!(by_deployment[0].restart_count, 0);

        let by_project = store.list_pods(Some("myapp")).await.unwrap();
        assert_eq!(by_project.len(), 1);
    }

    #[tokio::test]
    async fn delete_pod() {
        let store = InMemoryStore::new();
        let pod = Pod::new(Uuid::new_v4(), "myapp", "api", 0, "nginx:latest");
        let pod_id = pod.id;

        store.insert_pod(&pod).await.unwrap();
        store.delete_pod(&pod_id).await.unwrap();

        let all = store.list_pods(None).await.unwrap();
        assert_eq!(all.len(), 0);
    }
}
```

- [ ] **Step 2: Register the module**

Update `crates/nexa-core/src/ports/mod.rs`:

```rust
pub mod runtime;
pub mod state;
pub mod state_memory;
```

- [ ] **Step 3: Run InMemoryStore tests**

Run: `cargo test -p nexa-core -- ports::state_memory 2>&1`
Expected: all 6 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/nexa-core/src/ports/
git commit -m "feat: implement InMemoryStore for test-time StateStore"
```

---

### Task 5: Implement SqliteStore adapter

**Files:**
- Create: `crates/nexad/src/adapters/state/mod.rs`
- Create: `crates/nexad/src/adapters/state/sqlite.rs`
- Modify: `crates/nexad/src/adapters/mod.rs`

- [ ] **Step 1: Write failing integration test (inline in sqlite.rs)**

Create directory structure:

```bash
mkdir -p crates/nexad/src/adapters/state
```

Create `crates/nexad/src/adapters/state/sqlite.rs`:

```rust
use async_trait::async_trait;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use uuid::Uuid;

use nexa_core::domain::models::*;
use nexa_core::error::{NexaError, Result};
use nexa_core::ports::state::StateStore;

pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// Connect to the SQLite database and run all pending migrations.
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await?;

        // Enable WAL mode for better concurrency
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await?;

        // Enable foreign keys
        sqlx::query("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await?;

        Ok(Self { pool })
    }

    /// Expose pool for testing purposes.
    #[cfg(test)]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[async_trait]
impl StateStore for SqliteStore {
    // ── Projects ──────────────────────────────────────────────

    async fn insert_project(&self, project: &Project) -> Result<()> {
        sqlx::query(
            "INSERT INTO projects (name, status, created_at) VALUES (?, ?, ?)"
        )
        .bind(&project.name)
        .bind(project.status.to_string())
        .bind(project.created_at.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| NexaError::InvalidSpec(format!("insert project failed: {e}")))?;
        Ok(())
    }

    async fn get_project(&self, name: &str) -> Result<Option<Project>> {
        let row = sqlx::query(
            "SELECT name, status, created_at FROM projects WHERE name = ?"
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| NexaError::Runtime(format!("get project failed: {e}")))?;

        match row {
            Some(row) => {
                let status_str: String = row.get("status");
                let created_str: String = row.get("created_at");
                Ok(Some(Project {
                    name: row.get("name"),
                    status: status_str.parse::<ProjectStatus>()
                        .map_err(|e| NexaError::Runtime(e))?,
                    created_at: chrono::DateTime::parse_from_rfc3339(&created_str)
                        .map_err(|e| NexaError::Runtime(e.to_string()))?
                        .with_timezone(&chrono::Utc),
                }))
            }
            None => Ok(None),
        }
    }

    async fn list_projects(&self) -> Result<Vec<Project>> {
        let rows = sqlx::query("SELECT name, status, created_at FROM projects")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| NexaError::Runtime(format!("list projects failed: {e}")))?;

        let mut projects = Vec::with_capacity(rows.len());
        for row in rows {
            let status_str: String = row.get("status");
            let created_str: String = row.get("created_at");
            projects.push(Project {
                name: row.get("name"),
                status: status_str.parse::<ProjectStatus>()
                    .map_err(|e| NexaError::Runtime(e))?,
                created_at: chrono::DateTime::parse_from_rfc3339(&created_str)
                    .map_err(|e| NexaError::Runtime(e.to_string()))?
                    .with_timezone(&chrono::Utc),
            });
        }
        Ok(projects)
    }

    async fn update_project_status(&self, name: &str, status: ProjectStatus) -> Result<()> {
        let result = sqlx::query("UPDATE projects SET status = ? WHERE name = ?")
            .bind(status.to_string())
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| NexaError::Runtime(format!("update project status failed: {e}")))?;

        if result.rows_affected() == 0 {
            return Err(NexaError::ProjectNotFound(name.to_string()));
        }
        Ok(())
    }

    async fn delete_project(&self, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM projects WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| NexaError::Runtime(format!("delete project failed: {e}")))?;
        Ok(())
    }

    // ── Deployments ───────────────────────────────────────────

    async fn insert_deployment(&self, deployment: &Deployment) -> Result<()> {
        let spec_json = serde_json::to_string(&deployment.spec)
            .map_err(|e| NexaError::Serialization(e))?;

        sqlx::query(
            "INSERT INTO deployments (id, project, name, spec_json, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(deployment.id.to_string())
        .bind(deployment.project())
        .bind(deployment.name())
        .bind(&spec_json)
        .bind(deployment.status.to_string())
        .bind(deployment.created_at.to_rfc3339())
        .bind(deployment.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| NexaError::Runtime(format!("insert deployment failed: {e}")))?;
        Ok(())
    }

    async fn get_deployment(&self, project: &str, name: &str) -> Result<Option<Deployment>> {
        let row = sqlx::query(
            "SELECT id, project, name, spec_json, status, created_at, updated_at
             FROM deployments WHERE project = ? AND name = ?"
        )
        .bind(project)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| NexaError::Runtime(format!("get deployment failed: {e}")))?;

        match row {
            Some(row) => Ok(Some(Self::row_to_deployment(&row)?)),
            None => Ok(None),
        }
    }

    async fn list_deployments(&self, project: Option<&str>) -> Result<Vec<Deployment>> {
        let rows = match project {
            Some(p) => {
                sqlx::query(
                    "SELECT id, project, name, spec_json, status, created_at, updated_at
                     FROM deployments WHERE project = ?"
                )
                .bind(p)
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query(
                    "SELECT id, project, name, spec_json, status, created_at, updated_at
                     FROM deployments"
                )
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(|e| NexaError::Runtime(format!("list deployments failed: {e}")))?;

        let mut deployments = Vec::with_capacity(rows.len());
        for row in &rows {
            deployments.push(Self::row_to_deployment(row)?);
        }
        Ok(deployments)
    }

    async fn update_deployment(&self, deployment: &Deployment) -> Result<()> {
        let spec_json = serde_json::to_string(&deployment.spec)
            .map_err(|e| NexaError::Serialization(e))?;

        sqlx::query(
            "UPDATE deployments SET spec_json = ?, status = ?, updated_at = ? WHERE id = ?"
        )
        .bind(&spec_json)
        .bind(deployment.status.to_string())
        .bind(deployment.updated_at.to_rfc3339())
        .bind(deployment.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| NexaError::Runtime(format!("update deployment failed: {e}")))?;
        Ok(())
    }

    async fn delete_deployment(&self, id: &Uuid) -> Result<()> {
        sqlx::query("DELETE FROM deployments WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| NexaError::Runtime(format!("delete deployment failed: {e}")))?;
        Ok(())
    }

    // ── Pods ──────────────────────────────────────────────────

    async fn insert_pod(&self, pod: &Pod) -> Result<()> {
        sqlx::query(
            "INSERT INTO pods (id, deployment_id, project, deployment_name, replica_index,
             container_id, status, image, restart_count, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(pod.id.to_string())
        .bind(pod.deployment_id.to_string())
        .bind(&pod.project)
        .bind(&pod.deployment_name)
        .bind(pod.replica_index as i64)
        .bind(pod.container_id.as_deref())
        .bind(pod.status.to_string())
        .bind(&pod.image)
        .bind(pod.restart_count as i64)
        .bind(pod.created_at.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| NexaError::Runtime(format!("insert pod failed: {e}")))?;
        Ok(())
    }

    async fn list_pods(&self, project: Option<&str>) -> Result<Vec<Pod>> {
        let rows = match project {
            Some(p) => {
                sqlx::query(
                    "SELECT id, deployment_id, project, deployment_name, replica_index,
                     container_id, status, image, restart_count, created_at
                     FROM pods WHERE project = ?"
                )
                .bind(p)
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query(
                    "SELECT id, deployment_id, project, deployment_name, replica_index,
                     container_id, status, image, restart_count, created_at
                     FROM pods"
                )
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(|e| NexaError::Runtime(format!("list pods failed: {e}")))?;

        let mut pods = Vec::with_capacity(rows.len());
        for row in &rows {
            pods.push(Self::row_to_pod(row)?);
        }
        Ok(pods)
    }

    async fn update_pod(&self, pod: &Pod) -> Result<()> {
        sqlx::query(
            "UPDATE pods SET container_id = ?, status = ?, restart_count = ? WHERE id = ?"
        )
        .bind(pod.container_id.as_deref())
        .bind(pod.status.to_string())
        .bind(pod.restart_count as i64)
        .bind(pod.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| NexaError::Runtime(format!("update pod failed: {e}")))?;
        Ok(())
    }

    async fn delete_pod(&self, id: &Uuid) -> Result<()> {
        sqlx::query("DELETE FROM pods WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| NexaError::Runtime(format!("delete pod failed: {e}")))?;
        Ok(())
    }

    async fn pods_by_deployment(&self, deployment_id: &Uuid) -> Result<Vec<Pod>> {
        let rows = sqlx::query(
            "SELECT id, deployment_id, project, deployment_name, replica_index,
             container_id, status, image, restart_count, created_at
             FROM pods WHERE deployment_id = ?"
        )
        .bind(deployment_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| NexaError::Runtime(format!("pods by deployment failed: {e}")))?;

        let mut pods = Vec::with_capacity(rows.len());
        for row in &rows {
            pods.push(Self::row_to_pod(row)?);
        }
        Ok(pods)
    }
}

// ── Row mapping helpers ──────────────────────────────────────────

impl SqliteStore {
    fn row_to_deployment(row: &sqlx::sqlite::SqliteRow) -> Result<Deployment> {
        let id_str: String = row.get("id");
        let spec_json: String = row.get("spec_json");
        let status_str: String = row.get("status");
        let created_str: String = row.get("created_at");
        let updated_str: String = row.get("updated_at");

        let id = Uuid::parse_str(&id_str)
            .map_err(|e| NexaError::Runtime(format!("invalid deployment UUID: {e}")))?;
        let spec: DeploymentSpec = serde_json::from_str(&spec_json)?;
        let status: DeploymentStatus = status_str
            .parse()
            .map_err(|e: String| NexaError::Runtime(e))?;
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
            .map_err(|e| NexaError::Runtime(e.to_string()))?
            .with_timezone(&chrono::Utc);
        let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_str)
            .map_err(|e| NexaError::Runtime(e.to_string()))?
            .with_timezone(&chrono::Utc);

        Ok(Deployment {
            id,
            spec,
            status,
            created_at,
            updated_at,
        })
    }

    fn row_to_pod(row: &sqlx::sqlite::SqliteRow) -> Result<Pod> {
        let id_str: String = row.get("id");
        let deployment_id_str: String = row.get("deployment_id");
        let status_str: String = row.get("status");
        let created_str: String = row.get("created_at");
        let restart_count: i64 = row.get("restart_count");
        let replica_index: i64 = row.get("replica_index");

        let id = Uuid::parse_str(&id_str)
            .map_err(|e| NexaError::Runtime(format!("invalid pod UUID: {e}")))?;
        let deployment_id = Uuid::parse_str(&deployment_id_str)
            .map_err(|e| NexaError::Runtime(format!("invalid deployment UUID: {e}")))?;
        let status: PodStatus = status_str
            .parse()
            .map_err(|e: String| NexaError::Runtime(e))?;
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
            .map_err(|e| NexaError::Runtime(e.to_string()))?
            .with_timezone(&chrono::Utc);

        Ok(Pod {
            id,
            deployment_id,
            project: row.get("project"),
            deployment_name: row.get("deployment_name"),
            replica_index: replica_index as u32,
            container_id: row.get("container_id"),
            status,
            image: row.get("image"),
            restart_count: restart_count as u32,
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    async fn setup_store() -> SqliteStore {
        SqliteStore::connect("sqlite::memory:").await.unwrap()
    }

    #[tokio::test]
    async fn project_roundtrip() {
        let store = setup_store().await;
        let project = Project::new("test-proj");

        store.insert_project(&project).await.unwrap();
        let fetched = store.get_project("test-proj").await.unwrap().unwrap();

        assert_eq!(fetched.name, "test-proj");
        assert_eq!(fetched.status, ProjectStatus::Active);
    }

    #[tokio::test]
    async fn duplicate_project_fails() {
        let store = setup_store().await;
        let project = Project::new("dup");

        store.insert_project(&project).await.unwrap();
        let result = store.insert_project(&project).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn project_status_update() {
        let store = setup_store().await;
        store.insert_project(&Project::new("myapp")).await.unwrap();

        store
            .update_project_status("myapp", ProjectStatus::Suspended)
            .await
            .unwrap();

        let p = store.get_project("myapp").await.unwrap().unwrap();
        assert_eq!(p.status, ProjectStatus::Suspended);
    }

    #[tokio::test]
    async fn deployment_roundtrip() {
        let store = setup_store().await;
        store.insert_project(&Project::new("myapp")).await.unwrap();

        let spec = DeploymentSpec {
            project: "myapp".into(),
            deployment: DeploymentMeta { name: "api".into() },
            replicas: 3,
            image: "nginx:latest".into(),
            ports: vec![8080],
            env: HashMap::from([("KEY".into(), "VAL".into())]),
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
        };
        let deployment = Deployment::from_spec(spec);

        store.insert_deployment(&deployment).await.unwrap();
        let fetched = store.get_deployment("myapp", "api").await.unwrap().unwrap();

        assert_eq!(fetched.id, deployment.id);
        assert_eq!(fetched.spec.replicas, 3);
        assert_eq!(fetched.spec.ports, vec![8080]);
        assert_eq!(fetched.spec.env.get("KEY").unwrap(), "VAL");
        assert_eq!(fetched.status, DeploymentStatus::Pending);
    }

    #[tokio::test]
    async fn deployment_update() {
        let store = setup_store().await;
        store.insert_project(&Project::new("myapp")).await.unwrap();

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
        };
        let mut deployment = Deployment::from_spec(spec);
        store.insert_deployment(&deployment).await.unwrap();

        deployment.status = DeploymentStatus::Running;
        deployment.updated_at = chrono::Utc::now();
        store.update_deployment(&deployment).await.unwrap();

        let fetched = store.get_deployment("myapp", "api").await.unwrap().unwrap();
        assert_eq!(fetched.status, DeploymentStatus::Running);
    }

    #[tokio::test]
    async fn pod_roundtrip() {
        let store = setup_store().await;
        store.insert_project(&Project::new("myapp")).await.unwrap();

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
        };
        let deployment = Deployment::from_spec(spec);
        store.insert_deployment(&deployment).await.unwrap();

        let mut pod = Pod::new(deployment.id, "myapp", "api", 0, "nginx:latest");
        pod.container_id = Some("abc123".into());
        pod.status = PodStatus::Running;
        pod.restart_count = 2;

        store.insert_pod(&pod).await.unwrap();

        let pods = store.pods_by_deployment(&deployment.id).await.unwrap();
        assert_eq!(pods.len(), 1);
        assert_eq!(pods[0].container_id.as_deref(), Some("abc123"));
        assert_eq!(pods[0].status, PodStatus::Running);
        assert_eq!(pods[0].restart_count, 2);
        assert_eq!(pods[0].image, "nginx:latest");
    }

    #[tokio::test]
    async fn delete_deployment_cascades_pods() {
        let store = setup_store().await;
        store.insert_project(&Project::new("myapp")).await.unwrap();

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
        };
        let deployment = Deployment::from_spec(spec);
        store.insert_deployment(&deployment).await.unwrap();

        let pod = Pod::new(deployment.id, "myapp", "api", 0, "nginx");
        store.insert_pod(&pod).await.unwrap();

        store.delete_deployment(&deployment.id).await.unwrap();

        let pods = store.list_pods(None).await.unwrap();
        assert_eq!(pods.len(), 0, "pods should be cascade-deleted");
    }

    #[tokio::test]
    async fn list_pods_filters_by_project() {
        let store = setup_store().await;
        store.insert_project(&Project::new("a")).await.unwrap();
        store.insert_project(&Project::new("b")).await.unwrap();

        let spec_a = DeploymentSpec {
            project: "a".into(),
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
        let dep_a = Deployment::from_spec(spec_a);
        store.insert_deployment(&dep_a).await.unwrap();

        let spec_b = DeploymentSpec {
            project: "b".into(),
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
        let dep_b = Deployment::from_spec(spec_b);
        store.insert_deployment(&dep_b).await.unwrap();

        store.insert_pod(&Pod::new(dep_a.id, "a", "web", 0, "nginx")).await.unwrap();
        store.insert_pod(&Pod::new(dep_b.id, "b", "web", 0, "nginx")).await.unwrap();

        let all = store.list_pods(None).await.unwrap();
        assert_eq!(all.len(), 2);

        let proj_a = store.list_pods(Some("a")).await.unwrap();
        assert_eq!(proj_a.len(), 1);
        assert_eq!(proj_a[0].project, "a");
    }
}
```

- [ ] **Step 2: Create mod.rs files and register the adapter**

Create `crates/nexad/src/adapters/state/mod.rs`:

```rust
mod sqlite;

pub use sqlite::SqliteStore;
```

Update `crates/nexad/src/adapters/mod.rs` (currently has `pub mod runtime;`):

```rust
pub mod runtime;
pub mod state;
```

- [ ] **Step 3: Run SqliteStore tests**

Run: `cargo test -p nexad -- adapters::state::sqlite 2>&1`
Expected: all 8 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/nexad/src/adapters/state/ crates/nexad/src/adapters/mod.rs
git commit -m "feat: implement SqliteStore adapter with migration and full CRUD"
```

---

### Task 6: Update Orchestrator to accept and use StateStore

**Files:**
- Modify: `crates/nexa-core/src/domain/orchestrator.rs`

This is the largest task. The Orchestrator's `spawn()` method gains an `Arc<dyn StateStore>` parameter. Every mutation writes through to the state store after updating in-memory state. A new `load_state()` method hydrates in-memory state from the store on startup.

- [ ] **Step 1: Write failing test for state-store-backed orchestrator**

Add to the `tests` module in `crates/nexa-core/src/domain/orchestrator.rs`:

```rust
use crate::ports::state_memory::InMemoryStore;
use crate::ports::state::StateStore;

fn spawn_persisted_test_orchestrator() -> (OrchestratorHandle, Arc<InMemoryStore>) {
    let store = Arc::new(InMemoryStore::new());
    let handle = Orchestrator::spawn(Arc::new(MockRuntime), Some(store.clone() as Arc<dyn StateStore>));
    (handle, store)
}

#[tokio::test]
async fn deploy_persists_to_state_store() {
    let (handle, store) = spawn_persisted_test_orchestrator();

    let spec = DeploymentSpec {
        project: "test".into(),
        deployment: DeploymentMeta { name: "api".into() },
        replicas: 2,
        image: "nginx:latest".into(),
        ports: vec![],
        env: HashMap::new(),
        volumes: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::default(),
    };

    handle.deploy(spec).await.unwrap();

    // Verify state was written through to the store
    let projects = store.list_projects().await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "test");

    let deployments = store.list_deployments(None).await.unwrap();
    assert_eq!(deployments.len(), 1);
    assert_eq!(deployments[0].name(), "api");

    let pods = store.list_pods(None).await.unwrap();
    assert_eq!(pods.len(), 2);
}

#[tokio::test]
async fn stop_persists_to_state_store() {
    let (handle, store) = spawn_persisted_test_orchestrator();

    let spec = DeploymentSpec {
        project: "test".into(),
        deployment: DeploymentMeta { name: "api".into() },
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
    handle.stop("test".into(), "api".into()).await.unwrap();

    let pods = store.list_pods(None).await.unwrap();
    assert_eq!(pods.len(), 0, "pods should be deleted from store on stop");

    let deployments = store.list_deployments(None).await.unwrap();
    assert_eq!(deployments[0].status, DeploymentStatus::Stopped);
}

#[tokio::test]
async fn scale_persists_to_state_store() {
    let (handle, store) = spawn_persisted_test_orchestrator();

    let spec = DeploymentSpec {
        project: "test".into(),
        deployment: DeploymentMeta { name: "api".into() },
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
    handle.scale("test".into(), "api".into(), 3).await.unwrap();

    let pods = store.list_pods(None).await.unwrap();
    assert_eq!(pods.len(), 3);
}
```

Run: `cargo test -p nexa-core -- domain::orchestrator 2>&1`
Expected: FAIL — `Orchestrator::spawn` does not accept a state store parameter

- [ ] **Step 2: Update Orchestrator::spawn to accept Optional StateStore**

Modify `Orchestrator::spawn` signature and struct:

```rust
pub struct Orchestrator {
    runtime: Arc<dyn ContainerRuntime>,
    state_store: Option<Arc<dyn StateStore>>,
    projects: StdHashMap<String, Project>,
    deployments: StdHashMap<Uuid, Deployment>,
    pods: StdHashMap<Uuid, Pod>,
}

impl Orchestrator {
    /// Spawn the orchestrator actor. If `state_store` is provided,
    /// all mutations are persisted and state is loaded on startup.
    pub fn spawn(
        runtime: Arc<dyn ContainerRuntime>,
        state_store: Option<Arc<dyn StateStore>>,
    ) -> OrchestratorHandle {
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let mut orch = Self {
                runtime,
                state_store,
                projects: StdHashMap::new(),
                deployments: StdHashMap::new(),
                pods: StdHashMap::new(),
            };
            orch.run(rx).await;
        });
        OrchestratorHandle { tx }
    }
```

Also add the `use` import at the top of the file:

```rust
use crate::ports::state::StateStore;
```

- [ ] **Step 3: Update existing test helper to pass None for state_store**

Update the existing `spawn_test_orchestrator` helper:

```rust
fn spawn_test_orchestrator() -> OrchestratorHandle {
    Orchestrator::spawn(Arc::new(MockRuntime), None)
}
```

This ensures all existing tests still pass without a state store.

- [ ] **Step 4: Add persistence calls to ensure_project**

```rust
fn ensure_project(&mut self, name: &str) {
    if !self.projects.contains_key(name) {
        let project = Project::new(name);
        self.projects.insert(name.to_string(), project.clone());
        if let Some(store) = &self.state_store {
            let store = store.clone();
            let project = project.clone();
            // Fire and forget — we're in the single-threaded actor loop,
            // so we spawn a task and move on. Errors are logged.
            tokio::spawn(async move {
                if let Err(e) = store.insert_project(&project).await {
                    tracing::warn!(project = %project.name, error = %e, "failed to persist project");
                }
            });
        }
    }
}
```

**Wait** — this fire-and-forget pattern is wrong for our write-path guarantee. The spec says: `Mutate in-memory state -> Write to SQLite -> Perform container action -> Reply`. We need to await the store write. Since `ensure_project` is called from async contexts, convert it to async:

```rust
async fn ensure_project(&mut self, name: &str) {
    if !self.projects.contains_key(name) {
        let project = Project::new(name);
        self.projects.insert(name.to_string(), project.clone());
        self.persist_insert_project(&project).await;
    }
}

async fn persist_insert_project(&self, project: &Project) {
    if let Some(store) = &self.state_store {
        if let Err(e) = store.insert_project(project).await {
            tracing::warn!(project = %project.name, error = %e, "failed to persist project");
        }
    }
}
```

- [ ] **Step 5: Add persistence calls to handle_deploy**

After the in-memory mutation and before reconcile, persist the deployment:

```rust
async fn handle_deploy(&mut self, spec: DeploymentSpec) -> Result<Deployment> {
    self.ensure_project(&spec.project).await;

    let existing_id = self.find_deployment_id(&spec.project, &spec.deployment.name);

    if let Some(id) = existing_id {
        let deployment = self.deployments.get_mut(&id).unwrap();
        deployment.spec = spec.clone();
        deployment.updated_at = chrono::Utc::now();
        let cloned = deployment.clone();
        self.persist_update_deployment(&cloned).await;
        let id = cloned.id;
        self.reconcile_deployment(id).await?;
        return Ok(self.deployments[&id].clone());
    }

    let deployment = Deployment::from_spec(spec);
    let id = deployment.id;
    self.persist_insert_deployment(&deployment).await;
    self.deployments.insert(id, deployment);
    self.reconcile_deployment(id).await?;
    Ok(self.deployments[&id].clone())
}
```

- [ ] **Step 6: Add persistence helpers for deployments and pods**

```rust
async fn persist_insert_deployment(&self, deployment: &Deployment) {
    if let Some(store) = &self.state_store {
        if let Err(e) = store.insert_deployment(deployment).await {
            tracing::warn!(id = %deployment.id, error = %e, "failed to persist deployment insert");
        }
    }
}

async fn persist_update_deployment(&self, deployment: &Deployment) {
    if let Some(store) = &self.state_store {
        if let Err(e) = store.update_deployment(deployment).await {
            tracing::warn!(id = %deployment.id, error = %e, "failed to persist deployment update");
        }
    }
}

async fn persist_insert_pod(&self, pod: &Pod) {
    if let Some(store) = &self.state_store {
        if let Err(e) = store.insert_pod(pod).await {
            tracing::warn!(id = %pod.id, error = %e, "failed to persist pod insert");
        }
    }
}

async fn persist_update_pod(&self, pod: &Pod) {
    if let Some(store) = &self.state_store {
        if let Err(e) = store.update_pod(pod).await {
            tracing::warn!(id = %pod.id, error = %e, "failed to persist pod update");
        }
    }
}

async fn persist_delete_pod(&self, id: &Uuid) {
    if let Some(store) = &self.state_store {
        if let Err(e) = store.delete_pod(id).await {
            tracing::warn!(pod_id = %id, error = %e, "failed to persist pod delete");
        }
    }
}
```

- [ ] **Step 7: Add persistence to create_pod**

At the end of `create_pod`, after inserting into `self.pods`, add:

```rust
async fn create_pod(&mut self, deployment_id: Uuid, spec: &DeploymentSpec, index: u32) -> Result<()> {
    let mut pod = Pod::new(
        deployment_id,
        &spec.project,
        &spec.deployment.name,
        index,
        &spec.image,
    );

    let container_name = pod.container_name();
    let network_name = format!("nexa-{}", spec.project);

    pod.status = PodStatus::Creating;

    let _ = self.runtime.pull_image(&spec.image).await;

    if self.runtime.container_exists(&container_name).await? {
        let _ = self.runtime.stop_container(&container_name, 5).await;
        let _ = self.runtime.remove_container(&container_name, true).await;
    }

    let ports: Vec<crate::ports::runtime::PortBinding> = spec
        .ports
        .iter()
        .map(|&p| crate::ports::runtime::PortBinding {
            container_port: p,
            host_port: if spec.replicas == 1 { Some(p) } else { None },
        })
        .collect();

    let mut labels = StdHashMap::new();
    labels.insert("managed-by".to_string(), "nexanet".to_string());
    labels.insert("nexa.project".to_string(), spec.project.clone());
    labels.insert("nexa.deployment".to_string(), spec.deployment.name.clone());
    labels.insert("nexa.pod-id".to_string(), pod.id.to_string());

    let config = ContainerConfig {
        name: container_name,
        image: spec.image.clone(),
        env: spec.env.clone(),
        ports,
        volumes: spec
            .volumes
            .iter()
            .map(|v| crate::ports::runtime::VolumeBinding {
                source: v.name.clone(),
                target: v.mount_path.clone(),
                read_only: false,
            })
            .collect(),
        labels,
        network: Some(network_name),
    };

    match self.runtime.create_container(&config).await {
        Ok(container_id) => {
            self.runtime.start_container(&container_id).await?;
            pod.container_id = Some(container_id);
            pod.status = PodStatus::Running;
        }
        Err(_) => {
            pod.status = PodStatus::Failed;
        }
    }

    self.persist_insert_pod(&pod).await;
    self.pods.insert(pod.id, pod);
    Ok(())
}
```

- [ ] **Step 8: Add persistence to handle_stop**

```rust
async fn handle_stop(&mut self, project: &str, name: &str) -> Result<()> {
    let deployment_id = self
        .find_deployment_id(project, name)
        .ok_or_else(|| NexaError::DeploymentNotFound(format!("{project}/{name}")))?;

    let pod_ids: Vec<Uuid> = self
        .pods
        .values()
        .filter(|p| p.deployment_id == deployment_id)
        .map(|p| p.id)
        .collect();

    for pod_id in &pod_ids {
        if let Some(pod) = self.pods.get(pod_id) {
            if let Some(cid) = &pod.container_id {
                let _ = self.runtime.stop_container(cid, 10).await;
                let _ = self.runtime.remove_container(cid, true).await;
            }
        }
        self.persist_delete_pod(pod_id).await;
        self.pods.remove(pod_id);
    }

    if let Some(d) = self.deployments.get_mut(&deployment_id) {
        d.status = DeploymentStatus::Stopped;
        let cloned = d.clone();
        self.persist_update_deployment(&cloned).await;
    }

    Ok(())
}
```

- [ ] **Step 9: Add persistence to handle_scale**

In the reconcile path, pods are created/removed. The `reconcile_deployment` already calls `create_pod` (which now persists). For scale-down removal, update the reconcile method:

```rust
async fn reconcile_deployment(&mut self, deployment_id: Uuid) -> Result<()> {
    let spec = self.deployments[&deployment_id].spec.clone();
    let desired = spec.replicas;

    let network_name = format!("nexa-{}", spec.project);
    let _ = self.runtime.create_network(&network_name).await;

    let mut current_pods: Vec<Uuid> = self
        .pods
        .values()
        .filter(|p| p.deployment_id == deployment_id)
        .map(|p| p.id)
        .collect();

    let current_count = current_pods.len() as u32;

    if current_count < desired {
        for i in current_count..desired {
            self.create_pod(deployment_id, &spec, i).await?;
        }
    } else if current_count > desired {
        current_pods.sort();
        for &pod_id in &current_pods[(desired as usize)..] {
            if let Some(pod) = self.pods.get(&pod_id) {
                if let Some(cid) = &pod.container_id {
                    let _ = self.runtime.stop_container(cid, 10).await;
                    let _ = self.runtime.remove_container(cid, true).await;
                }
            }
            self.persist_delete_pod(&pod_id).await;
            self.pods.remove(&pod_id);
        }
    }

    let (all_running, any_failed) = self
        .pods
        .values()
        .filter(|p| p.deployment_id == deployment_id)
        .fold((true, false), |(all_r, any_f), p| {
            match p.status {
                PodStatus::Running => (all_r, any_f),
                PodStatus::Failed => (false, true),
                _ => (false, any_f),
            }
        });

    if let Some(d) = self.deployments.get_mut(&deployment_id) {
        d.status = if all_running && desired > 0 {
            DeploymentStatus::Running
        } else if any_failed {
            DeploymentStatus::Degraded
        } else {
            DeploymentStatus::Pending
        };
        let cloned = d.clone();
        self.persist_update_deployment(&cloned).await;
    }

    Ok(())
}
```

- [ ] **Step 10: Add persistence to handle_remove_deployment**

```rust
async fn handle_remove_deployment(&mut self, project: &str, name: &str) -> Result<()> {
    self.handle_stop(project, name).await?;
    let id = self
        .find_deployment_id(project, name)
        .ok_or_else(|| NexaError::DeploymentNotFound(format!("{project}/{name}")))?;

    if let Some(store) = &self.state_store {
        if let Err(e) = store.delete_deployment(&id).await {
            tracing::warn!(deployment_id = %id, error = %e, "failed to persist deployment delete");
        }
    }

    self.deployments.remove(&id);
    Ok(())
}
```

- [ ] **Step 11: Run all tests**

Run: `cargo test -p nexa-core -- domain::orchestrator 2>&1`
Expected: all tests pass (existing tests with `None` store + new persistence tests)

- [ ] **Step 12: Commit**

```bash
git add crates/nexa-core/src/domain/orchestrator.rs
git commit -m "feat: orchestrator writes all mutations through StateStore"
```

---

### Task 7: Startup reconciliation — load from StateStore

**Files:**
- Modify: `crates/nexa-core/src/domain/orchestrator.rs`

- [ ] **Step 1: Write failing test for startup load**

Add to the `tests` module:

```rust
#[tokio::test]
async fn loads_state_on_startup() {
    let store = Arc::new(InMemoryStore::new());

    // Pre-populate the store as if from a previous run
    let project = Project::new("loaded");
    store.insert_project(&project).await.unwrap();

    let spec = DeploymentSpec {
        project: "loaded".into(),
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
    let mut deployment = Deployment::from_spec(spec);
    deployment.status = DeploymentStatus::Running;
    store.insert_deployment(&deployment).await.unwrap();

    let mut pod = Pod::new(deployment.id, "loaded", "web", 0, "nginx");
    pod.status = PodStatus::Running;
    pod.container_id = Some("old-container-123".into());
    store.insert_pod(&pod).await.unwrap();

    // Start orchestrator with this pre-populated store
    let handle = Orchestrator::spawn(
        Arc::new(MockRuntime),
        Some(store.clone() as Arc<dyn StateStore>),
    );

    // Give the actor a moment to load state
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let projects = handle.list_projects().await;
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "loaded");

    let deployments = handle.list_deployments(Some("loaded".into())).await;
    assert_eq!(deployments.len(), 1);
    assert_eq!(deployments[0].name(), "web");
}
```

Run: `cargo test -p nexa-core -- domain::orchestrator::tests::loads_state_on_startup 2>&1`
Expected: FAIL — orchestrator does not load from store yet (projects list will be empty)

- [ ] **Step 2: Implement load_state method**

Add to `impl Orchestrator`:

```rust
/// Load all state from the store into memory.
/// Called once at startup before processing commands.
async fn load_state(&mut self) {
    let Some(store) = &self.state_store else { return };

    match store.list_projects().await {
        Ok(projects) => {
            for p in projects {
                self.projects.insert(p.name.clone(), p);
            }
            tracing::info!(count = self.projects.len(), "loaded projects from state store");
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to load projects from state store");
        }
    }

    match store.list_deployments(None).await {
        Ok(deployments) => {
            for d in deployments {
                self.deployments.insert(d.id, d);
            }
            tracing::info!(count = self.deployments.len(), "loaded deployments from state store");
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to load deployments from state store");
        }
    }

    match store.list_pods(None).await {
        Ok(pods) => {
            for p in pods {
                self.pods.insert(p.id, p);
            }
            tracing::info!(count = self.pods.len(), "loaded pods from state store");
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to load pods from state store");
        }
    }
}
```

- [ ] **Step 3: Call load_state at the start of run()**

Update the `run` method:

```rust
async fn run(&mut self, mut rx: mpsc::Receiver<Command>) {
    self.load_state().await;
    self.reconcile_stale_pods().await;

    while let Some(cmd) = rx.recv().await {
        match cmd {
            // ... existing match arms unchanged ...
        }
    }
}
```

- [ ] **Step 4: Run test to verify load works**

Run: `cargo test -p nexa-core -- domain::orchestrator::tests::loads_state_on_startup 2>&1`
Expected: test passes

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-core/src/domain/orchestrator.rs
git commit -m "feat: load persisted state from StateStore on startup"
```

---

### Task 8: Startup stale pod reconciliation

**Files:**
- Modify: `crates/nexa-core/src/domain/orchestrator.rs`

- [ ] **Step 1: Write failing test for stale pod detection**

The `MockRuntime` returns `ContainerState::Running` for `inspect_container`. We need a mock that can be configured to return different states. Add a new mock:

```rust
use std::sync::Mutex as StdMutex;

struct ConfigurableMockRuntime {
    /// Map from container_id to the state inspect should return.
    /// If not found, inspect returns an error (simulating container gone).
    container_states: StdMutex<StdHashMap<String, ContainerState>>,
}

impl ConfigurableMockRuntime {
    fn new() -> Self {
        Self {
            container_states: StdMutex::new(StdHashMap::new()),
        }
    }

    fn set_container_state(&self, id: &str, state: ContainerState) {
        self.container_states.lock().unwrap().insert(id.to_string(), state);
    }
}

#[async_trait::async_trait]
impl ContainerRuntime for ConfigurableMockRuntime {
    async fn pull_image(&self, _image: &str) -> Result<()> { Ok(()) }
    async fn create_container(&self, config: &ContainerConfig) -> Result<String> {
        let id = format!("mock-{}", config.name);
        self.container_states.lock().unwrap().insert(id.clone(), ContainerState::Running);
        Ok(id)
    }
    async fn start_container(&self, _id: &str) -> Result<()> { Ok(()) }
    async fn stop_container(&self, _id: &str, _timeout: u64) -> Result<()> { Ok(()) }
    async fn remove_container(&self, _id: &str, _force: bool) -> Result<()> { Ok(()) }
    async fn inspect_container(&self, id: &str) -> Result<ContainerInfo> {
        let states = self.container_states.lock().unwrap();
        match states.get(id) {
            Some(state) => Ok(ContainerInfo {
                id: id.into(),
                name: id.into(),
                image: "mock".into(),
                state: state.clone(),
            }),
            None => Err(NexaError::Runtime(format!("container {id} not found"))),
        }
    }
    async fn logs(&self, _id: &str, _tail: Option<u64>) -> Result<LogStream> {
        Ok(Box::pin(futures::stream::empty()))
    }
    async fn container_exists(&self, _name: &str) -> Result<bool> { Ok(false) }
    async fn create_network(&self, _name: &str) -> Result<String> { Ok("net-id".into()) }
    async fn remove_network(&self, _name: &str) -> Result<()> { Ok(()) }
    async fn connect_to_network(&self, _id: &str, _net: &str) -> Result<()> { Ok(()) }
}
```

Add the test:

```rust
#[tokio::test]
async fn reconcile_marks_stale_pods_failed() {
    let store = Arc::new(InMemoryStore::new());

    // Pre-populate: a "running" pod whose container is actually gone
    let project = Project::new("stale");
    store.insert_project(&project).await.unwrap();

    let spec = DeploymentSpec {
        project: "stale".into(),
        deployment: DeploymentMeta { name: "api".into() },
        replicas: 1,
        image: "nginx".into(),
        ports: vec![],
        env: HashMap::new(),
        volumes: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::default(),
    };
    let mut deployment = Deployment::from_spec(spec);
    deployment.status = DeploymentStatus::Running;
    store.insert_deployment(&deployment).await.unwrap();

    let mut pod = Pod::new(deployment.id, "stale", "api", 0, "nginx");
    pod.status = PodStatus::Running;
    pod.container_id = Some("vanished-container".into());
    store.insert_pod(&pod).await.unwrap();

    // Runtime does NOT know about "vanished-container" -> inspect will error
    let runtime = Arc::new(ConfigurableMockRuntime::new());

    let handle = Orchestrator::spawn(
        runtime,
        Some(store.clone() as Arc<dyn StateStore>),
    );

    // Wait for startup reconciliation
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let pods = handle.list_pods(Some("stale".into())).await;
    assert_eq!(pods.len(), 1);
    assert_eq!(pods[0].status, PodStatus::Failed, "stale pod should be marked Failed");

    let deployments = handle.list_deployments(Some("stale".into())).await;
    assert_eq!(deployments[0].status, DeploymentStatus::Degraded);
}
```

Run: `cargo test -p nexa-core -- domain::orchestrator::tests::reconcile_marks_stale_pods_failed 2>&1`
Expected: FAIL — `reconcile_stale_pods` not implemented yet

- [ ] **Step 2: Implement reconcile_stale_pods**

Add to `impl Orchestrator`:

```rust
/// On startup, check each pod marked Running to see if its container
/// actually exists. Mark it Failed if the container is gone.
/// Then recalculate deployment statuses.
async fn reconcile_stale_pods(&mut self) {
    let running_pod_ids: Vec<Uuid> = self
        .pods
        .values()
        .filter(|p| p.status == PodStatus::Running && p.container_id.is_some())
        .map(|p| p.id)
        .collect();

    if running_pod_ids.is_empty() {
        return;
    }

    tracing::info!(count = running_pod_ids.len(), "reconciling pods with runtime");

    for pod_id in running_pod_ids {
        let container_id = match self.pods.get(&pod_id) {
            Some(p) => match &p.container_id {
                Some(cid) => cid.clone(),
                None => continue,
            },
            None => continue,
        };

        let is_running = match self.runtime.inspect_container(&container_id).await {
            Ok(info) => info.state == ContainerState::Running,
            Err(_) => false, // container gone
        };

        if !is_running {
            if let Some(pod) = self.pods.get_mut(&pod_id) {
                tracing::warn!(
                    pod_id = %pod_id,
                    container_id = %container_id,
                    "pod container not running, marking Failed"
                );
                pod.status = PodStatus::Failed;
                let cloned = pod.clone();
                self.persist_update_pod(&cloned).await;
            }
        }
    }

    // Recalculate deployment statuses
    let deployment_ids: Vec<Uuid> = self.deployments.keys().cloned().collect();
    for deployment_id in deployment_ids {
        let desired = match self.deployments.get(&deployment_id) {
            Some(d) => d.spec.replicas,
            None => continue,
        };

        let (all_running, any_failed) = self
            .pods
            .values()
            .filter(|p| p.deployment_id == deployment_id)
            .fold((true, false), |(all_r, any_f), p| match p.status {
                PodStatus::Running => (all_r, any_f),
                PodStatus::Failed => (false, true),
                _ => (false, any_f),
            });

        if let Some(d) = self.deployments.get_mut(&deployment_id) {
            let new_status = if all_running && desired > 0 {
                DeploymentStatus::Running
            } else if any_failed {
                DeploymentStatus::Degraded
            } else {
                DeploymentStatus::Pending
            };

            if d.status != new_status {
                d.status = new_status;
                let cloned = d.clone();
                self.persist_update_deployment(&cloned).await;
            }
        }
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p nexa-core -- domain::orchestrator 2>&1`
Expected: all tests pass including the stale pod test

- [ ] **Step 4: Commit**

```bash
git add crates/nexa-core/src/domain/orchestrator.rs
git commit -m "feat: reconcile stale pods on startup by checking container runtime"
```

---

### Task 9: Wire SqliteStore into nexad main.rs

**Files:**
- Modify: `crates/nexad/src/main.rs`
- Modify: `crates/nexa-core/src/error.rs` (add StateStore error variant)

- [ ] **Step 1: Add StateStore error variant**

In `crates/nexa-core/src/error.rs`, add a new variant to `NexaError`:

```rust
#[derive(Debug, Error)]
pub enum NexaError {
    #[error("project not found: {0}")]
    ProjectNotFound(String),

    #[error("deployment not found: {0}")]
    DeploymentNotFound(String),

    #[error("pod not found: {0}")]
    PodNotFound(String),

    #[error("container runtime error: {0}")]
    Runtime(String),

    #[error("invalid deployment spec: {0}")]
    InvalidSpec(String),

    #[error("port conflict: port {0} is already in use")]
    PortConflict(u16),

    #[error("image pull failed: {0}")]
    ImagePull(String),

    #[error("health check failed for {0}")]
    HealthCheckFailed(String),

    #[error("state store error: {0}")]
    StateStore(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}
```

- [ ] **Step 2: Update nexad main.rs to create SqliteStore**

Replace `crates/nexad/src/main.rs`:

```rust
mod adapters;
mod api;

use std::sync::Arc;

use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

use nexa_core::domain::orchestrator::Orchestrator;
use nexa_core::ports::state::StateStore;

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

    // Ensure data directory exists
    std::fs::create_dir_all(&cli.data_dir)?;

    // Initialize SQLite state store
    let db_path = format!("{}/nexa.db", cli.data_dir);
    let database_url = format!("sqlite:{}?mode=rwc", db_path);
    let store = adapters::state::SqliteStore::connect(&database_url).await?;
    let store: Arc<dyn StateStore> = Arc::new(store);
    info!(path = db_path, "state store initialized");

    // Connect to Docker runtime
    let runtime = adapters::runtime::DockerRuntime::new()?;
    runtime.ping().await?;
    info!("connected to Docker runtime");

    // Spawn orchestrator with persistence
    let handle = Orchestrator::spawn(Arc::new(runtime), Some(store));
    let addr = format!("{}:{}", cli.host, cli.port);

    api::serve(handle, &addr).await
}
```

- [ ] **Step 3: Verify nexad compiles**

Run: `cargo check -p nexad 2>&1`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add crates/nexad/src/main.rs crates/nexa-core/src/error.rs
git commit -m "feat: wire SqliteStore into nexad startup with data_dir persistence"
```

---

### Task 10: End-to-end integration test with real SQLite (tempfile)

**Files:**
- Create: `crates/nexad/tests/sqlite_integration.rs`
- Modify: `crates/nexad/Cargo.toml` (add dev-dependency tempfile)

- [ ] **Step 1: Add tempfile dev-dependency**

In `Cargo.toml` (workspace), add:

```toml
tempfile = "3"
```

In `crates/nexad/Cargo.toml`, add:

```toml
[dev-dependencies]
tempfile = { workspace = true }
nexa-core = { workspace = true }
```

- [ ] **Step 2: Write the integration test**

Create `crates/nexad/tests/sqlite_integration.rs`:

```rust
use std::collections::HashMap;
use std::sync::Arc;

use nexa_core::domain::models::*;
use nexa_core::ports::state::StateStore;

// We test SqliteStore directly without the full daemon.
// The adapters module is internal to nexad, so we import via the crate.
// For integration tests, we reference the adapter through the public path.

#[tokio::test]
async fn full_lifecycle_with_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());

    // ── Connect and migrate ──
    let store = nexad::adapters::state::SqliteStore::connect(&url)
        .await
        .expect("failed to connect to SQLite");

    // ── Create project ──
    let project = Project::new("integration");
    store.insert_project(&project).await.unwrap();

    // ── Create deployment ──
    let spec = DeploymentSpec {
        project: "integration".into(),
        deployment: DeploymentMeta { name: "api".into() },
        replicas: 2,
        image: "nginx:latest".into(),
        ports: vec![8080],
        env: HashMap::from([("ENV".into(), "test".into())]),
        volumes: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::default(),
    };
    let deployment = Deployment::from_spec(spec);
    store.insert_deployment(&deployment).await.unwrap();

    // ── Create pods ──
    let pod0 = Pod::new(deployment.id, "integration", "api", 0, "nginx:latest");
    let pod1 = Pod::new(deployment.id, "integration", "api", 1, "nginx:latest");
    store.insert_pod(&pod0).await.unwrap();
    store.insert_pod(&pod1).await.unwrap();

    // ── Verify reads ──
    let projects = store.list_projects().await.unwrap();
    assert_eq!(projects.len(), 1);

    let deployments = store.list_deployments(Some("integration")).await.unwrap();
    assert_eq!(deployments.len(), 1);
    assert_eq!(deployments[0].spec.replicas, 2);
    assert_eq!(deployments[0].spec.env.get("ENV").unwrap(), "test");

    let pods = store.pods_by_deployment(&deployment.id).await.unwrap();
    assert_eq!(pods.len(), 2);

    // ── Update deployment status ──
    let mut updated = deployments[0].clone();
    updated.status = DeploymentStatus::Running;
    store.update_deployment(&updated).await.unwrap();

    let refetched = store
        .get_deployment("integration", "api")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(refetched.status, DeploymentStatus::Running);

    // ── Delete deployment (should cascade pods) ──
    store.delete_deployment(&deployment.id).await.unwrap();
    let remaining_pods = store.list_pods(None).await.unwrap();
    assert_eq!(remaining_pods.len(), 0, "cascade delete should remove pods");

    // ── Reconnect to same DB and verify persistence ──
    drop(store);
    let store2 = nexad::adapters::state::SqliteStore::connect(&url)
        .await
        .expect("reconnect failed");

    let projects2 = store2.list_projects().await.unwrap();
    assert_eq!(projects2.len(), 1, "project should survive reconnect");
    assert_eq!(projects2[0].name, "integration");
}
```

**Important:** This test requires `nexad` to expose `adapters` publicly. Since `adapters` is currently `mod adapters;` (private), we need to make it public for integration test access.

Update `crates/nexad/src/main.rs` — change `mod adapters;` to:

```rust
pub mod adapters;
```

Also add `#[path]` or make the crate a lib+bin. The simplest approach: create `crates/nexad/src/lib.rs` to re-export:

Create `crates/nexad/src/lib.rs`:

```rust
pub mod adapters;
```

And update `crates/nexad/src/main.rs` to use `nexad::adapters` instead of `mod adapters`:

```rust
mod api;

use std::sync::Arc;

use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

use nexa_core::domain::orchestrator::Orchestrator;
use nexa_core::ports::state::StateStore;

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

    // Ensure data directory exists
    std::fs::create_dir_all(&cli.data_dir)?;

    // Initialize SQLite state store
    let db_path = format!("{}/nexa.db", cli.data_dir);
    let database_url = format!("sqlite:{}?mode=rwc", db_path);
    let store = nexad::adapters::state::SqliteStore::connect(&database_url).await?;
    let store: Arc<dyn StateStore> = Arc::new(store);
    info!(path = db_path, "state store initialized");

    // Connect to Docker runtime
    let runtime = nexad::adapters::runtime::DockerRuntime::new()?;
    runtime.ping().await?;
    info!("connected to Docker runtime");

    // Spawn orchestrator with persistence
    let handle = Orchestrator::spawn(Arc::new(runtime), Some(store));
    let addr = format!("{}:{}", cli.host, cli.port);

    api::serve(handle, &addr).await
}
```

- [ ] **Step 3: Run the integration test**

Run: `cargo test -p nexad -- sqlite_integration 2>&1`
Expected: test passes

- [ ] **Step 4: Run full workspace test suite**

Run: `cargo test 2>&1`
Expected: all tests pass across all crates

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/nexad/
git commit -m "test: add end-to-end SQLite integration test with tempfile"
```

---

### Task 11: Final verification and cleanup

**Files:**
- None new — verification pass only

- [ ] **Step 1: Verify workspace compiles in release mode**

Run: `cargo build --release 2>&1 | tail -5`
Expected: compiles successfully

- [ ] **Step 2: Run full test suite one final time**

Run: `cargo test 2>&1`
Expected: all tests pass

- [ ] **Step 3: Check for any remaining warnings**

Run: `cargo check 2>&1 | grep -i warning`
Fix any legitimate warnings (unused imports, etc.)

- [ ] **Step 4: Commit any final cleanup**

```bash
git add -A
git commit -m "chore: clean up warnings after SQLite persistence integration"
```

- [ ] **Step 5: Push**

```bash
git push origin main
```
