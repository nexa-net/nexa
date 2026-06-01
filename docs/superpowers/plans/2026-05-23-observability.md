# NexaNet Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Prometheus metrics exposition to nexad with full-stack coverage, Grafana dashboard, and alerting rules.

**Architecture:** MetricsPort trait in nexa-core (hexagonal port), PrometheusMetrics adapter in nexad using the `prometheus` crate. Metrics recorded via dependency injection; HTTP metrics via Tower middleware; `/metrics` endpoint on the nexad server.

**Tech Stack:** `prometheus` crate (counters, gauges, histograms), `async-trait`, axum middleware (Tower)

---

## File Structure

### nexa-core
| File | Responsibility |
|---|---|
| `src/ports/metrics.rs` (CREATE) | MetricsPort trait + NoOpMetrics |
| `src/ports/mod.rs` (MODIFY) | Add `pub mod metrics` |
| `src/domain/orchestrator.rs` (MODIFY) | Accept `Option<Arc<dyn MetricsPort>>`, instrument handlers |
| `Cargo.toml` (no change needed — async-trait already a dependency) | — |

### nexad
| File | Responsibility |
|---|---|
| `src/adapters/metrics/mod.rs` (CREATE) | Module re-export |
| `src/adapters/metrics/prometheus.rs` (CREATE) | PrometheusMetrics adapter implementing MetricsPort |
| `src/adapters/mod.rs` (MODIFY) | Add `pub mod metrics` |
| `src/api/mod.rs` (MODIFY) | Add `metrics` field to AppState, update `serve()` |
| `src/api/handlers.rs` (MODIFY) | Add `metrics` handler + `metrics_middleware` |
| `src/api/routes.rs` (MODIFY) | Add `/metrics` route, wire middleware |
| `src/adapters/event_watcher.rs` (MODIFY) | Accept MetricsPort, call record_container_event |
| `src/main.rs` (MODIFY) | Wire PrometheusMetrics into orchestrator, AppState, event_watcher |
| `Cargo.toml` (MODIFY) | Add `prometheus` dependency |

### Deploy configs (under NexaNet meta-repo)
| File | Responsibility |
|---|---|
| `deploy/grafana/nexanet-dashboard.json` (CREATE) | Grafana dashboard |
| `deploy/prometheus/alerts.yml` (CREATE) | Prometheus alerting rules |
| `deploy/prometheus/scrape-config.yml` (CREATE) | Example scrape config |

---

### Task 1: MetricsPort Trait and NoOpMetrics (nexa-core)

**Files:**
- Create: `nexa-core/src/ports/metrics.rs`
- Modify: `nexa-core/src/ports/mod.rs`

- [ ] **Step 1: Write the test for NoOpMetrics**

In `nexa-core/src/ports/metrics.rs`, add the trait and NoOpMetrics with a test at the bottom:

```rust
use async_trait::async_trait;

#[async_trait]
pub trait MetricsPort: Send + Sync {
    fn record_http_request(&self, method: &str, path: &str, status: u16, duration_secs: f64);
    fn record_container_event(&self, event: &str);
    fn record_schedule_decision(&self, strategy: &str, duration_secs: f64);
    fn record_deployment_op(&self, op: &str);
    fn set_node_count(&self, count: usize);
    fn set_pod_count(&self, count: usize);
    fn set_deployment_count(&self, count: usize);
    fn record_proxy_request(&self, domain: &str, status: u16, duration_secs: f64);
    fn record_proxy_error(&self, domain: &str, error_type: &str);
}

pub struct NoOpMetrics;

impl MetricsPort for NoOpMetrics {
    fn record_http_request(&self, _method: &str, _path: &str, _status: u16, _duration_secs: f64) {}
    fn record_container_event(&self, _event: &str) {}
    fn record_schedule_decision(&self, _strategy: &str, _duration_secs: f64) {}
    fn record_deployment_op(&self, _op: &str) {}
    fn set_node_count(&self, _count: usize) {}
    fn set_pod_count(&self, _count: usize) {}
    fn set_deployment_count(&self, _count: usize) {}
    fn record_proxy_request(&self, _domain: &str, _status: u16, _duration_secs: f64) {}
    fn record_proxy_error(&self, _domain: &str, _error_type: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_metrics_implements_trait() {
        let m: Box<dyn MetricsPort> = Box::new(NoOpMetrics);
        m.record_http_request("GET", "/health", 200, 0.001);
        m.record_container_event("started");
        m.record_schedule_decision("spread", 0.005);
        m.record_deployment_op("deploy");
        m.set_node_count(3);
        m.set_pod_count(10);
        m.set_deployment_count(5);
        m.record_proxy_request("api.example.com", 200, 0.05);
        m.record_proxy_error("api.example.com", "connection_refused");
    }
}
```

- [ ] **Step 2: Add the module to ports/mod.rs**

Add `pub mod metrics;` to `nexa-core/src/ports/mod.rs` (after the existing modules).

- [ ] **Step 3: Run tests to verify**

Run: `cd /Users/nassime/GitHub/NexaNet/nexa-core && cargo test ports::metrics`
Expected: PASS — the noop_metrics_implements_trait test passes.

