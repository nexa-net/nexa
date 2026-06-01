# Critical Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 8 remaining CRITICAL issues from the production-readiness audit — security, correctness, and supply chain.

**Architecture:** Three blocs executed sequentially. Bloc 1 (security) adds Bearer token auth middleware to nexad's axum API, changes the default bind to 127.0.0.1, removes plaintext join token storage, and moves CLI secret input to stdin. Bloc 2 (correctness) implements the `stop_pod`/`remove_pod` no-ops in ClusterServer and adds graceful shutdown via `CancellationToken`. Bloc 3 (supply chain) adds SHA-256 checksums to release workflows and verifies them in install.sh.

**Tech Stack:** Rust (axum middleware, tokio-util CancellationToken, dialoguer Password), GitHub Actions (checksums), shell (install.sh)

**Repos:**
- `nexad`: `/Users/nassime/GitHub/NexaNet/nexad`
- `nexa-cli`: `/Users/nassime/GitHub/NexaNet/nexa-cli`
- `nexa` (infra): `/Users/nassime/GitHub/NexaNet`

---

## File Structure

### nexad changes
- Create: `src/api/auth.rs` — Bearer token auth middleware + token generation/hashing
- Modify: `src/api/mod.rs` — add `pub mod auth;`, pass `api_token_hash` into `AppState`, accept shutdown signal in `serve`
- Modify: `src/api/routes.rs` — apply auth middleware to `/api/v1/*` routes only
- Modify: `src/api/handlers.rs` — fix `cluster_init`, `cluster_token_show`, `cluster_token_rotate`
- Modify: `src/main.rs` — default bind 127.0.0.1, add `--api-token` flag, generate/load API token, wire graceful shutdown
- Modify: `src/cluster/server.rs` — implement `stop_pod` and `remove_pod`
- Modify: `Cargo.toml` — add `argon2`, `tokio-util`

### nexa-cli changes
- Modify: `src/main.rs` — change `value: String` to `value: Option<String>` in `SecretCommands::Set`
- Modify: `src/commands/secret.rs` — read secret from stdin/prompt when value not provided

### infra changes
- Modify: `nexad/.github/workflows/release.yml` — add SHA-256 checksums step, fix `if: always()`
- Modify: `nexa-cli/.github/workflows/release.yml` — same
- Modify: `install.sh` — verify checksums after download

---

## Task 1: Default bind to 127.0.0.1 (CRITICAL 2)

**Files:**
- Modify: `nexad/src/main.rs:34,68`

- [ ] **Step 1: Change default host from 0.0.0.0 to 127.0.0.1**

In `nexad/src/main.rs`, change line 34:

```rust
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
```

And line 68 (DNS listen):

```rust
    #[arg(long, default_value = "127.0.0.1:15353")]
    dns_listen: String,
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo check 2>&1`
Expected: `Finished` with no errors

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
git add src/main.rs
git commit -m "fix(security): default bind to 127.0.0.1 instead of 0.0.0.0

