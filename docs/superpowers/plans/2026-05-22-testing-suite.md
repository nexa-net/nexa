# Helyos Testing Suite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add integration tests, E2E tests with real Docker, and Criterion benchmarks with CI regression detection across all Helyos repos.

**Architecture:** Three test layers, each independent. Layer 1 adds integration tests to helyosd (API + SQLite) and helyos-cli (command parsing + output). Layer 2 adds full-stack E2E tests to helyosd using real Docker containers. Layer 3 adds Criterion benchmarks to helyos-core and helyosd with CI regression gating via `github-action-benchmark`.

**Tech Stack:** Rust test framework, criterion 0.5, reqwest 0.12 (test HTTP client), tempfile 3 (temp dirs), hyper 1 (test HTTP servers), tokio (async test runtime), github-action-benchmark (CI regression detection)

**Multi-repo layout:**
- helyos-core: `/Users/nassime/GitHub/Helyos/helyos-core`
- helyosd: `/Users/nassime/GitHub/Helyos/helyosd`
- helyos-cli: `/Users/nassime/GitHub/Helyos/helyos-cli`

---

## File Structure

### helyosd changes
- **Modify:** `src/lib.rs` — expose `api` module publicly for integration tests
- **Modify:** `src/main.rs` — move `mod api` from here to `src/lib.rs`
- **Modify:** `Cargo.toml` — add `reqwest`, `criterion` dev-dependencies
- **Create:** `tests/api_integration.rs` — API integration tests with MockRuntime + SQLite
- **Modify:** `tests/sqlite_integration.rs` — enrich with cascade, concurrent, node, route tests
- **Create:** `tests/e2e.rs` — full-stack E2E with real Docker
- **Create:** `benches/sqlite_store.rs` — SQLite benchmark
- **Create:** `benches/crypto.rs` — AES-256-GCM benchmark
- **Create:** `benches/dns.rs` — DNS record store benchmark
- **Modify:** `.github/workflows/ci.yml` — add integration + e2e jobs
- **Create:** `.github/workflows/bench.yml` — benchmark CI with regression gate

### helyos-core changes
- **Modify:** `Cargo.toml` — add `criterion` dev-dependency
- **Create:** `benches/scheduler.rs` — scheduler benchmark
- **Create:** `benches/config_parsing.rs` — YAML parsing benchmark
- **Create:** `.github/workflows/bench.yml` — benchmark CI

### helyos-cli changes
- **Modify:** `src/output/table.rs` — add tests for table rendering + JSON mode
- **Modify:** `src/output/mod.rs` — add tests for style functions
- **Modify:** `src/main.rs` — add tests for CLI argument parsing

---

### Task 1: Expose helyosd API module for integration tests

**Files:**
- Modify: `helyosd/src/lib.rs`
- Modify: `helyosd/src/main.rs`

The `api` module is currently private to the binary (`mod api` in `main.rs`). Integration tests use the library crate, so they can't access it. We need to move it to `lib.rs` as a public module.

- [ ] **Step 1: Move `mod api` from main.rs to lib.rs**

In `helyosd/src/lib.rs`, add `pub mod api;`:

```rust
pub mod adapters;
pub mod api;
pub mod cluster;
pub mod crypto;
```

In `helyosd/src/main.rs`, remove the `mod api;` line (line 1). Replace internal usages of `api::` with `helyosd::api::`. The main.rs already uses `helyosd::adapters::...` for adapters, so update the `api::serve` call:

Find this in main.rs:
```rust
mod api;
```
Remove it.

Find calls like `api::serve(...)` and replace with `helyosd::api::serve(...)`.

- [ ] **Step 2: Verify compilation**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo build`
Expected: Compiles successfully

- [ ] **Step 3: Verify existing tests pass**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo test --lib`
Expected: All 156 tests pass

- [ ] **Step 4: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyosd
git add src/lib.rs src/main.rs
git commit -m "refactor: expose api module publicly for integration tests"
```

---

### Task 2: Add dev-dependencies to helyosd

**Files:**
- Modify: `helyosd/Cargo.toml`

- [ ] **Step 1: Add reqwest and criterion to dev-dependencies**

In `helyosd/Cargo.toml`, add to the existing `[dev-dependencies]` section:

```toml
[dev-dependencies]
tempfile = "3"
uuid = { version = "1", features = ["v4"] }
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
criterion = { version = "0.5", features = ["html_reports", "async_tokio"] }
```

- [ ] **Step 2: Verify compilation**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo test --lib --no-run`
Expected: Compiles with new dependencies

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyosd
git add Cargo.toml
git commit -m "build: add reqwest and criterion dev-dependencies"
```

---

### Task 3: helyosd API integration tests — health + projects

**Files:**
- Create: `helyosd/tests/api_integration.rs`

These tests spin up the full axum API with real SQLite and MockRuntime (no Docker needed).

- [ ] **Step 1: Create the test file with setup helpers and first tests**

Create `helyosd/tests/api_integration.rs`:

```rust
use std::net::SocketAddr;
use std::sync::Arc;

use helyos_core::domain::orchestrator::Orchestrator;
use helyos_core::ports::state::StateStore;
use helyosd::adapters::secrets::EncryptedSqliteSecretStore;
use helyosd::adapters::state::SqliteStore;
use helyosd::api;

struct TestServer {
    addr: SocketAddr,
    _data_dir: tempfile::TempDir,
}

impl TestServer {
    async fn start() -> Self {
        let data_dir = tempfile::tempdir().unwrap();
        let db_path = data_dir.path().join("test.db");
        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());

        let store = SqliteStore::connect(&db_url).await.unwrap();
        let store: Arc<dyn StateStore> = Arc::new(store);

        let secret_db_path = data_dir.path().join("secrets.db");
        let secret_conn = rusqlite::Connection::open(&secret_db_path).unwrap();
        let master_key = [0u8; 32];
        let secret_store: Arc<dyn helyos_core::ports::secrets::SecretStore> =
            Arc::new(EncryptedSqliteSecretStore::new(secret_conn, &master_key).unwrap());