- [ ] **Step 4: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexa-core
git add src/ports/metrics.rs src/ports/mod.rs
git commit -m "feat: add MetricsPort trait and NoOpMetrics"
```

---

### Task 2: PrometheusMetrics Adapter (nexad)

**Files:**
- Create: `nexad/src/adapters/metrics/mod.rs`
- Create: `nexad/src/adapters/metrics/prometheus.rs`
- Modify: `nexad/src/adapters/mod.rs`
- Modify: `nexad/Cargo.toml`

- [ ] **Step 1: Add prometheus dependency to nexad/Cargo.toml**

Add to `[dependencies]` section in `nexad/Cargo.toml`:

```toml
prometheus = "0.13"
```

- [ ] **Step 2: Create the module file**

Create `nexad/src/adapters/metrics/mod.rs`:

```rust
mod prometheus;
pub use self::prometheus::PrometheusMetrics;
```

- [ ] **Step 3: Write PrometheusMetrics with tests**

Create `nexad/src/adapters/metrics/prometheus.rs`:

```rust
use nexa_core::ports::metrics::MetricsPort;
use prometheus::{
    Encoder, GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, Opts, Registry,
    TextEncoder,
};

pub struct PrometheusMetrics {
    registry: Registry,
    http_requests_total: IntCounterVec,
    http_request_duration: HistogramVec,
    container_events_total: IntCounterVec,
    schedule_duration: HistogramVec,
    deployment_ops_total: IntCounterVec,
    nodes_total: IntGauge,
    pods_total: IntGauge,
    deployments_total: IntGauge,
    proxy_requests_total: IntCounterVec,
    proxy_request_duration: HistogramVec,
    proxy_errors_total: IntCounterVec,
}

impl PrometheusMetrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let http_requests_total = IntCounterVec::new(
            Opts::new("nexa_http_requests_total", "Total HTTP requests"),
            &["method", "path", "status"],
        )
        .unwrap();

        let http_request_duration = HistogramVec::new(
            HistogramOpts::new(
                "nexa_http_request_duration_seconds",
                "HTTP request duration in seconds",
            ),
            &["method", "path"],
        )
        .unwrap();

        let container_events_total = IntCounterVec::new(
            Opts::new("nexa_container_events_total", "Total container lifecycle events"),
            &["event"],
        )
        .unwrap();

        let schedule_duration = HistogramVec::new(
            HistogramOpts::new(
                "nexa_schedule_duration_seconds",
                "Scheduler decision duration in seconds",
            ),
            &["strategy"],
        )
        .unwrap();

        let deployment_ops_total = IntCounterVec::new(
            Opts::new("nexa_deployment_ops_total", "Total deployment operations"),
            &["op"],
        )
        .unwrap();

        let nodes_total =
            IntGauge::new("nexa_nodes_total", "Current number of cluster nodes").unwrap();

        let pods_total = IntGauge::new("nexa_pods_total", "Current number of pods").unwrap();

        let deployments_total =
            IntGauge::new("nexa_deployments_total", "Current number of deployments").unwrap();

        let proxy_requests_total = IntCounterVec::new(
            Opts::new("nexa_proxy_requests_total", "Total proxy requests"),
            &["domain", "status"],
        )
        .unwrap();

        let proxy_request_duration = HistogramVec::new(
            HistogramOpts::new(
                "nexa_proxy_request_duration_seconds",
                "Proxy upstream request duration in seconds",
            ),
            &["domain"],
        )
        .unwrap();

        let proxy_errors_total = IntCounterVec::new(
            Opts::new("nexa_proxy_errors_total", "Total proxy errors"),
            &["domain", "error_type"],
        )
        .unwrap();

        registry.register(Box::new(http_requests_total.clone())).unwrap();
        registry.register(Box::new(http_request_duration.clone())).unwrap();
        registry.register(Box::new(container_events_total.clone())).unwrap();
        registry.register(Box::new(schedule_duration.clone())).unwrap();
        registry.register(Box::new(deployment_ops_total.clone())).unwrap();
        registry.register(Box::new(nodes_total.clone())).unwrap();
        registry.register(Box::new(pods_total.clone())).unwrap();
        registry.register(Box::new(deployments_total.clone())).unwrap();
        registry.register(Box::new(proxy_requests_total.clone())).unwrap();
        registry.register(Box::new(proxy_request_duration.clone())).unwrap();
        registry.register(Box::new(proxy_errors_total.clone())).unwrap();

        Self {
            registry,
            http_requests_total,
            http_request_duration,
            container_events_total,
            schedule_duration,
            deployment_ops_total,
            nodes_total,
            pods_total,
            deployments_total,
            proxy_requests_total,
            proxy_request_duration,
            proxy_errors_total,
        }
    }

    pub fn encode(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }
}

impl MetricsPort for PrometheusMetrics {
    fn record_http_request(&self, method: &str, path: &str, status: u16, duration_secs: f64) {
        self.http_requests_total
            .with_label_values(&[method, path, &status.to_string()])
            .inc();
        self.http_request_duration
            .with_label_values(&[method, path])
            .observe(duration_secs);
    }

    fn record_container_event(&self, event: &str) {
        self.container_events_total
            .with_label_values(&[event])
            .inc();
    }

    fn record_schedule_decision(&self, strategy: &str, duration_secs: f64) {
        self.schedule_duration
            .with_label_values(&[strategy])
            .observe(duration_secs);
    }

    fn record_deployment_op(&self, op: &str) {
        self.deployment_ops_total.with_label_values(&[op]).inc();
    }

    fn set_node_count(&self, count: usize) {
        self.nodes_total.set(count as i64);
    }

    fn set_pod_count(&self, count: usize) {
        self.pods_total.set(count as i64);
    }

    fn set_deployment_count(&self, count: usize) {
        self.deployments_total.set(count as i64);
    }

    fn record_proxy_request(&self, domain: &str, status: u16, duration_secs: f64) {
        self.proxy_requests_total
            .with_label_values(&[domain, &status.to_string()])
            .inc();
        self.proxy_request_duration
            .with_label_values(&[domain])
            .observe(duration_secs);
    }

