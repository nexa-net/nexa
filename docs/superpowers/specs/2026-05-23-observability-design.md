# NexaNet Observability Design — Prometheus Integration

**Date:** 2026-05-23
**Status:** Approved

## Goal

Add Prometheus-native metrics exposition to nexad, covering the full stack: API requests, container lifecycle, scheduler decisions, proxy throughput, and cluster state. Ship ready-to-use Grafana dashboards and Prometheus alerting rules.

## Decisions

- **Metrics consumer:** Prometheus only — expose `/metrics` in Prometheus text format
- **Scope:** Full stack (API, containers, scheduler, proxy, node/cluster gauges)
- **Endpoint location:** Same axum server in nexad (port 6443)
- **Architecture:** MetricsPort trait in nexa-core (hexagonal), Prometheus adapter in nexad
- **Dashboards & alerts:** Ship Grafana JSON dashboard and Prometheus alerting rules as files

## Architecture

### MetricsPort Trait (nexa-core)

New port at `nexa-core/src/ports/metrics.rs`:

```rust
#[async_trait]
pub trait MetricsPort: Send + Sync {
    // API / HTTP
    fn record_http_request(&self, method: &str, path: &str, status: u16, duration_secs: f64);

    // Container lifecycle
    fn record_container_event(&self, event: &str); // "created", "started", "stopped", "died", "oom"

    // Scheduler
    fn record_schedule_decision(&self, strategy: &str, duration_secs: f64);

    // Deployment operations
    fn record_deployment_op(&self, op: &str); // "deploy", "scale", "stop", "remove"

    // Node / cluster gauges
    fn set_node_count(&self, count: usize);
    fn set_pod_count(&self, count: usize);
    fn set_deployment_count(&self, count: usize);

    // Proxy (emitted by nexad when proxying via Traefik/Nginx/Caddy)
    fn record_proxy_request(&self, domain: &str, status: u16, duration_secs: f64);
    fn record_proxy_error(&self, domain: &str, error_type: &str);
}
```

A `NoOpMetrics` struct implements this trait with empty bodies for use in tests.

### Prometheus Adapter (nexad)

New adapter at `nexad/src/adapters/metrics/prometheus.rs` using the `prometheus` crate.

#### Metrics Table

| Metric Name | Type | Labels |
|---|---|---|
| `nexa_http_requests_total` | Counter | `method`, `path`, `status` |
| `nexa_http_request_duration_seconds` | Histogram | `method`, `path` |
| `nexa_container_events_total` | Counter | `event` |
| `nexa_schedule_duration_seconds` | Histogram | `strategy` |
| `nexa_deployment_ops_total` | Counter | `op` |
| `nexa_nodes_total` | Gauge | — |
| `nexa_pods_total` | Gauge | — |
| `nexa_deployments_total` | Gauge | — |
| `nexa_proxy_requests_total` | Counter | `domain`, `status` | (emitted by nexad) |
| `nexa_proxy_request_duration_seconds` | Histogram | `domain` | (emitted by nexad) |
| `nexa_proxy_errors_total` | Counter | `domain`, `error_type` | (emitted by nexad) |

The adapter holds a `prometheus::Registry`, pre-registers all metrics in `new()`, and exposes an `encode()` method that renders the registry to Prometheus text format.

### Endpoint

`GET /metrics` added to the existing axum router in `nexad/src/api/routes.rs`. The handler calls `metrics.encode()` and returns `Content-Type: text/plain; version=0.0.4`. Proxy-related metrics are emitted by nexad itself (not by the external proxy backend).

## Integration Points

### Orchestrator

`Orchestrator::spawn()` gains an `Option<Arc<dyn MetricsPort>>` as 9th parameter.

Inside the run loop:
- `handle_deploy()` → `record_deployment_op("deploy")`, update `set_pod_count()` / `set_deployment_count()`
- `handle_scale()` → `record_deployment_op("scale")`, update gauges
- `handle_stop()` → `record_deployment_op("stop")`
- `handle_remove()` → `record_deployment_op("remove")`, update gauges
- Scheduler path → `record_schedule_decision(strategy, elapsed)`
- `ContainerExited` / health events → `record_container_event()`

### API Middleware

A Tower middleware layer wraps the axum router. It:
1. Records `Instant::now()` before the request
2. Awaits the inner handler
3. Extracts method, path, status code
4. Calls `record_http_request()` on the `MetricsPort` from `AppState`

Handlers themselves never touch metrics — the middleware handles it.

### Event Watcher

`nexad/src/adapters/event_watcher.rs` already listens to Docker events (die, start, oom). Add `record_container_event()` calls alongside the existing `send_container_exited()` dispatch.

### Wiring (nexad main.rs)

```rust
let metrics = Arc::new(PrometheusMetrics::new());
let handle = Orchestrator::spawn(
    runtime, store, secrets, transport, dns, master_ip, proxy, route_store,
    Some(metrics.clone()),
);
let state = AppState { handle, store, metrics };
```

## Grafana Dashboard

Shipped as `deploy/grafana/nexanet-dashboard.json`.

- **Row 1 — API:** Request rate (rpm), latency p50/p95/p99, error rate (5xx %)
- **Row 2 — Containers:** Events over time (stacked: started/died/oom), pod/deployment/node gauges
- **Row 3 — Scheduler:** Decision latency histogram, decisions/min by strategy
- **Row 4 — Proxy:** Throughput by domain, upstream latency p50/p95, error rate by domain

Template variable `$instance` for filtering by nexad instance.

## Prometheus Alerting Rules

Shipped as `deploy/prometheus/alerts.yml`.

| Alert | Condition | Severity |
|---|---|---|
| `NexaHighErrorRate` | 5xx rate > 5% over 5m | warning |
| `NexaContainerOOM` | any OOM event in last 5m | critical |
| `NexaNodeDown` | `nexa_nodes_total` drops below expected for 2m | critical |
| `NexaHighAPILatency` | p99 > 2s over 5m | warning |
| `NexaProxyUpstreamErrors` | proxy error rate > 10% over 5m | warning |
| `NexaSchedulerSlow` | schedule decision p99 > 500ms over 5m | warning |

## Scrape Config

Shipped as `deploy/prometheus/scrape-config.yml` with sample `scrape_configs` targeting nexad:6443 `/metrics`.

## File Map

### nexa-core (new files)
- `src/ports/metrics.rs` — MetricsPort trait + NoOpMetrics

### nexa-core (modified)
- `src/ports/mod.rs` — add `pub mod metrics`
- `src/domain/orchestrator.rs` — add metrics parameter, instrument handlers

### nexad (new files)
- `src/adapters/metrics/mod.rs` — module declaration
- `src/adapters/metrics/prometheus.rs` — PrometheusMetrics adapter

### nexad (modified)
- `src/adapters/mod.rs` — add `pub mod metrics`
- `src/api/routes.rs` — add `/metrics` route
- `src/api/handlers.rs` — add metrics handler + middleware
- `src/api/mod.rs` — update AppState with metrics field
- `src/adapters/event_watcher.rs` — add container event recording
- `src/main.rs` — wire PrometheusMetrics into orchestrator + AppState
- `Cargo.toml` — add `prometheus` dependency

### Deploy configs (new files)
- `deploy/grafana/nexanet-dashboard.json`
- `deploy/prometheus/alerts.yml`
- `deploy/prometheus/scrape-config.yml`