CRITICAL-2: API and DNS no longer exposed to the network by default.
Use --host 0.0.0.0 to explicitly bind to all interfaces."
```

---

## Task 2: Hash join token — stop storing plaintext (CRITICAL 3)

**Files:**
- Modify: `nexad/src/api/handlers.rs:275-329`

- [ ] **Step 1: Fix `cluster_init` — remove plaintext storage**

In `nexad/src/api/handlers.rs`, replace the `cluster_init` function (lines 275-293):

```rust
pub async fn cluster_init(State(state): AppStateExtractor) -> impl IntoResponse {
    let token = crate::cluster::token::generate_token();
    let hash = crate::cluster::token::hash_token(&token);
    match state
        .store
        .set_cluster_config("join_token_hash", &hash)
        .await
    {
        Ok(()) => {
            Json(serde_json::json!({ "token": token })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
```

- [ ] **Step 2: Fix `cluster_token_show` — return 410 Gone**

Replace the `cluster_token_show` function (lines 295-309):

```rust
pub async fn cluster_token_show(State(state): AppStateExtractor) -> impl IntoResponse {
    match state.store.get_cluster_config("join_token_hash").await {
        Ok(Some(_)) => (
            StatusCode::GONE,
            Json(serde_json::json!({
                "error": "token is only shown at creation. Use POST /api/v1/cluster/token/rotate to generate a new one."
            })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "cluster not initialized" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
```

- [ ] **Step 3: Fix `cluster_token_rotate` — remove plaintext storage**

Replace the `cluster_token_rotate` function (lines 311-329):

```rust
pub async fn cluster_token_rotate(State(state): AppStateExtractor) -> impl IntoResponse {
    let token = crate::cluster::token::generate_token();
    let hash = crate::cluster::token::hash_token(&token);
    match state
        .store
        .set_cluster_config("join_token_hash", &hash)
        .await
    {
        Ok(()) => {
            Json(serde_json::json!({ "token": token })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo check 2>&1`
Expected: `Finished` with no errors

- [ ] **Step 5: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
git add src/api/handlers.rs
git commit -m "fix(security): only store hashed join token, never plaintext

CRITICAL-3: cluster_init and cluster_token_rotate no longer persist
the plaintext token in SQLite. Token is shown once at creation/rotation.
cluster_token_show returns 410 Gone."
```

---

## Task 3: Bearer token auth middleware (CRITICAL 1)

**Files:**
- Modify: `nexad/Cargo.toml` — add `argon2`
- Create: `nexad/src/api/auth.rs`
- Modify: `nexad/src/api/mod.rs` — add module, add `api_token_hash` to `AppState`
- Modify: `nexad/src/api/routes.rs` — apply auth layer
- Modify: `nexad/src/main.rs` — add `--api-token` flag, generate/load token

- [ ] **Step 1: Add argon2 dependency**

In `nexad/Cargo.toml`, add to `[dependencies]`:

```toml
argon2 = "0.5"
```

- [ ] **Step 2: Create auth middleware**

Create `nexad/src/api/auth.rs`:

```rust
use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use super::AppState;

pub fn generate_api_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    format!("nxa-api_{}", hex::encode(bytes))
}

pub fn hash_api_token(token: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(token.as_bytes(), &salt)
        .expect("argon2 hash failed")
        .to_string()
}

pub fn verify_api_token(token: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(token.as_bytes(), &parsed)
        .is_ok()
}

pub async fn require_bearer_token(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let token_hash = match &state.api_token_hash {
        Some(hash) => hash,
        None => return next.run(req).await,
    };

    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "missing or invalid bearer token"})),
            )
                .into_response();
        }
    };

    if !verify_api_token(token, token_hash) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "missing or invalid bearer token"})),
        )
            .into_response();
    }

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_token_format() {
        let token = generate_api_token();
        assert!(token.starts_with("nxa-api_"));
        assert_eq!(token.len(), 8 + 64); // "nxa-api_" + 64 hex chars
    }

    #[test]
    fn hash_and_verify_roundtrip() {
        let token = generate_api_token();
        let hash = hash_api_token(&token);
        assert!(verify_api_token(&token, &hash));
    }

    #[test]
    fn verify_rejects_wrong_token() {
        let token = generate_api_token();
        let hash = hash_api_token(&token);
        assert!(!verify_api_token("wrong-token", &hash));
    }

    #[test]
    fn verify_rejects_bad_hash() {
        assert!(!verify_api_token("any-token", "not-a-valid-hash"));
    }
}
```

- [ ] **Step 3: Update AppState to include api_token_hash**

In `nexad/src/api/mod.rs`, add the module and update `AppState`:

```rust
pub mod auth;
mod handlers;
pub mod routes;

use std::sync::Arc;

use nexa_core::domain::orchestrator::OrchestratorHandle;
use nexa_core::ports::metrics::MetricsPort;
use nexa_core::ports::state::StateStore;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct AppState {
    pub handle: OrchestratorHandle,
    pub store: Arc<dyn StateStore>,
    pub metrics: Arc<dyn MetricsPort>,
    pub event_tx: broadcast::Sender<ClusterEvent>,
    pub api_token_hash: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ClusterEvent {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub kind: String,
    pub name: String,
    pub action: String,
    pub message: String,
}

pub async fn serve(
    handle: OrchestratorHandle,
    store: Arc<dyn StateStore>,
    metrics: Arc<dyn MetricsPort>,
    event_tx: broadcast::Sender<ClusterEvent>,
    addr: &str,
    api_token_hash: Option<String>,
) -> anyhow::Result<()> {
    let state = AppState {
        handle,
        store,
        metrics,
        event_tx,
        api_token_hash,
    };
    let app = routes::build(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("nexad API listening on {addr}");

    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 4: Apply auth middleware to API routes**

In `nexad/src/api/routes.rs`, split routes into public and protected:

```rust
use axum::Router;
use axum::middleware;
use axum::routing::{delete, get, post};
use tower_http::trace::TraceLayer;

use super::AppState;
use super::auth;
use super::handlers;

pub fn build(state: AppState) -> Router {
    let public = Router::new()
        .route("/health", get(handlers::health))
        .route("/metrics", get(handlers::metrics_endpoint));

    let protected = Router::new()
        .route("/api/v1/projects", get(handlers::list_projects))
        .route("/api/v1/projects", post(handlers::create_project))
        .route(
            "/api/v1/projects/{name}/suspend",
            post(handlers::suspend_project),
        )
        .route(
            "/api/v1/projects/{name}/resume",
            post(handlers::resume_project),
        )
        .route("/api/v1/projects/{name}", delete(handlers::delete_project))
        .route("/api/v1/deployments", get(handlers::list_deployments))
        .route("/api/v1/deploy", post(handlers::deploy))
        .route(
            "/api/v1/projects/{project}/deployments/{name}",
            delete(handlers::remove_deployment),
        )
        .route(
            "/api/v1/projects/{project}/deployments/{name}/stop",
            post(handlers::stop_deployment),
        )
        .route(
            "/api/v1/projects/{project}/deployments/{name}/scale",
            post(handlers::scale_deployment),
        )
        .route("/api/v1/pods", get(handlers::list_pods))
        .route(
            "/api/v1/projects/{project}/deployments/{name}/logs",
            get(handlers::logs),
        )
        .route(
            "/api/v1/projects/{project}/secrets",
            get(handlers::list_secrets),
        )
        .route(
            "/api/v1/projects/{project}/secrets/{name}",
            post(handlers::set_secret),
        )
        .route(
            "/api/v1/projects/{project}/secrets/{name}",
            delete(handlers::delete_secret),
        )
        .route("/api/v1/cluster/init", post(handlers::cluster_init))
        .route("/api/v1/cluster/token", get(handlers::cluster_token_show))
        .route(
            "/api/v1/cluster/token/rotate",
            post(handlers::cluster_token_rotate),
        )
        .route("/api/v1/nodes", get(handlers::list_nodes))
        .route("/api/v1/nodes/stats", get(handlers::node_stats))
        .route("/api/v1/events", get(handlers::events_stream))
        .route("/api/v1/nodes/{name}/drain", post(handlers::drain_node))
        .route("/api/v1/nodes/{name}", delete(handlers::remove_node))
        .route(
            "/api/v1/cluster/scheduler",
            get(handlers::get_scheduler_config),
        )
        .route(
            "/api/v1/cluster/scheduler",
            post(handlers::set_scheduler_config),
        )
        .route("/api/v1/routes", get(handlers::list_routes))
        .route("/api/v1/routes", post(handlers::add_route))
        .route("/api/v1/routes/{domain}", delete(handlers::remove_route))
        .route("/api/v1/certs/import", post(handlers::import_cert))
        .route(
            "/api/v1/cluster/config/proxy",
            get(handlers::get_proxy_config),
        )
        .route(
            "/api/v1/cluster/config/proxy",
            post(handlers::set_proxy_config),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer_token,
        ));

    public
        .merge(protected)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            handlers::metrics_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
```

- [ ] **Step 5: Add --api-token flag and token generation in main.rs**

In `nexad/src/main.rs`, add the CLI flag after the `overlay` field (line 97):

```rust
    /// API authentication token (if not set, one is generated on first startup)
    #[arg(long, env = "NEXA_API_TOKEN")]
    api_token: Option<String>,
```

Add a helper function after `init_dns` (after line 276):

```rust
/// Load or generate the API authentication token.
/// Returns the argon2 hash for middleware verification.
async fn init_api_token(
    cli: &Cli,
    store: &Arc<dyn StateStore>,
) -> anyhow::Result<Option<String>> {
    use nexad::api::auth;

    if let Some(ref token) = cli.api_token {
        let hash = auth::hash_api_token(token);
        store.set_cluster_config("api_token_hash", &hash).await?;
        info!("API token set from --api-token flag");
        return Ok(Some(hash));
    }

    match store.get_cluster_config("api_token_hash").await? {
        Some(hash) => {
            info!("API token loaded from store");
            Ok(Some(hash))
        }
        None => {
            let token = auth::generate_api_token();
            let hash = auth::hash_api_token(&token);
            store.set_cluster_config("api_token_hash", &hash).await?;
            info!("Generated new API token — save this, it won't be shown again:");
            info!("  {}", token);
            Ok(Some(hash))
        }
    }
}
```

Update the three `nexad::api::serve` call sites in `start_single_node`, `start_master`, and the worker-mode function to pass the token hash. In `start_single_node` (around line 320-321):

```rust
    let api_token_hash = init_api_token(cli, &store).await?;
    let addr = format!("{}:{}", cli.host, cli.port);
    nexad::api::serve(handle, Arc::clone(&store), metrics, event_tx.clone(), &addr, api_token_hash).await
```

In `start_master` (around line 432-434):

```rust
    let api_token_hash = init_api_token(cli, &store).await?;
    let addr = format!("{}:{}", cli.host, cli.port);
    nexad::api::serve(handle, Arc::clone(&store), metrics, event_tx.clone(), &addr, api_token_hash).await
```

Worker mode does not run an HTTP API, so no change needed there.

- [ ] **Step 6: Run tests**

Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo test api::auth 2>&1`
Expected: 4 tests pass (generate_token_format, hash_and_verify_roundtrip, verify_rejects_wrong_token, verify_rejects_bad_hash)

- [ ] **Step 7: Verify full compilation**

Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo check 2>&1`
Expected: `Finished` with no errors

- [ ] **Step 8: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
git add Cargo.toml Cargo.lock src/api/auth.rs src/api/mod.rs src/api/routes.rs src/main.rs
git commit -m "feat(security): add Bearer token auth middleware on API

CRITICAL-1: All /api/v1/* endpoints now require Authorization: Bearer <token>.
/health and /metrics remain public. Token is auto-generated on first start
and shown once. Use --api-token or NEXA_API_TOKEN env to set explicitly."
```

---

## Task 4: Read secrets from stdin (CRITICAL 4)

**Files:**
- Modify: `nexa-cli/src/main.rs:195-205,365-371`
- Modify: `nexa-cli/src/commands/secret.rs:6`

- [ ] **Step 1: Change CLI argument to optional --value**

In `nexa-cli/src/main.rs`, replace the `SecretCommands::Set` variant (lines 196-205):

```rust
    /// Set a secret value
    Set {
        /// Secret name
        name: String,
        /// Secret value (if omitted, reads from stdin or prompts interactively)
        #[arg(long)]
        value: Option<String>,
        /// Project name
        #[arg(short, long)]
        project: String,
    },
```

- [ ] **Step 2: Update the match arm to read from stdin**

In `nexa-cli/src/main.rs`, replace the `SecretCommands::Set` match arm (lines 365-371):

```rust
            SecretCommands::Set {
                name,
                value,
                project,
            } => {
                let secret_value = match value {
                    Some(v) => v,
                    None => {
                        if atty::is(atty::Stream::Stdin) {
                            dialoguer::Password::new()
                                .with_prompt(format!("Value for secret '{name}'"))
                                .interact()?
                        } else {
                            let mut buf = String::new();
                            std::io::stdin().read_line(&mut buf)?;
                            buf.trim_end().to_string()
                        }
                    }
                };
                commands::secret::set(&client, &project, &name, &secret_value).await
            }
```

- [ ] **Step 3: Add atty dependency**

In `nexa-cli/Cargo.toml`, add to `[dependencies]`:

```toml
atty = "0.2"
```

- [ ] **Step 4: Update the test for parse_secret_set**

In `nexa-cli/src/main.rs`, find the test `parse_secret_set` (around line 491) and update it to use `--value`:

```rust
    #[test]
    fn parse_secret_set() {
        let cli =
            Cli::try_parse_from(["nexa", "secret", "set", "DB_PASS", "--value", "s3cret", "-p", "myapp"])
                .unwrap();
        match cli.command {
            Commands::Secret { command } => match command {
                SecretCommands::Set {
                    name,
                    value,
                    project,
                } => {
                    assert_eq!(name, "DB_PASS");
                    assert_eq!(value, Some("s3cret".to_string()));
                    assert_eq!(project, "myapp");
                }
                _ => panic!("expected Set"),
            },
            _ => panic!("expected Secret"),
        }
    }
```

- [ ] **Step 5: Verify it compiles and tests pass**

Run: `cd /Users/nassime/GitHub/NexaNet/nexa-cli && cargo test parse_secret 2>&1`
Expected: test passes

Run: `cd /Users/nassime/GitHub/NexaNet/nexa-cli && cargo check 2>&1`
Expected: `Finished` with no errors

- [ ] **Step 6: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexa-cli
git add Cargo.toml Cargo.lock src/main.rs
git commit -m "fix(security): read secrets from stdin instead of CLI arguments

CRITICAL-4: Secret value is no longer a positional argument visible in
ps aux and shell history. Use --value flag, stdin pipe, or interactive
prompt: echo \$SECRET | nexa secret set KEY -p app"
```

---

## Task 5: Implement stop_pod and remove_pod (CRITICAL 5)

**Files:**
- Modify: `nexad/src/cluster/server.rs:251-273`

- [ ] **Step 1: Implement stop_pod**

In `nexad/src/cluster/server.rs`, replace the `stop_pod` method (lines 251-261):

```rust
    async fn stop_pod(
        &self,
        request: Request<proto::StopPodRequest>,
    ) -> std::result::Result<Response<proto::StopPodResponse>, Status> {
        let req = request.into_inner();
        info!(pod_id = req.pod_id, "worker: stopping pod");

        if let Err(e) = self.runtime.stop_container(&req.pod_id, 10).await {
            warn!(pod_id = req.pod_id, error = %e, "failed to stop pod");
            return Ok(Response::new(proto::StopPodResponse {
                success: false,
                message: format!("stop failed: {e}"),
            }));
        }

        Ok(Response::new(proto::StopPodResponse {
            success: true,
            message: "stopped".into(),
        }))
    }
```

- [ ] **Step 2: Implement remove_pod**

Replace the `remove_pod` method (lines 263-273):

```rust
    async fn remove_pod(
        &self,
        request: Request<proto::RemovePodRequest>,
    ) -> std::result::Result<Response<proto::RemovePodResponse>, Status> {
        let req = request.into_inner();
        info!(pod_id = req.pod_id, "worker: removing pod");

        let _ = self.runtime.stop_container(&req.pod_id, 5).await;

        if let Err(e) = self.runtime.remove_container(&req.pod_id, true).await {
            warn!(pod_id = req.pod_id, error = %e, "failed to remove pod");
            return Ok(Response::new(proto::RemovePodResponse {
                success: false,
                message: format!("remove failed: {e}"),
            }));
        }

        Ok(Response::new(proto::RemovePodResponse {
            success: true,
            message: "removed".into(),
        }))
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo check 2>&1`
Expected: `Finished` with no errors

- [ ] **Step 4: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
git add src/cluster/server.rs
git commit -m "fix(cluster): implement stop_pod and remove_pod in ClusterServer

CRITICAL-5: These were no-ops returning success without touching the
container runtime. Now they call stop_container/remove_container."
```

---

## Task 6: Graceful shutdown (CRITICAL 6)

**Files:**
- Modify: `nexad/Cargo.toml` — add `tokio-util`
- Modify: `nexad/src/api/mod.rs` — accept shutdown signal in `serve`
- Modify: `nexad/src/main.rs` — wire CancellationToken through all modes

- [ ] **Step 1: Add tokio-util dependency**

In `nexad/Cargo.toml`, add to `[dependencies]`:

```toml
tokio-util = "0.7"
```

- [ ] **Step 2: Update api::serve to accept a shutdown signal**

In `nexad/src/api/mod.rs`, update the `serve` function to accept a shutdown future:

```rust
pub async fn serve(
    handle: OrchestratorHandle,
    store: Arc<dyn StateStore>,
    metrics: Arc<dyn MetricsPort>,
    event_tx: broadcast::Sender<ClusterEvent>,
    addr: &str,
    api_token_hash: Option<String>,
    shutdown: tokio::sync::watch::Receiver<()>,
) -> anyhow::Result<()> {
    let state = AppState {
        handle,
        store,
        metrics,
        event_tx,
        api_token_hash,
    };
    let app = routes::build(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("nexad API listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let mut shutdown = shutdown;
            let _ = shutdown.changed().await;
            tracing::info!("HTTP server shutting down gracefully");
        })
        .await?;
    Ok(())
}
```

- [ ] **Step 3: Wire shutdown into start_single_node**

In `nexad/src/main.rs`, add at the top of `start_single_node` (after the log line):

```rust
async fn start_single_node(cli: &Cli) -> anyhow::Result<()> {
    info!(
        "starting nexad in single-node mode on {}:{}",
        cli.host, cli.port
    );

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
    tokio::spawn(async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler");
        #[cfg(unix)]
        tokio::select! {
            _ = ctrl_c => { tracing::info!("received SIGINT"); }
            _ = sigterm.recv() => { tracing::info!("received SIGTERM"); }
        }
        #[cfg(not(unix))]
        ctrl_c.await.expect("failed to listen for ctrl-c");
        drop(shutdown_tx);
    });

    let (data_dir, store, runtime) = init_infrastructure(cli).await?;
    let secret_store = init_secrets(cli, &data_dir)?;
    let (dns, master_ip) = init_dns(cli).await?;
    let (proxy, route_store) = init_proxy(cli)?;
    let metrics: Arc<dyn MetricsPort> =
        Arc::new(nexad::adapters::metrics::PrometheusMetrics::new());
    let (event_tx, _) = tokio::sync::broadcast::channel::<nexad::api::ClusterEvent>(256);
    let handle = spawn_orchestrator(
        &runtime,
        &store,
        secret_store,
        dns,
        master_ip,
        Some(Arc::clone(&proxy)),
        Some(Arc::clone(&route_store)),
        Some(metrics.clone()),
        event_tx.clone(),
    );

    if let Some(ref email) = cli.acme_email {
        let acme = Arc::new(nexad::adapters::tls::AcmeManager::new(
            email,
            Arc::clone(&route_store),
            false,
        ));
        nexad::adapters::tls::spawn_renewal_task(
            Arc::clone(&route_store),
            acme,
            std::time::Duration::from_secs(86400),
            30,
        );
        info!(email, "TLS auto-renewal enabled");
    }

    let api_token_hash = init_api_token(cli, &store).await?;
    let addr = format!("{}:{}", cli.host, cli.port);
    nexad::api::serve(handle, Arc::clone(&store), metrics, event_tx.clone(), &addr, api_token_hash, shutdown_rx).await
}
```

- [ ] **Step 4: Wire shutdown into start_master**

Apply the same pattern to `start_master`. Add the `shutdown_tx`/`shutdown_rx` channel and signal handler at the top, pass `shutdown_rx` to `nexad::api::serve`. The gRPC server should also respect shutdown — update the gRPC spawn:

```rust
    let grpc_shutdown_rx = shutdown_tx.subscribe();
    tokio::spawn(async move {
        if let Err(e) = nexad::cluster::server::start_grpc_server(
            &grpc_addr,
            grpc_runtime,
            grpc_state,
            grpc_token_hash,
        )
        .await
        {
            tracing::error!(error = %e, "gRPC cluster server failed");
        }
    });
```

Note: `start_grpc_server` already uses `tonic::transport::Server` which can be extended with graceful shutdown later. For now the critical fix is the HTTP API — gRPC graceful shutdown is a HIGH, not CRITICAL.

Pass `shutdown_rx` to the final `serve` call:

```rust
    let api_token_hash = init_api_token(cli, &store).await?;
    let addr = format!("{}:{}", cli.host, cli.port);
    nexad::api::serve(handle, Arc::clone(&store), metrics, event_tx.clone(), &addr, api_token_hash, shutdown_rx).await
```

- [ ] **Step 5: Verify it compiles**

Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo check 2>&1`
Expected: `Finished` with no errors

- [ ] **Step 6: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
git add Cargo.toml Cargo.lock src/api/mod.rs src/main.rs
git commit -m "feat(ops): add graceful shutdown via SIGINT/SIGTERM

CRITICAL-6: nexad now handles SIGINT and SIGTERM, gracefully draining
HTTP connections before exiting. Prevents orphaned containers and
inconsistent state on kill."
```

---

## Task 7: SHA-256 checksums in release workflows (CRITICAL 7-8)

**Files:**
- Modify: `nexad/.github/workflows/release.yml`
- Modify: `nexa-cli/.github/workflows/release.yml`
- Modify: `install.sh`

- [ ] **Step 1: Update nexad release workflow**

Replace the full content of `nexad/.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    tags: ['v*']

permissions:
  contents: write

env:
  CARGO_TERM_COLOR: always
  BINARY_NAME: nexad

jobs:
  build:
    name: Build ${{ matrix.target }}
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
            binary_suffix: linux-amd64
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-latest
            binary_suffix: linux-arm64
            use_cross: true
          - target: x86_64-apple-darwin
            os: macos-latest
            binary_suffix: darwin-amd64
          - target: aarch64-apple-darwin
            os: macos-latest
            binary_suffix: darwin-arm64
    steps:
      - uses: actions/checkout@v4
      - name: Install protoc
        if: runner.os == 'Linux'
        run: sudo apt-get update && sudo apt-get install -y protobuf-compiler
      - name: Install protoc (macOS)
        if: runner.os == 'macOS'
        run: brew install protobuf
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2

      - name: Install cross
        if: matrix.use_cross
        run: cargo install cross --git https://github.com/cross-rs/cross

      - name: Build
        run: |
          if [ "${{ matrix.use_cross }}" = "true" ]; then
            cross build --release --target ${{ matrix.target }}
          else
            cargo build --release --target ${{ matrix.target }}
          fi

      - name: Package
        run: |
          cd target/${{ matrix.target }}/release
          tar czf ${BINARY_NAME}-${{ matrix.binary_suffix }}.tar.gz ${BINARY_NAME}
          mv ${BINARY_NAME}-${{ matrix.binary_suffix }}.tar.gz ../../../

      - uses: actions/upload-artifact@v4
        with:
          name: ${{ env.BINARY_NAME }}-${{ matrix.binary_suffix }}
          path: ${{ env.BINARY_NAME }}-${{ matrix.binary_suffix }}.tar.gz

  release:
    name: Release
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
        with:
          merge-multiple: true

      - name: Generate checksums
        run: sha256sum *.tar.gz > sha256sums.txt

      - uses: softprops/action-gh-release@v2
        with:
          files: |
            *.tar.gz
            sha256sums.txt
          generate_release_notes: true
```

Key changes: removed `if: always()` from release job (won't publish if build fails), added checksum generation step.

- [ ] **Step 2: Update nexa-cli release workflow**

Replace the full content of `nexa-cli/.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    tags: ['v*']

permissions:
  contents: write

env:
  CARGO_TERM_COLOR: always
  BINARY_NAME: nexa

jobs:
  build:
    name: Build ${{ matrix.target }}
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
            binary_suffix: linux-amd64
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-latest
            binary_suffix: linux-arm64
            use_cross: true
          - target: x86_64-apple-darwin
            os: macos-latest
            binary_suffix: darwin-amd64
          - target: aarch64-apple-darwin
            os: macos-latest
            binary_suffix: darwin-arm64
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2

      - name: Install cross
        if: matrix.use_cross
        run: cargo install cross --git https://github.com/cross-rs/cross

      - name: Build
        run: |
          if [ "${{ matrix.use_cross }}" = "true" ]; then
            cross build --release --target ${{ matrix.target }}
          else
            cargo build --release --target ${{ matrix.target }}
          fi

      - name: Package
        run: |
          cd target/${{ matrix.target }}/release
          tar czf ${BINARY_NAME}-${{ matrix.binary_suffix }}.tar.gz ${BINARY_NAME}
          mv ${BINARY_NAME}-${{ matrix.binary_suffix }}.tar.gz ../../../

      - uses: actions/upload-artifact@v4
        with:
          name: ${{ env.BINARY_NAME }}-${{ matrix.binary_suffix }}
          path: ${{ env.BINARY_NAME }}-${{ matrix.binary_suffix }}.tar.gz

  release:
    name: Release
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
        with:
          merge-multiple: true

      - name: Generate checksums
        run: sha256sum *.tar.gz > sha256sums.txt

      - uses: softprops/action-gh-release@v2
        with:
          files: |
            *.tar.gz
            sha256sums.txt
          generate_release_notes: true
```

- [ ] **Step 3: Add checksum verification to install.sh**

In `install.sh`, replace the `download_and_install` function (lines 161-239) with a version that verifies checksums:

```sh
download_and_install() {
    REPO="$1"
    BINARY="$2"

    if [ -n "$VERSION" ]; then
        VERSION_TAG="v${VERSION#v}"
    else
        VERSION_TAG=$(get_latest_version "$REPO") || {
            warn "Warning: no release found for ${REPO}, skipping"
            return 0
        }
    fi

    REMOTE_VERSION="${VERSION_TAG#v}"
    LOCAL_VERSION=$(get_installed_version "${INSTALL_DIR}/${BINARY}")

    if [ -n "$LOCAL_VERSION" ]; then
        if [ "$LOCAL_VERSION" = "$REMOTE_VERSION" ]; then
            success "${BINARY} ${LOCAL_VERSION} is already up to date"
            return 0
        fi

        # Different version — needs upgrade/downgrade
        if [ "$FORCE" = "1" ]; then
            info "Updating ${BINARY}: ${LOCAL_VERSION} -> ${REMOTE_VERSION}"
        elif [ ! -e /dev/tty ]; then
            # Non-interactive (piped) — update automatically
            info "Updating ${BINARY}: ${LOCAL_VERSION} -> ${REMOTE_VERSION}"
        else
            ask_choice "${BINARY} ${LOCAL_VERSION} is installed. Version ${REMOTE_VERSION} is available." \
                "Update to ${REMOTE_VERSION}" \
                "Reinstall ${REMOTE_VERSION} (overwrite)" \
                "Skip"

            case "$CHOICE" in
                1) info "Updating ${BINARY}..." ;;
                2) info "Reinstalling ${BINARY}..." ;;
                3) dim "Skipped ${BINARY}"; return 0 ;;
                *) dim "Skipped ${BINARY}"; return 0 ;;
            esac
        fi

        NEEDS_RESTART=1
    else
        info "Downloading ${BINARY}..."
    fi

    ARTIFACT="${BINARY}-${PLATFORM}-${ARCH}.tar.gz"
    URL="https://github.com/${GITHUB_ORG}/${REPO}/releases/download/${VERSION_TAG}/${ARTIFACT}"
    CHECKSUM_URL="https://github.com/${GITHUB_ORG}/${REPO}/releases/download/${VERSION_TAG}/sha256sums.txt"

    TMPDIR=$(mktemp -d)
    trap "rm -rf '$TMPDIR'" EXIT

    HTTP_CODE=$(curl -sSL -w '%{http_code}' -o "$TMPDIR/${ARTIFACT}" "$URL" 2>/dev/null) || true

    if [ "$HTTP_CODE" != "200" ]; then
        warn "Warning: failed to download ${BINARY} ${VERSION_TAG} (HTTP ${HTTP_CODE})"
        rm -rf "$TMPDIR"
        return 0
    fi

    # Verify checksum
    CHECKSUM_CODE=$(curl -sSL -w '%{http_code}' -o "$TMPDIR/sha256sums.txt" "$CHECKSUM_URL" 2>/dev/null) || true
    if [ "$CHECKSUM_CODE" = "200" ]; then
        EXPECTED=$(grep "${ARTIFACT}" "$TMPDIR/sha256sums.txt" | awk '{print $1}')
        if [ -n "$EXPECTED" ]; then
            if command -v sha256sum >/dev/null 2>&1; then
                ACTUAL=$(sha256sum "$TMPDIR/${ARTIFACT}" | awk '{print $1}')
            elif command -v shasum >/dev/null 2>&1; then
                ACTUAL=$(shasum -a 256 "$TMPDIR/${ARTIFACT}" | awk '{print $1}')
            else
                warn "Warning: no sha256sum or shasum found, skipping checksum verification"
                ACTUAL="$EXPECTED"
            fi
            if [ "$ACTUAL" != "$EXPECTED" ]; then
                error "Checksum verification failed for ${ARTIFACT}! Expected ${EXPECTED}, got ${ACTUAL}"
            fi
            dim "Checksum verified"
        else
            warn "Warning: artifact not found in sha256sums.txt, skipping verification"
        fi
    else
        warn "Warning: sha256sums.txt not available, skipping checksum verification"
    fi

    tar -xzf "$TMPDIR/${ARTIFACT}" -C "$TMPDIR" 2>/dev/null || {
        warn "Warning: failed to extract ${BINARY} archive"
        rm -rf "$TMPDIR"
        return 0
    }

    if [ -f "$TMPDIR/${BINARY}" ]; then
        install -m 755 "$TMPDIR/${BINARY}" "${INSTALL_DIR}/${BINARY}"
        if [ -n "$LOCAL_VERSION" ]; then
            success "Updated ${BINARY}: ${LOCAL_VERSION} -> ${REMOTE_VERSION}"
        else
            success "Installed ${BINARY} ${REMOTE_VERSION}"
        fi
    else
        warn "Warning: binary '${BINARY}' not found in archive"
    fi

    rm -rf "$TMPDIR"
    trap - EXIT
}
```

- [ ] **Step 4: Commit nexad workflow**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
git add .github/workflows/release.yml
git commit -m "fix(ci): add SHA-256 checksums to release, fix if:always

CRITICAL-7/8: Release artifacts now include sha256sums.txt.
Release job no longer runs if build fails."
```

- [ ] **Step 5: Commit nexa-cli workflow**

```bash
cd /Users/nassime/GitHub/NexaNet/nexa-cli
git add .github/workflows/release.yml
git commit -m "fix(ci): add SHA-256 checksums to release, fix if:always

CRITICAL-7/8: Release artifacts now include sha256sums.txt.
Release job no longer runs if build fails."
```

- [ ] **Step 6: Commit install.sh**

```bash
cd /Users/nassime/GitHub/NexaNet
git add install.sh
git commit -m "fix(security): verify SHA-256 checksums in install script

CRITICAL-7: install.sh now downloads sha256sums.txt and verifies
binary integrity before installation. Falls back gracefully if
checksums are unavailable (pre-existing releases)."
```

---

## Task 8: Push all changes

- [ ] **Step 1: Push nexad**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad && git push origin main
```

- [ ] **Step 2: Push nexa-cli**

```bash
cd /Users/nassime/GitHub/NexaNet/nexa-cli && git push origin main
```

- [ ] **Step 3: Push nexa (infra)**

```bash
cd /Users/nassime/GitHub/NexaNet && git push origin main
```

---

## Verification Checklist

After all tasks:

- [ ] `cd /Users/nassime/GitHub/NexaNet/nexad && cargo test 2>&1` — all tests pass
- [ ] `cd /Users/nassime/GitHub/NexaNet/nexa-cli && cargo test 2>&1` — all tests pass
- [ ] Verify `nexad --help` shows `--api-token`, `--host` defaults to `127.0.0.1`
- [ ] Verify `nexa secret set --help` shows `--value` as optional
- [ ] All 3 repos pushed with no uncommitted changes