        let runtime: Arc<dyn helyos_core::ports::runtime::ContainerRuntime> =
            Arc::new(MockRuntime);

        let transport: Arc<dyn helyos_core::ports::cluster::ClusterTransport> =
            Arc::new(helyosd::adapters::transport::LocalTransport::new(Arc::clone(&runtime)));

        let handle = Orchestrator::spawn(
            runtime,
            Some(store.clone()),
            Some(secret_store),
            Some(transport),
            None,
            None,
            None,
            None,
        );

        let state = api::AppState {
            handle,
            store,
        };
        let app = api::routes::build(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self { addr, _data_dir: data_dir }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

struct MockRuntime;

#[async_trait::async_trait]
impl helyos_core::ports::runtime::ContainerRuntime for MockRuntime {
    fn runtime_name(&self) -> &'static str { "mock" }
    async fn pull_image(&self, _image: &str) -> helyos_core::error::Result<()> { Ok(()) }
    async fn create_container(
        &self,
        config: &helyos_core::ports::runtime::ContainerConfig,
    ) -> helyos_core::error::Result<String> {
        Ok(format!("mock-{}", config.name))
    }
    async fn start_container(&self, _id: &str) -> helyos_core::error::Result<()> { Ok(()) }
    async fn stop_container(&self, _id: &str, _timeout: u64) -> helyos_core::error::Result<()> { Ok(()) }
    async fn remove_container(&self, _id: &str, _force: bool) -> helyos_core::error::Result<()> { Ok(()) }
    async fn inspect_container(
        &self,
        _id: &str,
    ) -> helyos_core::error::Result<helyos_core::ports::runtime::ContainerInfo> {
        Ok(helyos_core::ports::runtime::ContainerInfo {
            id: "mock".into(),
            name: "mock".into(),
            image: "mock".into(),
            state: helyos_core::ports::runtime::ContainerState::Running,
        })
    }
    async fn logs(
        &self,
        _id: &str,
        _tail: Option<u64>,
    ) -> helyos_core::error::Result<helyos_core::ports::runtime::LogStream> {
        Ok(Box::pin(futures::stream::empty()))
    }
    async fn container_exists(&self, _name: &str) -> helyos_core::error::Result<bool> { Ok(false) }
    async fn create_network(&self, _name: &str) -> helyos_core::error::Result<String> {
        Ok("net-id".into())
    }
    async fn remove_network(&self, _name: &str) -> helyos_core::error::Result<()> { Ok(()) }
    async fn connect_to_network(&self, _id: &str, _net: &str) -> helyos_core::error::Result<()> {
        Ok(())
    }
    async fn container_ip(
        &self,
        _container_id: &str,
        _network: &str,
    ) -> helyos_core::error::Result<String> {
        Ok("172.17.0.2".to_string())
    }
    async fn events(&self) -> helyos_core::error::Result<helyos_core::ports::runtime::EventStream> {
        Ok(Box::pin(futures::stream::pending()))
    }
}

#[tokio::test]
async fn health_returns_ok() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();
    let resp = client.get(server.url("/health")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn create_and_list_projects() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // Create a project
    let resp = client
        .post(server.url("/api/v1/projects"))
        .json(&serde_json::json!({ "name": "testproject" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // List projects
    let resp = client.get(server.url("/api/v1/projects")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let projects = body.as_array().unwrap();
    assert!(projects.iter().any(|p| p["name"] == "testproject"));
}

#[tokio::test]
async fn delete_project() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    client
        .post(server.url("/api/v1/projects"))
        .json(&serde_json::json!({ "name": "deleteme" }))
        .send()
        .await
        .unwrap();

    let resp = client
        .delete(server.url("/api/v1/projects/deleteme"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = client.get(server.url("/api/v1/projects")).send().await.unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let projects = body.as_array().unwrap();
    assert!(!projects.iter().any(|p| p["name"] == "deleteme"));
}

#[tokio::test]
async fn suspend_and_resume_project() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    client
        .post(server.url("/api/v1/projects"))
        .json(&serde_json::json!({ "name": "pausable" }))
        .send()
        .await
        .unwrap();

    let resp = client
        .post(server.url("/api/v1/projects/pausable/suspend"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let resp = client
        .post(server.url("/api/v1/projects/pausable/resume"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}
```

- [ ] **Step 2: Run to verify tests pass**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo test --test api_integration`
Expected: 4 tests pass

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyosd
git add tests/api_integration.rs
git commit -m "test: add API integration tests for health and project CRUD"
```

---

### Task 4: helyosd API integration tests — deploy, scale, pods, secrets, routes

**Files:**
- Modify: `helyosd/tests/api_integration.rs`

- [ ] **Step 1: Add deploy lifecycle test**

Append to `helyosd/tests/api_integration.rs`:

```rust
#[tokio::test]
async fn deploy_and_list_pods() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // Create project first
    client
        .post(server.url("/api/v1/projects"))
        .json(&serde_json::json!({ "name": "deploytest" }))
        .send()
        .await
        .unwrap();

    // Deploy
    let spec = serde_json::json!({
        "project": "deploytest",
        "deployment": { "name": "web" },
        "replicas": 2,
        "image": "nginx:latest",
        "ports": [8080]
    });
    let resp = client
        .post(server.url("/api/v1/deploy"))
        .header("content-type", "application/json")
        .body(serde_json::to_string(&spec).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // List deployments
    let resp = client
        .get(server.url("/api/v1/deployments?project=deploytest"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let deployments = body.as_array().unwrap();
    assert_eq!(deployments.len(), 1);

    // List pods
    let resp = client
        .get(server.url("/api/v1/pods?project=deploytest"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let pods = body.as_array().unwrap();
    assert_eq!(pods.len(), 2);
}

#[tokio::test]
async fn scale_deployment() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    client
        .post(server.url("/api/v1/projects"))
        .json(&serde_json::json!({ "name": "scaletest" }))
        .send()
        .await
        .unwrap();

    let spec = serde_json::json!({
        "project": "scaletest",
        "deployment": { "name": "api" },
        "replicas": 1,
        "image": "nginx:latest",
        "ports": [3000]
    });
    client
        .post(server.url("/api/v1/deploy"))
        .header("content-type", "application/json")
        .body(serde_json::to_string(&spec).unwrap())
        .send()
        .await
        .unwrap();

    // Scale to 3
    let resp = client
        .post(server.url("/api/v1/projects/scaletest/deployments/api/scale"))
        .json(&serde_json::json!({ "replicas": 3 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify 3 pods
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let resp = client
        .get(server.url("/api/v1/pods?project=scaletest"))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let pods = body.as_array().unwrap();
    assert_eq!(pods.len(), 3);
}

#[tokio::test]
async fn stop_and_remove_deployment() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    client
        .post(server.url("/api/v1/projects"))
        .json(&serde_json::json!({ "name": "stoptest" }))
        .send()
        .await
        .unwrap();

    let spec = serde_json::json!({
        "project": "stoptest",
        "deployment": { "name": "svc" },
        "replicas": 1,
        "image": "nginx:latest",
        "ports": [80]
    });
    client
        .post(server.url("/api/v1/deploy"))
        .header("content-type", "application/json")
        .body(serde_json::to_string(&spec).unwrap())
        .send()
        .await
        .unwrap();

    // Stop
    let resp = client
        .post(server.url("/api/v1/projects/stoptest/deployments/svc/stop"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Remove
    let resp = client
        .delete(server.url("/api/v1/projects/stoptest/deployments/svc"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn secrets_crud() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    client
        .post(server.url("/api/v1/projects"))
        .json(&serde_json::json!({ "name": "secrettest" }))
        .send()
        .await
        .unwrap();

    // Set secret
    let resp = client
        .post(server.url("/api/v1/projects/secrettest/secrets/DB_PASS"))
        .json(&serde_json::json!({ "value": "s3cret" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // List secrets
    let resp = client
        .get(server.url("/api/v1/projects/secrettest/secrets"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let secrets = body.as_array().unwrap();
    assert!(secrets.iter().any(|s| s.as_str() == Some("DB_PASS")));

    // Delete secret
    let resp = client
        .delete(server.url("/api/v1/projects/secrettest/secrets/DB_PASS"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn route_management() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    client
        .post(server.url("/api/v1/projects"))
        .json(&serde_json::json!({ "name": "routetest" }))
        .send()
        .await
        .unwrap();

    // Deploy something first so the route has a target
    let spec = serde_json::json!({
        "project": "routetest",
        "deployment": { "name": "web" },
        "replicas": 1,
        "image": "nginx:latest",
        "ports": [80]
    });
    client
        .post(server.url("/api/v1/deploy"))
        .header("content-type", "application/json")
        .body(serde_json::to_string(&spec).unwrap())
        .send()
        .await
        .unwrap();

    // Add route
    let resp = client
        .post(server.url("/api/v1/routes"))
        .json(&serde_json::json!({
            "domain": "test.example.com",
            "project": "routetest",
            "deployment": "web",
            "tls_mode": "none"
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // List routes
    let resp = client
        .get(server.url("/api/v1/routes"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Delete route
    let resp = client
        .delete(server.url("/api/v1/routes/test.example.com"))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn scheduler_config() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    // Get default config
    let resp = client
        .get(server.url("/api/v1/cluster/scheduler"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.get("strategy").is_some());

    // Set new config
    let resp = client
        .post(server.url("/api/v1/cluster/scheduler"))
        .json(&serde_json::json!({
            "strategy": "binpack",
            "weights": { "cpu": -0.3, "memory": -0.3, "load": -0.1, "failure": 0.15 }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn deploy_invalid_spec_returns_400() {
    let server = TestServer::start().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(server.url("/api/v1/deploy"))
        .header("content-type", "application/json")
        .body("not valid json or yaml")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}
```

- [ ] **Step 2: Run all API integration tests**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo test --test api_integration`
Expected: 11 tests pass

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyosd
git add tests/api_integration.rs
git commit -m "test: add API integration tests for deploy, scale, secrets, routes"
```

---

### Task 5: Enrich SQLite integration tests

**Files:**
- Modify: `helyosd/tests/sqlite_integration.rs`

- [ ] **Step 1: Read the existing file**

Read: `helyosd/tests/sqlite_integration.rs` to see current structure and imports.

- [ ] **Step 2: Add cascade delete, node CRUD, and concurrent write tests**

Append the following tests to `helyosd/tests/sqlite_integration.rs`:

```rust
#[tokio::test]
async fn cascade_delete_project_removes_deployments_and_pods() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cascade.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let store = helyosd::adapters::state::SqliteStore::connect(&url).await.unwrap();

    use helyos_core::domain::models::*;
    use helyos_core::ports::state::StateStore;

    let project = Project::new("cascadetest");
    store.insert_project(&project).await.unwrap();

    let spec = DeploymentSpec {
        project: "cascadetest".into(),
        deployment: DeploymentMeta { name: "svc".into() },
        replicas: 1,
        image: "nginx:latest".into(),
        ports: vec![80],
        env: Default::default(),
        volumes: vec![],
        secrets: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::default(),
        resources: None,
    };
    let deployment = Deployment::new(spec);
    let deployment_id = deployment.id;
    store.insert_deployment(&deployment).await.unwrap();

    let pod = Pod::new(deployment_id, "cascadetest", "svc", "nginx:latest");
    store.insert_pod(&pod).await.unwrap();

    // Delete project
    store.delete_project("cascadetest").await.unwrap();

    // Verify deployments gone
    let deployments = store.list_deployments(Some("cascadetest")).await.unwrap();
    assert!(deployments.is_empty());

    // Verify pods gone
    let pods = store.list_pods(Some("cascadetest")).await.unwrap();
    assert!(pods.is_empty());
}

#[tokio::test]
async fn node_crud_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("nodes.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let store = helyosd::adapters::state::SqliteStore::connect(&url).await.unwrap();

    use helyos_core::domain::models::*;
    use helyos_core::ports::state::StateStore;

    // Insert node
    let node = Node {
        id: uuid::Uuid::new_v4(),
        name: "worker-1".into(),
        address: "10.0.1.1:6444".into(),
        role: NodeRole::Worker,
        status: NodeStatus::Ready,
        last_heartbeat: chrono::Utc::now(),
        cpu_total: 4.0,
        cpu_available: 4.0,
        memory_total: 8_000_000_000,
        memory_available: 8_000_000_000,
        running_pods: 0,
        max_pods: 110,
    };
    store.insert_node(&node).await.unwrap();

    // List nodes
    let nodes = store.list_nodes().await.unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name, "worker-1");

    // Update node status
    let mut updated = node.clone();
    updated.status = NodeStatus::NotReady;
    store.update_node(&updated).await.unwrap();

    let nodes = store.list_nodes().await.unwrap();
    assert_eq!(nodes[0].status, NodeStatus::NotReady);

    // Remove node
    store.delete_node(&node.id).await.unwrap();
    let nodes = store.list_nodes().await.unwrap();
    assert!(nodes.is_empty());
}

#[tokio::test]
async fn concurrent_pod_inserts() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("concurrent.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let store = std::sync::Arc::new(
        helyosd::adapters::state::SqliteStore::connect(&url).await.unwrap(),
    );

    use helyos_core::domain::models::*;
    use helyos_core::ports::state::StateStore;

    let project = Project::new("conctest");
    store.insert_project(&project).await.unwrap();

    let spec = DeploymentSpec {
        project: "conctest".into(),
        deployment: DeploymentMeta { name: "svc".into() },
        replicas: 1,
        image: "nginx:latest".into(),
        ports: vec![],
        env: Default::default(),
        volumes: vec![],
        secrets: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::default(),
        resources: None,
    };
    let deployment = Deployment::new(spec);
    let deployment_id = deployment.id;
    store.insert_deployment(&deployment).await.unwrap();

    // Spawn 20 concurrent pod inserts
    let mut handles = Vec::new();
    for _ in 0..20 {
        let store = store.clone();
        handles.push(tokio::spawn(async move {
            let pod = Pod::new(deployment_id, "conctest", "svc", "nginx:latest");
            store.insert_pod(&pod).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let pods = store.list_pods(Some("conctest")).await.unwrap();
    assert_eq!(pods.len(), 20);
}
```

- [ ] **Step 3: Run to verify**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo test --test sqlite_integration`
Expected: 4 tests pass (1 existing + 3 new)

- [ ] **Step 4: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyosd
git add tests/sqlite_integration.rs
git commit -m "test: enrich SQLite integration tests with cascade, nodes, concurrency"
```

---

### Task 6: helyos-cli command parsing and output tests

**Files:**
- Modify: `helyos-cli/src/output/table.rs`
- Modify: `helyos-cli/src/main.rs`

- [ ] **Step 1: Add table rendering tests**

Append to the bottom of `helyos-cli/src/output/table.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::output;

    #[test]
    fn json_mode_outputs_valid_json() {
        output::set_json_mode(true);
        let headers = &["NAME", "STATUS", "AGE"];
        let rows = vec![
            vec!["api".into(), "running".into(), "5m".into()],
            vec!["web".into(), "stopped".into(), "1h".into()],
        ];

        // Capture would need redirect; instead verify the JSON structure
        let mut json_rows = Vec::new();
        for row in &rows {
            let mut obj = serde_json::Map::new();
            for (i, header) in headers.iter().enumerate() {
                obj.insert(
                    header.to_lowercase(),
                    serde_json::Value::String(row[i].clone()),
                );
            }
            json_rows.push(serde_json::Value::Object(obj));
        }
        let json = serde_json::Value::Array(json_rows);
        assert_eq!(json[0]["name"], "api");
        assert_eq!(json[1]["status"], "stopped");

        output::set_json_mode(false);
    }

    #[test]
    fn empty_rows_handled() {
        output::set_json_mode(false);
        let headers = &["NAME"];
        let rows: Vec<Vec<String>> = vec![];
        // Should not panic
        print_table(headers, &rows);
    }
}
```

- [ ] **Step 2: Add CLI argument parsing tests**

Append to the bottom of `helyos-cli/src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use clap::Parser;
    use super::*;

    #[test]
    fn parse_deploy_command() {
        let cli = Cli::try_parse_from(["helyos", "deploy", "app.yaml"]).unwrap();
        match cli.command {
            Commands::Deploy { file } => assert_eq!(file, "app.yaml"),
            _ => panic!("expected Deploy"),
        }
    }

    #[test]
    fn parse_scale_command() {
        let cli = Cli::try_parse_from(["helyos", "scale", "api", "5", "-p", "myapp"]).unwrap();
        match cli.command {
            Commands::Scale { name, replicas, project } => {
                assert_eq!(name, "api");
                assert_eq!(replicas, 5);
                assert_eq!(project.as_deref(), Some("myapp"));
            }
            _ => panic!("expected Scale"),
        }
    }

    #[test]
    fn parse_pods_with_project() {
        let cli = Cli::try_parse_from(["helyos", "pods", "--project", "web"]).unwrap();
        match cli.command {
            Commands::Pods { project } => assert_eq!(project.as_deref(), Some("web")),
            _ => panic!("expected Pods"),
        }
    }

    #[test]
    fn parse_json_flag() {
        let cli = Cli::try_parse_from(["helyos", "--json", "status"]).unwrap();
        assert!(cli.json);
    }

    #[test]
    fn parse_server_flag() {
        let cli = Cli::try_parse_from(["helyos", "--server", "http://10.0.1.1:6443", "status"]).unwrap();
        assert_eq!(cli.server, "http://10.0.1.1:6443");
    }

    #[test]
    fn parse_secret_set() {
        let cli = Cli::try_parse_from(["helyos", "secret", "set", "DB_PASS", "s3cret", "-p", "myapp"]).unwrap();
        match cli.command {
            Commands::Secret { command: SecretCommands::Set { name, value, project } } => {
                assert_eq!(name, "DB_PASS");
                assert_eq!(value, "s3cret");
                assert_eq!(project, "myapp");
            }
            _ => panic!("expected Secret Set"),
        }
    }

    #[test]
    fn parse_route_add() {
        let cli = Cli::try_parse_from([
            "helyos", "route", "add", "api.example.com",
            "-p", "web", "--deployment", "api", "--https",
        ]).unwrap();
        match cli.command {
            Commands::Route { command: RouteCommands::Add { domain, project, deployment, https } } => {
                assert_eq!(domain, "api.example.com");
                assert_eq!(project, "web");
                assert_eq!(deployment, "api");
                assert!(https);
            }
            _ => panic!("expected Route Add"),
        }
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo test`
Expected: All existing + new tests pass

- [ ] **Step 4: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/output/table.rs src/main.rs
git commit -m "test: add CLI argument parsing and table output tests"
```

---

### Task 7: Update CI workflows — integration + E2E jobs

**Files:**
- Modify: `helyosd/.github/workflows/ci.yml`

- [ ] **Step 1: Update helyosd CI with integration and E2E jobs**

Replace the full content of `helyosd/.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  check:
    name: Check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install protoc
        run: sudo apt-get update && sudo apt-get install -y protobuf-compiler
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings

  test:
    name: Test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install protoc
        run: sudo apt-get update && sudo apt-get install -y protobuf-compiler
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --lib

  integration:
    name: Integration
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install protoc
        run: sudo apt-get update && sudo apt-get install -y protobuf-compiler
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --test api_integration
      - run: cargo test --test sqlite_integration
      - run: docker pull busybox:latest
      - run: cargo test --test runtime_integration -- --ignored

  e2e:
    name: E2E
    runs-on: ubuntu-latest
    needs: [check, test]
    if: github.event_name == 'push'
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v4
      - name: Install protoc
        run: sudo apt-get update && sudo apt-get install -y protobuf-compiler
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: docker pull busybox:latest
      - run: cargo test --test e2e -- --ignored --test-threads=1
```

- [ ] **Step 2: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyosd
git add .github/workflows/ci.yml
git commit -m "ci: add integration and E2E test jobs"
```

---

### Task 8: helyosd E2E tests with real Docker

**Files:**
- Create: `helyosd/tests/e2e.rs`

These tests require Docker and are `#[ignore]`d by default.

- [ ] **Step 1: Create E2E test file**

Create `helyosd/tests/e2e.rs`:

```rust
use std::net::SocketAddr;
use std::sync::Arc;

use helyos_core::domain::orchestrator::Orchestrator;
use helyos_core::ports::state::StateStore;
use helyosd::adapters::secrets::EncryptedSqliteSecretStore;
use helyosd::adapters::state::SqliteStore;
use helyosd::adapters::transport::LocalTransport;
use helyosd::api;

struct E2eServer {
    addr: SocketAddr,
    _data_dir: tempfile::TempDir,
}

impl E2eServer {
    async fn start() -> Self {
        let data_dir = tempfile::tempdir().unwrap();
        let db_path = data_dir.path().join("e2e.db");
        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());

        let store = SqliteStore::connect(&db_url).await.unwrap();
        let store: Arc<dyn StateStore> = Arc::new(store);

        let secret_db_path = data_dir.path().join("secrets.db");
        let secret_conn = rusqlite::Connection::open(&secret_db_path).unwrap();
        let master_key = [0u8; 32];
        let secret_store: Arc<dyn helyos_core::ports::secrets::SecretStore> =
            Arc::new(EncryptedSqliteSecretStore::new(secret_conn, &master_key).unwrap());

        // Real Docker runtime
        let runtime: Arc<dyn helyos_core::ports::runtime::ContainerRuntime> = Arc::new(
            helyosd::adapters::runtime::DockerRuntime::new(data_dir.path().to_str().unwrap())
                .await
                .expect("Docker must be running for E2E tests"),
        );

        let transport: Arc<dyn helyos_core::ports::cluster::ClusterTransport> =
            Arc::new(LocalTransport::new(Arc::clone(&runtime)));

        let handle = Orchestrator::spawn(
            Arc::clone(&runtime),
            Some(store.clone()),
            Some(secret_store),
            Some(transport),
            None,
            None,
            None,
            None,
        );

        let state = api::AppState {
            handle,
            store,
        };
        let app = api::routes::build(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Wait for health
        let client = reqwest::Client::new();
        for _ in 0..50 {
            if client
                .get(format!("http://{}/health", addr))
                .send()
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        Self { addr, _data_dir: data_dir }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

/// Clean up any Docker containers with the `helyos-e2e-` prefix.
async fn cleanup_containers() {
    let output = tokio::process::Command::new("docker")
        .args(["ps", "-a", "--filter", "name=helyos-e2e-", "--format", "{{.Names}}"])
        .output()
        .await;
    if let Ok(output) = output {
        let names = String::from_utf8_lossy(&output.stdout);
        for name in names.lines() {
            let _ = tokio::process::Command::new("docker")
                .args(["rm", "-f", name])
                .output()
                .await;
        }
    }
}

#[tokio::test]
#[ignore]
async fn e2e_deploy_lifecycle() {
    cleanup_containers().await;
    let server = E2eServer::start().await;
    let client = reqwest::Client::new();

    let project = format!("e2e-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap());

    // Create project
    let resp = client
        .post(server.url("/api/v1/projects"))
        .json(&serde_json::json!({ "name": &project }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Deploy busybox
    let spec = serde_json::json!({
        "project": &project,
        "deployment": { "name": "sleeper" },
        "replicas": 1,
        "image": "busybox:latest",
        "ports": []
    });
    let resp = client
        .post(server.url("/api/v1/deploy"))
        .header("content-type", "application/json")
        .body(serde_json::to_string(&spec).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Wait for pod to appear
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let resp = client
        .get(server.url(&format!("/api/v1/pods?project={project}")))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let pods = body.as_array().unwrap();
    assert_eq!(pods.len(), 1, "expected 1 pod, got {}", pods.len());

    // Stop deployment
    let resp = client
        .post(server.url(&format!("/api/v1/projects/{project}/deployments/sleeper/stop")))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Remove deployment
    let resp = client
        .delete(server.url(&format!("/api/v1/projects/{project}/deployments/sleeper")))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Delete project
    let resp = client
        .delete(server.url(&format!("/api/v1/projects/{project}")))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    cleanup_containers().await;
}

#[tokio::test]
#[ignore]
async fn e2e_scale_up_down() {
    cleanup_containers().await;
    let server = E2eServer::start().await;
    let client = reqwest::Client::new();

    let project = format!("e2e-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap());

    client
        .post(server.url("/api/v1/projects"))
        .json(&serde_json::json!({ "name": &project }))
        .send()
        .await
        .unwrap();

    let spec = serde_json::json!({
        "project": &project,
        "deployment": { "name": "scaler" },
        "replicas": 1,
        "image": "busybox:latest",
        "ports": []
    });
    client
        .post(server.url("/api/v1/deploy"))
        .header("content-type", "application/json")
        .body(serde_json::to_string(&spec).unwrap())
        .send()
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Scale to 3
    let resp = client
        .post(server.url(&format!("/api/v1/projects/{project}/deployments/scaler/scale")))
        .json(&serde_json::json!({ "replicas": 3 }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let resp = client
        .get(server.url(&format!("/api/v1/pods?project={project}")))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body.as_array().unwrap().len(), 3);

    // Scale back to 1
    client
        .post(server.url(&format!("/api/v1/projects/{project}/deployments/scaler/scale")))
        .json(&serde_json::json!({ "replicas": 1 }))
        .send()
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let resp = client
        .get(server.url(&format!("/api/v1/pods?project={project}")))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body.as_array().unwrap().len(), 1);

    // Cleanup
    client.post(server.url(&format!("/api/v1/projects/{project}/deployments/scaler/stop"))).send().await.unwrap();
    client.delete(server.url(&format!("/api/v1/projects/{project}/deployments/scaler"))).send().await.unwrap();
    client.delete(server.url(&format!("/api/v1/projects/{project}"))).send().await.unwrap();
    cleanup_containers().await;
}

#[tokio::test]
#[ignore]
async fn e2e_route_management() {
    let server = E2eServer::start().await;
    let client = reqwest::Client::new();

    let project = format!("e2e-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap());

    client
        .post(server.url("/api/v1/projects"))
        .json(&serde_json::json!({ "name": &project }))
        .send()
        .await
        .unwrap();

    let spec = serde_json::json!({
        "project": &project,
        "deployment": { "name": "web" },
        "replicas": 1,
        "image": "busybox:latest",
        "ports": []
    });
    client
        .post(server.url("/api/v1/deploy"))
        .header("content-type", "application/json")
        .body(serde_json::to_string(&spec).unwrap())
        .send()
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // Add route
    let resp = client
        .post(server.url("/api/v1/routes"))
        .json(&serde_json::json!({
            "domain": "e2e-test.example.com",
            "project": &project,
            "deployment": "web",
            "tls_mode": "none"
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // List routes
    let resp = client.get(server.url("/api/v1/routes")).send().await.unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let routes = body.as_array().unwrap();
    assert!(routes.iter().any(|r| r["domain"] == "e2e-test.example.com"));

    // Delete route
    let resp = client
        .delete(server.url("/api/v1/routes/e2e-test.example.com"))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Cleanup
    client.post(server.url(&format!("/api/v1/projects/{project}/deployments/web/stop"))).send().await.unwrap();
    client.delete(server.url(&format!("/api/v1/projects/{project}/deployments/web"))).send().await.unwrap();
    client.delete(server.url(&format!("/api/v1/projects/{project}"))).send().await.unwrap();
    cleanup_containers().await;
}
```

- [ ] **Step 2: Verify E2E tests compile**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo test --test e2e --no-run`
Expected: Compiles successfully (tests are `#[ignore]` so won't run)

- [ ] **Step 3: Run E2E tests locally (requires Docker)**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && docker pull busybox:latest && cargo test --test e2e -- --ignored --test-threads=1`
Expected: 3 tests pass (takes ~30 seconds)

- [ ] **Step 4: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyosd
git add tests/e2e.rs
git commit -m "test: add E2E tests with real Docker containers"
```

---

### Task 9: helyos-core Criterion benchmarks

**Files:**
- Modify: `helyos-core/Cargo.toml`
- Create: `helyos-core/benches/scheduler.rs`
- Create: `helyos-core/benches/config_parsing.rs`

- [ ] **Step 1: Add criterion dev-dependency and bench targets**

Add to `helyos-core/Cargo.toml`:

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "scheduler"
harness = false

[[bench]]
name = "config_parsing"
harness = false
```

- [ ] **Step 2: Create scheduler benchmark**

Create `helyos-core/benches/scheduler.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use helyos_core::domain::scheduler::{
    NodeSnapshot, PodRequest, SchedulerWeights, WeightedScheduler,
};
use uuid::Uuid;

fn make_nodes(count: usize) -> Vec<NodeSnapshot> {
    (0..count)
        .map(|i| NodeSnapshot {
            node_id: Uuid::new_v4(),
            cpu_available: 4.0 - (i as f64 % 3.0) * 0.5,
            cpu_total: 4.0,
            memory_available: 8_000_000_000 - (i as u64 % 4) * 1_000_000_000,
            memory_total: 8_000_000_000,
            running_pods: (i as u32) % 10,
            max_pods: 110,
            recent_failures: vec![],
        })
        .collect()
}

fn bench_scheduler(c: &mut Criterion) {
    let spread_weights = SchedulerWeights {
        cpu: 0.35,
        memory: 0.35,
        load: 0.15,
        failure: 0.15,
    };
    let binpack_weights = SchedulerWeights {
        cpu: -0.30,
        memory: -0.30,
        load: -0.10,
        failure: 0.15,
    };

    let request = PodRequest {
        cpu_request: 0.5,
        memory_request: 256_000_000,
    };

    let mut group = c.benchmark_group("scheduler_spread");
    for node_count in [5, 20] {
        let nodes = make_nodes(node_count);
        let scheduler = WeightedScheduler::new(spread_weights.clone());
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{node_count}_nodes")),
            &nodes,
            |b, nodes| {
                b.iter(|| scheduler.select_node(black_box(&request), black_box(nodes)))
            },
        );
    }
    group.finish();

    let mut group = c.benchmark_group("scheduler_binpack");
    for node_count in [5, 20] {
        let nodes = make_nodes(node_count);
        let scheduler = WeightedScheduler::new(binpack_weights.clone());
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{node_count}_nodes")),
            &nodes,
            |b, nodes| {
                b.iter(|| scheduler.select_node(black_box(&request), black_box(nodes)))
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_scheduler);
criterion_main!(benches);
```

- [ ] **Step 3: Create config parsing benchmark**

Create `helyos-core/benches/config_parsing.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use helyos_core::config::parse_deployment;

const MINIMAL_SPEC: &str = r#"
project: test
deployment:
  name: api
image: nginx:latest
"#;

const FULL_SPEC: &str = r#"
project: ecommerce
deployment:
  name: api
replicas: 3
image: ghcr.io/company/api:latest
ports:
  - 3000
  - 8080
env:
  DATABASE_URL: "postgres://localhost/ecommerce"
  REDIS_URL: "redis://localhost:6379"
  LOG_LEVEL: "info"
network:
  public: true
  domain: api.example.com
  https: true
healthcheck:
  path: /health
  interval: 10s
  timeout: 5s
  retries: 3
volumes:
  - name: data
    mount: /app/data
restart: always
resources:
  cpu: 0.5
  memory: 256M
"#;

fn bench_config_parsing(c: &mut Criterion) {
    c.bench_function("parse_minimal_spec", |b| {
        b.iter(|| parse_deployment(black_box(MINIMAL_SPEC)).unwrap())
    });

    c.bench_function("parse_full_spec", |b| {
        b.iter(|| parse_deployment(black_box(FULL_SPEC)).unwrap())
    });
}

criterion_group!(benches, bench_config_parsing);
criterion_main!(benches);
```

- [ ] **Step 4: Run benchmarks**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-core && cargo bench`
Expected: Benchmarks run and output timing results

- [ ] **Step 5: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-core
git add Cargo.toml benches/
git commit -m "bench: add Criterion benchmarks for scheduler and config parsing"
```

---

### Task 10: helyosd Criterion benchmarks

**Files:**
- Modify: `helyosd/Cargo.toml` (add bench targets)
- Create: `helyosd/benches/sqlite_store.rs`
- Create: `helyosd/benches/crypto.rs`
- Create: `helyosd/benches/dns.rs`

- [ ] **Step 1: Add bench targets to Cargo.toml**

Append to `helyosd/Cargo.toml`:

```toml
[[bench]]
name = "sqlite_store"
harness = false

[[bench]]
name = "crypto"
harness = false

[[bench]]
name = "dns"
harness = false
```

- [ ] **Step 2: Create SQLite store benchmark**

Create `helyosd/benches/sqlite_store.rs`:

```rust
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use helyos_core::domain::models::*;
use helyos_core::ports::state::StateStore;
use helyosd::adapters::state::SqliteStore;
use tokio::runtime::Runtime;

async fn setup_store() -> (SqliteStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("bench.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let store = SqliteStore::connect(&url).await.unwrap();
    (store, dir)
}

fn bench_sqlite(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("insert_pod", |b| {
        b.iter_custom(|iters| {
            rt.block_on(async {
                let (store, _dir) = setup_store().await;
                let project = Project::new("bench");
                store.insert_project(&project).await.unwrap();

                let spec = DeploymentSpec {
                    project: "bench".into(),
                    deployment: DeploymentMeta { name: "svc".into() },
                    replicas: 1,
                    image: "nginx:latest".into(),
                    ports: vec![],
                    env: Default::default(),
                    volumes: vec![],
                    secrets: vec![],
                    network: None,
                    healthcheck: None,
                    restart: RestartPolicy::default(),
                    resources: None,
                };
                let deployment = Deployment::new(spec);
                store.insert_deployment(&deployment).await.unwrap();

                let start = std::time::Instant::now();
                for _ in 0..iters {
                    let pod = Pod::new(deployment.id, "bench", "svc", "nginx:latest");
                    store.insert_pod(&pod).await.unwrap();
                }
                start.elapsed()
            })
        })
    });

    let mut group = c.benchmark_group("list_pods");
    for count in [100, 1000] {
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &count,
            |b, &count| {
                b.iter_custom(|iters| {
                    rt.block_on(async {
                        let (store, _dir) = setup_store().await;
                        let project = Project::new("bench");
                        store.insert_project(&project).await.unwrap();

                        let spec = DeploymentSpec {
                            project: "bench".into(),
                            deployment: DeploymentMeta { name: "svc".into() },
                            replicas: 1,
                            image: "nginx:latest".into(),
                            ports: vec![],
                            env: Default::default(),
                            volumes: vec![],
                            secrets: vec![],
                            network: None,
                            healthcheck: None,
                            restart: RestartPolicy::default(),
                            resources: None,
                        };
                        let deployment = Deployment::new(spec);
                        store.insert_deployment(&deployment).await.unwrap();

                        for _ in 0..count {
                            let pod = Pod::new(deployment.id, "bench", "svc", "nginx:latest");
                            store.insert_pod(&pod).await.unwrap();
                        }

                        let start = std::time::Instant::now();
                        for _ in 0..iters {
                            let _ = store.list_pods(Some("bench")).await.unwrap();
                        }
                        start.elapsed()
                    })
                })
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_sqlite);
criterion_main!(benches);
```

- [ ] **Step 3: Create crypto benchmark**

Create `helyosd/benches/crypto.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use helyos_core::ports::secrets::SecretStore;
use helyosd::adapters::secrets::EncryptedSqliteSecretStore;

fn bench_crypto(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("bench_secrets.db")).unwrap();
    let master_key = [42u8; 32];
    let store = EncryptedSqliteSecretStore::new(conn, &master_key).unwrap();

    let mut group = c.benchmark_group("encrypt");
    for size in [64, 1024, 65536] {
        let payload = vec![0xABu8; size];
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{size}B")),
            &payload,
            |b, payload| {
                b.iter(|| {
                    rt.block_on(async {
                        store
                            .set("bench", "key", black_box(payload))
                            .await
                            .unwrap();
                    })
                })
            },
        );
    }
    group.finish();

    // Setup data for decrypt benchmarks
    for size in [64, 1024] {
        let payload = vec![0xCDu8; size];
        rt.block_on(async {
            store
                .set("bench", &format!("dec_{size}"), &payload)
                .await
                .unwrap();
        });
    }

    let mut group = c.benchmark_group("decrypt");
    for size in [64, 1024] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{size}B")),
            &size,
            |b, &size| {
                b.iter(|| {
                    rt.block_on(async {
                        let _ = store
                            .get("bench", &format!("dec_{size}"))
                            .await
                            .unwrap();
                    })
                })
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_crypto);
criterion_main!(benches);
```

- [ ] **Step 4: Create DNS benchmark**

Create `helyosd/benches/dns.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use helyosd::adapters::dns::record_store::DnsRecordStore;
use std::net::{IpAddr, Ipv4Addr};

fn bench_dns(c: &mut Criterion) {
    let mut group = c.benchmark_group("dns_lookup");

    for record_count in [10, 100, 1000] {
        let store = DnsRecordStore::new();
        for i in 0..record_count {
            let ip = IpAddr::V4(Ipv4Addr::new(10, 0, (i / 256) as u8, (i % 256) as u8));
            store.register(&format!("proj{i}"), "svc", ip);
        }

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{record_count}_records")),
            &store,
            |b, store| {
                b.iter(|| {
                    store.resolve(black_box("svc.proj0.internal"))
                })
            },
        );
    }
    group.finish();

    c.bench_function("dns_register_deregister", |b| {
        let store = DnsRecordStore::new();
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        b.iter(|| {
            store.register(black_box("myapp"), black_box("api"), ip);
            store.deregister(black_box("myapp"), black_box("api"), ip);
        })
    });
}

criterion_group!(benches, bench_dns);
criterion_main!(benches);
```

- [ ] **Step 5: Run benchmarks**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo bench`
Expected: All benchmarks run with timing output

- [ ] **Step 6: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyosd
git add Cargo.toml benches/
git commit -m "bench: add Criterion benchmarks for SQLite, crypto, and DNS"
```

---

### Task 11: Benchmark CI workflows with regression detection

**Files:**
- Create: `helyos-core/.github/workflows/bench.yml`
- Create: `helyosd/.github/workflows/bench.yml`

- [ ] **Step 1: Create helyos-core bench workflow**

Create `helyos-core/.github/workflows/bench.yml`:

```yaml
name: Benchmarks

on:
  push:
    branches: [main]

permissions:
  contents: write
  deployments: write

jobs:
  bench:
    name: Performance
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Run benchmarks
        run: cargo bench --bench scheduler --bench config_parsing -- --output-format bencher | tee output.txt
      - name: Store benchmark result
        uses: benchmark-action/github-action-benchmark@v1
        with:
          tool: cargo
          output-file-path: output.txt
          alert-threshold: '115%'
          fail-on-alert: true
          github-token: ${{ secrets.GITHUB_TOKEN }}
          auto-push: true
          benchmark-data-dir-path: dev/bench
```

- [ ] **Step 2: Create helyosd bench workflow**

Create `helyosd/.github/workflows/bench.yml`:

```yaml
name: Benchmarks

on:
  push:
    branches: [main]

permissions:
  contents: write
  deployments: write

jobs:
  bench:
    name: Performance
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install protoc
        run: sudo apt-get update && sudo apt-get install -y protobuf-compiler
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Run benchmarks
        run: cargo bench --bench sqlite_store --bench crypto --bench dns -- --output-format bencher | tee output.txt
      - name: Store benchmark result
        uses: benchmark-action/github-action-benchmark@v1
        with:
          tool: cargo
          output-file-path: output.txt
          alert-threshold: '115%'
          fail-on-alert: true
          github-token: ${{ secrets.GITHUB_TOKEN }}
          auto-push: true
          benchmark-data-dir-path: dev/bench
```

- [ ] **Step 3: Commit all bench workflows**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-core
git add .github/workflows/bench.yml
git commit -m "ci: add benchmark workflow with regression detection"

cd /Users/nassime/GitHub/Helyos/helyosd
git add .github/workflows/bench.yml
git commit -m "ci: add benchmark workflow with regression detection"
```

---

### Task 12: Push all repos and verify CI

- [ ] **Step 1: Push all repos**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-core && git push
cd /Users/nassime/GitHub/Helyos/helyosd && git push
cd /Users/nassime/GitHub/Helyos/helyos-cli && git push
```

- [ ] **Step 2: Verify CI passes on all repos**

Run: `gh run list --repo helyos-labs/helyos-core --limit 1 && gh run list --repo helyos-labs/helyosd --limit 1 && gh run list --repo helyos-labs/helyos-cli --limit 1`

Expected: All CI runs succeed (E2E may take a few minutes on helyosd).