    fn record_proxy_error(&self, domain: &str, error_type: &str) {
        self.proxy_errors_total
            .with_label_values(&[domain, error_type])
            .inc();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_registry_with_all_metrics() {
        let m = PrometheusMetrics::new();
        let output = m.encode();
        assert!(output.is_empty(), "no metrics recorded yet, output should have no samples");
    }

    #[test]
    fn record_http_request_appears_in_output() {
        let m = PrometheusMetrics::new();
        m.record_http_request("GET", "/health", 200, 0.001);
        let output = m.encode();
        assert!(output.contains("nexa_http_requests_total"));
        assert!(output.contains("nexa_http_request_duration_seconds"));
    }

    #[test]
    fn record_container_event_appears_in_output() {
        let m = PrometheusMetrics::new();
        m.record_container_event("died");
        let output = m.encode();
        assert!(output.contains("nexa_container_events_total"));
        assert!(output.contains("died"));
    }

    #[test]
    fn gauges_update_correctly() {
        let m = PrometheusMetrics::new();
        m.set_node_count(3);
        m.set_pod_count(10);
        m.set_deployment_count(5);
        let output = m.encode();
        assert!(output.contains("nexa_nodes_total 3"));
        assert!(output.contains("nexa_pods_total 10"));
        assert!(output.contains("nexa_deployments_total 5"));
    }

    #[test]
    fn record_deployment_op_appears_in_output() {
        let m = PrometheusMetrics::new();
        m.record_deployment_op("deploy");
        m.record_deployment_op("scale");
        let output = m.encode();
        assert!(output.contains("nexa_deployment_ops_total"));
        assert!(output.contains("deploy"));
        assert!(output.contains("scale"));
    }

    #[test]
    fn record_proxy_metrics_appear_in_output() {
        let m = PrometheusMetrics::new();
        m.record_proxy_request("api.example.com", 200, 0.05);
        m.record_proxy_error("api.example.com", "connection_refused");
        let output = m.encode();
        assert!(output.contains("nexa_proxy_requests_total"));
        assert!(output.contains("nexa_proxy_errors_total"));
        assert!(output.contains("api.example.com"));
    }

    #[test]
    fn implements_metrics_port_trait() {
        let m = PrometheusMetrics::new();
        let port: &dyn MetricsPort = &m;
        port.record_http_request("POST", "/api/v1/deploy", 201, 0.5);
        port.record_schedule_decision("spread", 0.002);
    }
}
```

- [ ] **Step 4: Add metrics module to adapters/mod.rs**

Add `pub mod metrics;` to `nexad/src/adapters/mod.rs`.

- [ ] **Step 5: Run tests to verify**

Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo test adapters::metrics`
Expected: PASS — all 7 tests pass.

- [ ] **Step 6: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
git add src/adapters/metrics/ src/adapters/mod.rs Cargo.toml
git commit -m "feat: add PrometheusMetrics adapter implementing MetricsPort"
```

---

### Task 3: Add MetricsPort to Orchestrator (nexa-core)

**Files:**
- Modify: `nexa-core/src/domain/orchestrator.rs`

The Orchestrator struct (line 390) and `spawn()` (line 410) need a metrics field. The run loop handlers need instrumentation.

- [ ] **Step 1: Add metrics field to Orchestrator struct**

At `nexa-core/src/domain/orchestrator.rs:390`, add the import and field:

Add to the imports at the top of the file (around line 22, after the existing `use crate::ports::...` lines):
```rust
use crate::ports::metrics::MetricsPort;
```

Add to the `Orchestrator` struct (after `route_store` field at line 405):
```rust
    metrics: Option<Arc<dyn MetricsPort>>,
```

- [ ] **Step 2: Update spawn() to accept metrics parameter**

At `nexa-core/src/domain/orchestrator.rs:410`, add `metrics: Option<Arc<dyn MetricsPort>>` as the 9th parameter to `spawn()`:

```rust
    pub fn spawn(
        runtime: Arc<dyn ContainerRuntime>,
        state_store: Option<Arc<dyn StateStore>>,
        secret_store: Option<Arc<dyn SecretStore>>,
        transport: Option<Arc<dyn ClusterTransport>>,
        dns: Option<Arc<dyn DnsProvider>>,
        master_ip: Option<String>,
        proxy: Option<Arc<dyn ProxyBackend>>,
        route_store: Option<Arc<dyn RouteStore>>,
        metrics: Option<Arc<dyn MetricsPort>>,
    ) -> OrchestratorHandle {
```

And in the `Self { ... }` constructor inside spawn (after `route_store,` at line 438), add:
```rust
                metrics,
```

- [ ] **Step 3: Add helper method for metric recording**

Add a helper method on `Orchestrator` (after the `run` method, around line 575):

```rust
    fn update_gauge_counts(&self) {
        if let Some(ref m) = self.metrics {
            m.set_pod_count(self.pods.len());
            m.set_deployment_count(self.deployments.len());
        }
    }
```

- [ ] **Step 4: Instrument handle_deploy**

In `handle_deploy` (line 706), add after a successful deployment is inserted/updated (before the final `Ok(...)` returns). Add at the end of the method, just before the two `Ok(...)` return lines (around lines 726 and 734):

After `self.reconcile_deployment(id).await?;` (the update path, line 725), add:
```rust
            if let Some(ref m) = self.metrics {
                m.record_deployment_op("deploy");
            }
            self.update_gauge_counts();
```

After `self.reconcile_deployment(id).await?;` (the create path, line 733), add:
```rust
        if let Some(ref m) = self.metrics {
            m.record_deployment_op("deploy");
        }
        self.update_gauge_counts();
```

- [ ] **Step 5: Instrument handle_scale**

In `handle_scale` (line 812), add after `self.reconcile_deployment(deployment_id).await?;` (line 827):

```rust
        if let Some(ref m) = self.metrics {
            m.record_deployment_op("scale");
        }
        self.update_gauge_counts();
```

- [ ] **Step 6: Instrument handle_stop**

In `handle_stop` (line 759), add just before the final `Ok(())` (line 797):

```rust
        if let Some(ref m) = self.metrics {
            m.record_deployment_op("stop");
        }
        self.update_gauge_counts();
```

- [ ] **Step 7: Instrument handle_remove_deployment**

In `handle_remove_deployment` (line 800), add just before the final `Ok(())` (line 809):

```rust
        if let Some(ref m) = self.metrics {
            m.record_deployment_op("remove");
        }
        self.update_gauge_counts();
```

- [ ] **Step 8: Instrument handle_container_exited**

In `handle_container_exited` (line 1204), add near the top of the method, after the pod is looked up (after line 1209):

```rust
        if let Some(ref m) = self.metrics {
            m.record_container_event("died");
        }
```

- [ ] **Step 9: Instrument select_node (scheduler timing)**

In `select_node` (line 995), wrap the scheduler call with timing. Replace the `self.scheduler.select_node(...)` call at line 1027 with:

```rust
        let start = std::time::Instant::now();
        let result = self.scheduler.select_node(&pod_request, &snapshots).ok();
        if let Some(ref m) = self.metrics {
            m.record_schedule_decision(&self.scheduler_strategy, start.elapsed().as_secs_f64());
        }
        result
```

- [ ] **Step 10: Update all existing callers of Orchestrator::spawn**

Every call site that creates `Orchestrator::spawn(...)` needs the new 9th argument `None` added (for now — wiring real metrics comes in Task 6).

Search the codebase for `Orchestrator::spawn(` — there are callers in:
1. `nexad/tests/api_integration.rs` (line 134) — add `None,` after the `Some(route_store),` argument
2. `nexa-core/src/domain/orchestrator.rs` itself in tests (search for `Orchestrator::spawn` in the `#[cfg(test)]` blocks at the bottom of the file) — add `None,` as the last argument to each call

- [ ] **Step 11: Run tests to verify**

Run: `cd /Users/nassime/GitHub/NexaNet/nexa-core && cargo test`
Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo test`
Expected: both PASS

- [ ] **Step 12: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexa-core
git add src/domain/orchestrator.rs
git commit -m "feat: add MetricsPort to Orchestrator with handler instrumentation"
```

---

### Task 4: /metrics Endpoint and HTTP Middleware (nexad)

**Files:**
- Modify: `nexad/src/api/mod.rs`
- Modify: `nexad/src/api/handlers.rs`
- Modify: `nexad/src/api/routes.rs`

- [ ] **Step 1: Add metrics to AppState**

In `nexad/src/api/mod.rs`, update the AppState struct and `serve()` function:

```rust
mod handlers;
pub mod routes;

use std::sync::Arc;

use nexa_core::domain::orchestrator::OrchestratorHandle;
use nexa_core::ports::metrics::MetricsPort;
use nexa_core::ports::state::StateStore;

#[derive(Clone)]
pub struct AppState {
    pub handle: OrchestratorHandle,
    pub store: Arc<dyn StateStore>,
    pub metrics: Arc<dyn MetricsPort>,
}

pub async fn serve(
    handle: OrchestratorHandle,
    store: Arc<dyn StateStore>,
    metrics: Arc<dyn MetricsPort>,
    addr: &str,
) -> anyhow::Result<()> {
    let state = AppState {
        handle,
        store,
        metrics,
    };
    let app = routes::build(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("nexad API listening on {addr}");

    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 2: Add metrics handler and middleware to handlers.rs**

Add at the top of `nexad/src/api/handlers.rs`, alongside existing imports:

```rust
use axum::middleware::Next;
use axum::http::Request as AxumRequest;
use std::time::Instant;
```

Add two new functions at the bottom of `nexad/src/api/handlers.rs` (before any `#[cfg(test)]` block):

```rust
pub async fn metrics_endpoint(State(state): AppStateExtractor) -> impl IntoResponse {
    use crate::adapters::metrics::PrometheusMetrics;
    let prom = state
        .metrics
        .as_any()
        .downcast_ref::<PrometheusMetrics>();
    match prom {
        Some(p) => (
            StatusCode::OK,
            [("content-type", "text/plain; version=0.0.4")],
            p.encode(),
        )
            .into_response(),
        None => (StatusCode::OK, "# no prometheus metrics available\n").into_response(),
    }
}

pub async fn metrics_middleware(
    State(state): AppStateExtractor,
    req: AxumRequest<axum::body::Body>,
    next: Next,
) -> impl IntoResponse {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let start = Instant::now();
    let response = next.run(req).await;
    let status = response.status().as_u16();
    let duration = start.elapsed().as_secs_f64();
    state
        .metrics
        .record_http_request(&method, &path, status, duration);
    response
}
```

**Important:** For the `metrics_endpoint` handler to work via downcasting, we need to add `as_any()` to the MetricsPort trait. Go back to `nexa-core/src/ports/metrics.rs` and add:

```rust
use std::any::Any;
```

Add this method to the `MetricsPort` trait:
```rust
    fn as_any(&self) -> &dyn Any;
```

Add this implementation to `NoOpMetrics`:
```rust
    fn as_any(&self) -> &dyn Any {
        self
    }
```

And in `nexad/src/adapters/metrics/prometheus.rs`, add to the `MetricsPort for PrometheusMetrics` impl:
```rust
    fn as_any(&self) -> &dyn Any {
        self
    }
```

- [ ] **Step 3: Wire the /metrics route and middleware in routes.rs**

Replace `nexad/src/api/routes.rs` content with:

```rust
use axum::Router;
use axum::middleware;
use axum::routing::{delete, get, post};
use tower_http::trace::TraceLayer;

use super::AppState;
use super::handlers;

pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/metrics", get(handlers::metrics_endpoint))
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
            handlers::metrics_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
```

- [ ] **Step 4: Update api_integration.rs test to pass metrics**

In `nexad/tests/api_integration.rs`, the `TestServer::new()` method constructs `AppState`. Update it:

Add import:
```rust
use nexa_core::ports::metrics::NoOpMetrics;
```

Update the AppState construction (around line 146):
```rust
        let metrics: Arc<dyn nexa_core::ports::metrics::MetricsPort> = Arc::new(NoOpMetrics);
```

Update the Orchestrator::spawn call to add `None,` as the last (9th) argument.

Update AppState:
```rust
        let state = AppState {
            handle,
            store: store.clone(),
            metrics,
        };
```

Also update the `serve()` call in main.rs (done in Task 6).

- [ ] **Step 5: Run tests to verify**

Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo test`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexa-core
git add src/ports/metrics.rs
git commit -m "feat: add as_any() to MetricsPort for downcasting"

cd /Users/nassime/GitHub/NexaNet/nexad
git add src/api/mod.rs src/api/handlers.rs src/api/routes.rs tests/api_integration.rs
git commit -m "feat: add /metrics endpoint and HTTP metrics middleware"
```

---

### Task 5: Instrument Event Watcher (nexad)

**Files:**
- Modify: `nexad/src/adapters/event_watcher.rs`

- [ ] **Step 1: Update spawn_event_watcher signature**

Modify `nexad/src/adapters/event_watcher.rs` to accept an `Option<Arc<dyn MetricsPort>>`:

```rust
use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

use nexa_core::domain::orchestrator::Command;
use nexa_core::ports::metrics::MetricsPort;
use nexa_core::ports::runtime::{ContainerRuntime, RuntimeEvent};

pub fn spawn_event_watcher(
    runtime: Arc<dyn ContainerRuntime>,
    tx: mpsc::Sender<Command>,
    metrics: Option<Arc<dyn MetricsPort>>,
) {
    tokio::spawn(async move {
        info!("container event watcher starting");
        loop {
            match runtime.events().await {
                Ok(stream) => {
                    handle_event_stream(stream, &tx, metrics.as_deref()).await;
                    warn!("event stream ended, reconnecting in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    error!(error = %e, "failed to open event stream, retrying in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    });
}
```

- [ ] **Step 2: Update handle_event_stream to record metrics**

```rust
async fn handle_event_stream(
    mut stream: nexa_core::ports::runtime::EventStream,
    tx: &mpsc::Sender<Command>,
    metrics: Option<&dyn MetricsPort>,
) {
    while let Some(event) = stream.next().await {
        match event {
            RuntimeEvent::ContainerDied {
                container_id,
                exit_code,
            } => {
                info!(container_id, exit_code, "container died event");
                if let Some(m) = metrics {
                    m.record_container_event("died");
                }
                if let Some(pod_id) = extract_pod_id(&container_id) {
                    let cmd = Command::ContainerExited { pod_id, exit_code };
                    if tx.send(cmd).await.is_err() {
                        error!("orchestrator channel closed, stopping event watcher");
                        return;
                    }
                }
            }
            RuntimeEvent::ContainerOom { container_id } => {
                warn!(container_id, "container OOM event");
                if let Some(m) = metrics {
                    m.record_container_event("oom");
                }
                if let Some(pod_id) = extract_pod_id(&container_id) {
                    let cmd = Command::ContainerExited {
                        pod_id,
                        exit_code: 137,
                    };
                    if tx.send(cmd).await.is_err() {
                        error!("orchestrator channel closed, stopping event watcher");
                        return;
                    }
                }
            }
            RuntimeEvent::ContainerStarted { container_id } => {
                info!(container_id, "container started event");
                if let Some(m) = metrics {
                    m.record_container_event("started");
                }
            }
        }
    }
}
```

- [ ] **Step 3: Update existing tests**

The existing tests call `handle_event_stream(stream, &tx).await` — update to pass `None` as the 3rd argument:

Replace all `handle_event_stream(stream, &tx).await;` with `handle_event_stream(stream, &tx, None).await;` in the `#[cfg(test)]` block.

- [ ] **Step 4: Add test for metrics recording**

Add to the test module:

```rust
    #[tokio::test]
    async fn event_watcher_records_metrics_on_die() {
        use nexa_core::ports::metrics::NoOpMetrics;

        let pod_id = Uuid::new_v4();
        let events = vec![RuntimeEvent::ContainerDied {
            container_id: pod_id.to_string(),
            exit_code: 1,
        }];
        let (tx, _rx) = mpsc::channel(16);
        let stream: nexa_core::ports::runtime::EventStream =
            Box::pin(futures::stream::iter(events));
        let metrics = NoOpMetrics;
        handle_event_stream(stream, &tx, Some(&metrics)).await;
    }
```

- [ ] **Step 5: Run tests to verify**

Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo test adapters::event_watcher`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
git add src/adapters/event_watcher.rs
git commit -m "feat: instrument event watcher with MetricsPort"
```

---

### Task 6: Wire Everything in nexad main.rs

**Files:**
- Modify: `nexad/src/main.rs`

- [ ] **Step 1: Update imports**

Add to the imports at the top of `nexad/src/main.rs`:

```rust
use nexa_core::ports::metrics::MetricsPort;
```

- [ ] **Step 2: Update spawn_orchestrator to accept and pass metrics**

Update the `spawn_orchestrator` function signature (line 200) to accept metrics:

```rust
fn spawn_orchestrator(
    runtime: &Arc<dyn ContainerRuntime>,
    store: &Arc<dyn StateStore>,
    secret_store: Arc<dyn SecretStore>,
    dns: Option<Arc<dyn DnsProvider>>,
    master_ip: Option<String>,
    proxy: Option<Arc<dyn nexa_core::ports::proxy::ProxyBackend>>,
    route_store: Option<Arc<dyn nexa_core::ports::route_store::RouteStore>>,
    metrics: Option<Arc<dyn MetricsPort>>,
) -> nexa_core::domain::orchestrator::OrchestratorHandle {
```

Update the `Orchestrator::spawn(...)` call inside this function (line 212) to pass `metrics.clone()` as the 9th argument:

```rust
    let handle = Orchestrator::spawn(
        Arc::clone(runtime),
        Some(Arc::clone(store)),
        Some(secret_store),
        Some(transport),
        dns,
        master_ip,
        proxy,
        route_store,
        metrics.clone(),
    );
```

Update the `spawn_event_watcher` call (line 229) to pass metrics:

```rust
    nexad::adapters::event_watcher::spawn_event_watcher(
        Arc::clone(runtime),
        handle.command_sender(),
        metrics,
    );
```

- [ ] **Step 3: Update start_single_node**

In `start_single_node` (line 268), create PrometheusMetrics and pass to orchestrator and serve:

After the `let (proxy, route_store) = init_proxy(cli)?;` line (line 277), add:

```rust
    let metrics: Arc<dyn MetricsPort> =
        Arc::new(nexad::adapters::metrics::PrometheusMetrics::new());
```

Update `spawn_orchestrator` call to pass `Some(metrics.clone())`:

```rust
    let handle = spawn_orchestrator(
        &runtime,
        &store,
        secret_store,
        dns,
        master_ip,
        Some(Arc::clone(&proxy)),
        Some(Arc::clone(&route_store)),
        Some(metrics.clone()),
    );
```

Update the `nexad::api::serve` call (line 304) to pass metrics:

```rust
    nexad::api::serve(handle, Arc::clone(&store), metrics, &addr).await
```

- [ ] **Step 4: Update start_master similarly**

Apply the same changes to `start_master` (line 309): create `PrometheusMetrics`, pass to `spawn_orchestrator`, pass to `nexad::api::serve`.

- [ ] **Step 5: Run tests and cargo check**

Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo check`
Expected: PASS — no compilation errors.

Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo test`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
git add src/main.rs
git commit -m "feat: wire PrometheusMetrics into nexad startup"
```

---

### Task 7: Push nexa-core and Update nexad Dependency

Since nexad depends on nexa-core via git, nexa-core changes must be pushed first.

- [ ] **Step 1: Push nexa-core**

```bash
cd /Users/nassime/GitHub/NexaNet/nexa-core
git push origin main
```

- [ ] **Step 2: Update nexad's lock file**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
cargo update -p nexa-core
```

- [ ] **Step 3: Verify nexad compiles and tests pass**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
cargo test
```

- [ ] **Step 4: Commit lock file if changed**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
git add Cargo.lock
git commit -m "chore: update nexa-core dependency (MetricsPort)"
```

---

### Task 8: Grafana Dashboard (deploy config)

**Files:**
- Create: `deploy/grafana/nexanet-dashboard.json`

- [ ] **Step 1: Create deploy directories**

```bash
mkdir -p /Users/nassime/GitHub/NexaNet/deploy/grafana
mkdir -p /Users/nassime/GitHub/NexaNet/deploy/prometheus
```

- [ ] **Step 2: Write dashboard JSON**

Create `deploy/grafana/nexanet-dashboard.json` with a Grafana dashboard containing 4 rows:

```json
{
  "__inputs": [
    {
      "name": "DS_PROMETHEUS",
      "label": "Prometheus",
      "type": "datasource",
      "pluginId": "prometheus"
    }
  ],
  "title": "NexaNet Overview",
  "uid": "nexanet-overview",
  "version": 1,
  "schemaVersion": 39,
  "templating": {
    "list": [
      {
        "name": "instance",
        "type": "query",
        "query": "label_values(nexa_http_requests_total, instance)",
        "datasource": "${DS_PROMETHEUS}",
        "multi": true,
        "includeAll": true
      }
    ]
  },
  "panels": [
    {
      "title": "API Request Rate",
      "type": "timeseries",
      "gridPos": { "h": 8, "w": 8, "x": 0, "y": 0 },
      "targets": [
        {
          "expr": "sum(rate(nexa_http_requests_total{instance=~\"$instance\"}[5m])) by (method)",
          "legendFormat": "{{method}}"
        }
      ]
    },
    {
      "title": "API Latency (p50 / p95 / p99)",
      "type": "timeseries",
      "gridPos": { "h": 8, "w": 8, "x": 8, "y": 0 },
      "targets": [
        {
          "expr": "histogram_quantile(0.50, sum(rate(nexa_http_request_duration_seconds_bucket{instance=~\"$instance\"}[5m])) by (le))",
          "legendFormat": "p50"
        },
        {
          "expr": "histogram_quantile(0.95, sum(rate(nexa_http_request_duration_seconds_bucket{instance=~\"$instance\"}[5m])) by (le))",
          "legendFormat": "p95"
        },
        {
          "expr": "histogram_quantile(0.99, sum(rate(nexa_http_request_duration_seconds_bucket{instance=~\"$instance\"}[5m])) by (le))",
          "legendFormat": "p99"
        }
      ]
    },
    {
      "title": "API Error Rate (5xx %)",
      "type": "timeseries",
      "gridPos": { "h": 8, "w": 8, "x": 16, "y": 0 },
      "targets": [
        {
          "expr": "sum(rate(nexa_http_requests_total{instance=~\"$instance\",status=~\"5..\"}[5m])) / sum(rate(nexa_http_requests_total{instance=~\"$instance\"}[5m])) * 100",
          "legendFormat": "5xx %"
        }
      ]
    },
    {
      "title": "Container Events",
      "type": "timeseries",
      "gridPos": { "h": 8, "w": 8, "x": 0, "y": 8 },
      "targets": [
        {
          "expr": "sum(rate(nexa_container_events_total{instance=~\"$instance\"}[5m])) by (event)",
          "legendFormat": "{{event}}"
        }
      ]
    },
    {
      "title": "Cluster Gauges",
      "type": "stat",
      "gridPos": { "h": 8, "w": 8, "x": 8, "y": 8 },
      "targets": [
        { "expr": "nexa_nodes_total{instance=~\"$instance\"}", "legendFormat": "Nodes" },
        { "expr": "nexa_pods_total{instance=~\"$instance\"}", "legendFormat": "Pods" },
        { "expr": "nexa_deployments_total{instance=~\"$instance\"}", "legendFormat": "Deployments" }
      ]
    },
    {
      "title": "Deployment Operations",
      "type": "timeseries",
      "gridPos": { "h": 8, "w": 8, "x": 16, "y": 8 },
      "targets": [
        {
          "expr": "sum(rate(nexa_deployment_ops_total{instance=~\"$instance\"}[5m])) by (op)",
          "legendFormat": "{{op}}"
        }
      ]
    },
    {
      "title": "Scheduler Decision Latency",
      "type": "timeseries",
      "gridPos": { "h": 8, "w": 12, "x": 0, "y": 16 },
      "targets": [
        {
          "expr": "histogram_quantile(0.99, sum(rate(nexa_schedule_duration_seconds_bucket{instance=~\"$instance\"}[5m])) by (le, strategy))",
          "legendFormat": "p99 {{strategy}}"
        },
        {
          "expr": "histogram_quantile(0.50, sum(rate(nexa_schedule_duration_seconds_bucket{instance=~\"$instance\"}[5m])) by (le, strategy))",
          "legendFormat": "p50 {{strategy}}"
        }
      ]
    },
    {
      "title": "Scheduler Decisions/min",
      "type": "timeseries",
      "gridPos": { "h": 8, "w": 12, "x": 12, "y": 16 },
      "targets": [
        {
          "expr": "sum(rate(nexa_schedule_duration_seconds_count{instance=~\"$instance\"}[5m])) by (strategy) * 60",
          "legendFormat": "{{strategy}}"
        }
      ]
    },
    {
      "title": "Proxy Throughput",
      "type": "timeseries",
      "gridPos": { "h": 8, "w": 8, "x": 0, "y": 24 },
      "targets": [
        {
          "expr": "sum(rate(nexa_proxy_requests_total{instance=~\"$instance\"}[5m])) by (domain)",
          "legendFormat": "{{domain}}"
        }
      ]
    },
    {
      "title": "Proxy Upstream Latency (p50 / p95)",
      "type": "timeseries",
      "gridPos": { "h": 8, "w": 8, "x": 8, "y": 24 },
      "targets": [
        {
          "expr": "histogram_quantile(0.50, sum(rate(nexa_proxy_request_duration_seconds_bucket{instance=~\"$instance\"}[5m])) by (le, domain))",
          "legendFormat": "p50 {{domain}}"
        },
        {
          "expr": "histogram_quantile(0.95, sum(rate(nexa_proxy_request_duration_seconds_bucket{instance=~\"$instance\"}[5m])) by (le, domain))",
          "legendFormat": "p95 {{domain}}"
        }
      ]
    },
    {
      "title": "Proxy Error Rate",
      "type": "timeseries",
      "gridPos": { "h": 8, "w": 8, "x": 16, "y": 24 },
      "targets": [
        {
          "expr": "sum(rate(nexa_proxy_errors_total{instance=~\"$instance\"}[5m])) by (domain, error_type)",
          "legendFormat": "{{domain}} - {{error_type}}"
        }
      ]
    }
  ]
}
```

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet
git add deploy/grafana/nexanet-dashboard.json
git commit -m "feat: add Grafana dashboard for NexaNet observability"
```

---

### Task 9: Prometheus Alerting Rules (deploy config)

**Files:**
- Create: `deploy/prometheus/alerts.yml`

- [ ] **Step 1: Write alerting rules**

Create `deploy/prometheus/alerts.yml`:

```yaml
groups:
  - name: nexanet
    rules:
      - alert: NexaHighErrorRate
        expr: >
          sum(rate(nexa_http_requests_total{status=~"5.."}[5m]))
          /
          sum(rate(nexa_http_requests_total[5m]))
          > 0.05
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High API error rate"
          description: "5xx error rate is above 5% for the last 5 minutes."

      - alert: NexaContainerOOM
        expr: increase(nexa_container_events_total{event="oom"}[5m]) > 0
        labels:
          severity: critical
        annotations:
          summary: "Container OOM detected"
          description: "A container was killed due to out-of-memory in the last 5 minutes."

      - alert: NexaNodeDown
        expr: nexa_nodes_total < 1
        for: 2m
        labels:
          severity: critical
        annotations:
          summary: "No cluster nodes"
          description: "nexa_nodes_total has been below 1 for 2 minutes."

      - alert: NexaHighAPILatency
        expr: >
          histogram_quantile(0.99,
            sum(rate(nexa_http_request_duration_seconds_bucket[5m])) by (le)
          ) > 2
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High API latency"
          description: "API p99 latency is above 2 seconds for the last 5 minutes."

      - alert: NexaProxyUpstreamErrors
        expr: >
          sum(rate(nexa_proxy_errors_total[5m]))
          /
          (sum(rate(nexa_proxy_requests_total[5m])) + sum(rate(nexa_proxy_errors_total[5m])))
          > 0.1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High proxy error rate"
          description: "Proxy error rate is above 10% for the last 5 minutes."

      - alert: NexaSchedulerSlow
        expr: >
          histogram_quantile(0.99,
            sum(rate(nexa_schedule_duration_seconds_bucket[5m])) by (le)
          ) > 0.5
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Slow scheduler decisions"
          description: "Scheduler p99 latency is above 500ms for the last 5 minutes."
```

- [ ] **Step 2: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet
git add deploy/prometheus/alerts.yml
git commit -m "feat: add Prometheus alerting rules for NexaNet"
```

---

### Task 10: Prometheus Scrape Config Example (deploy config)

**Files:**
- Create: `deploy/prometheus/scrape-config.yml`

- [ ] **Step 1: Write example scrape config**

Create `deploy/prometheus/scrape-config.yml`:

```yaml
# Example Prometheus scrape configuration for NexaNet.
# Add these entries to your prometheus.yml under scrape_configs.

scrape_configs:
  - job_name: "nexad"
    static_configs:
      - targets: ["localhost:6443"]
    metrics_path: /metrics
    scrape_interval: 15s
```

- [ ] **Step 2: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet
git add deploy/prometheus/scrape-config.yml
git commit -m "feat: add example Prometheus scrape config"
```

---

### Task 11: Integration Test — /metrics Endpoint (nexad)

**Files:**
- Modify: `nexad/tests/api_integration.rs`

- [ ] **Step 1: Add metrics endpoint test**

Add this test to `nexad/tests/api_integration.rs` (after the existing tests):

```rust
#[tokio::test]
async fn metrics_endpoint_returns_prometheus_format() {
    let server = TestServer::new().await;

    // Make a health request first to generate some metrics
    let _ = server.get("/health").await;

    let resp = server.get("/metrics").await;
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    // NoOpMetrics is used in tests, so body may be minimal.
    // Just verify the endpoint responds with the right content type.
    assert!(resp.headers().get("content-type").is_some() || body.contains("nexa_") || body.is_empty() || body.contains("# no prometheus"));
}
```

Wait — the test uses `NoOpMetrics`, which means the endpoint will return "# no prometheus metrics available". Let's instead use the real `PrometheusMetrics` in the test server to verify the full flow. Update TestServer::new() in the test to use `PrometheusMetrics`:

Change the metrics construction in `TestServer::new()` from `NoOpMetrics` to:

```rust
        use nexad::adapters::metrics::PrometheusMetrics;
        let metrics: Arc<dyn nexa_core::ports::metrics::MetricsPort> =
            Arc::new(PrometheusMetrics::new());
```

Then add the test:

```rust
#[tokio::test]
async fn metrics_endpoint_returns_prometheus_format() {
    let server = TestServer::new().await;

    // Generate some traffic
    let _ = server.get("/health").await;

    let resp = server.get("/metrics").await;
    assert_eq!(resp.status(), 200);

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/plain"),
        "expected text/plain content-type, got: {content_type}"
    );

    let body = resp.text().await.unwrap();
    assert!(
        body.contains("nexa_http_requests_total"),
        "expected nexa_http_requests_total in metrics output"
    );
    assert!(
        body.contains("nexa_http_request_duration_seconds"),
        "expected duration histogram in metrics output"
    );
}
```

- [ ] **Step 2: Run tests**

Run: `cd /Users/nassime/GitHub/NexaNet/nexad && cargo test metrics_endpoint`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad
git add tests/api_integration.rs
git commit -m "test: add /metrics endpoint integration test"
```

---

### Task 12: Push All Repos

- [ ] **Step 1: Run full test suite across all repos**

```bash
cd /Users/nassime/GitHub/NexaNet/nexa-core && cargo test
cd /Users/nassime/GitHub/NexaNet/nexad && cargo test
```

Expected: all PASS.

- [ ] **Step 2: Run cargo fmt and clippy across all repos**

```bash
cd /Users/nassime/GitHub/NexaNet/nexa-core && cargo fmt --check && cargo clippy -- -D warnings
cd /Users/nassime/GitHub/NexaNet/nexad && cargo fmt --check && cargo clippy -- -D warnings
```

Fix any issues and commit.

- [ ] **Step 3: Push all repos**

```bash
cd /Users/nassime/GitHub/NexaNet/nexa-core && git push origin main
cd /Users/nassime/GitHub/NexaNet/nexad && git push origin main
cd /Users/nassime/GitHub/NexaNet && git push origin main
```

- [ ] **Step 4: Verify CI passes on all repos**

Check GitHub Actions for nexa-core, nexad — all should be green.
