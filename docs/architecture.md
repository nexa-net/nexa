# NexaNet Architecture

This document describes how NexaNet is structured, how a request flows through
the system, and the key cross-cutting concerns (persistence, the cluster
protocol, security, and observability). It is intended for contributors and
operators who need a mental model of the system beyond any single crate.

## 1. Repository layout

NexaNet is developed as a **multi-repo** project. Each crate is released
independently and consumes its upstream by a pinned git tag, not a workspace
path. The crates and their roles:

| Crate | Role | Depends on |
|-------|------|-----------|
| `nexa-core` | Pure domain logic — the orchestrator, scheduling, models, and the port (trait) definitions. No I/O. | — |
| `nexad` | The daemon. Implements every port (`nexa-core`) with a concrete adapter (Docker/containerd, SQLite, gRPC, Prometheus, …) and exposes the HTTP API and the cluster gRPC service. | `nexa-core` (git tag) |
| `nexa-cli` (`nexa`) | The user-facing CLI / TUI. Talks to `nexad` over HTTP. | `nexa-core` (git tag) |

Because `nexad` and `nexa-cli` pin `nexa-core` by tag (e.g. `tag = "v0.1.4"`),
a change to `nexa-core` only reaches them after `nexa-core` is released and the
tag bump is applied downstream. Keep this in mind when adding trait methods:
extend ports with **defaulted** methods so existing downstream adapters keep
compiling against the older tag.

Supporting directories in this meta-repo:

- `proto/` (under `nexad/`) — the cluster gRPC contract (`cluster.proto`).
- `deploy/` — Prometheus scrape config + alert rules, Grafana dashboard.
- `install.sh` — checksum-verified installer.
- `docs/` — this document, the audit, and the design specs/plans.

## 2. nexa-core: hexagonal domain

`nexa-core` follows a **ports-and-adapters (hexagonal)** architecture:

```
            domain/models/         pure data types (Deployment, Pod, Node, …)
                  │
            domain/orchestrator.rs  the application core (actor)
                  │  uses
            ports/*.rs              async traits: StateStore, ContainerRuntime,
                  │                 DnsProvider, ProxyManager, SecretStore,
                  │                 ClusterTransport, MetricsPort, RouteStore
                  ▼
            adapters/*.rs           in-memory reference implementations
                                    (real adapters live in nexad)
```

- **Models** (`domain/models/`) are plain types with validation helpers
  (`DeploymentSpec::validate()` enforces DNS-safe names, non-empty image,
  replica/port/resource invariants).
- **Ports** (`ports/`) are `#[async_trait]` interfaces. The domain depends only
  on these traits, never on a concrete backend — dependency injection happens
  via `Arc<dyn Trait>`.
- **The orchestrator** (`domain/orchestrator.rs`) is an **actor**: a single task
  owns all mutable state and processes `Command`s from an mpsc channel
  (`Handle::*` methods send commands and await a oneshot reply). This serializes
  all state mutation without locks. `handle_deploy` is the single choke point
  every deploy path flows through, so validation and persistence are centralized
  there.

Secondary indexes (`deployment_index`, `pods_by_deployment`) keep name/owner
lookups O(1). Persistence is best-effort and asynchronous: `persist_*` helpers
write through to the `StateStore` and, on failure, log a warning **and**
increment the `MetricsPort::record_persistence_error` counter rather than
aborting the in-memory operation.

## 3. nexad: the daemon

`nexad` wires the domain to the real world. It runs in one of two modes:

- **single-node** — one process is both control plane and worker.
- **cluster** — a control-plane node (master) schedules pods and assigns them to
  worker nodes over gRPC; workers run containers and heartbeat back.

### Adapters

| Port | Adapters |
|------|----------|
| `ContainerRuntime` | `docker` (bollard), `containerd` (`ctr` CLI) |
| `StateStore` | `sqlite` (sqlx) |
| `SecretStore` | encrypted SQLite (rusqlite + AES-256-GCM) |
| `DnsProvider` | `hickory`, `noop` |
| `ProxyManager` | `traefik` (default), `nginx`, `caddy` |
| `RouteStore` | `SqliteRouteStore` (routes, certificates, subnet allocations) |
| `ClusterTransport` | `grpc` (tonic, optional TLS), `local` |
| Networking | CNI bridge, WireGuard overlay (both experimental) |
| `MetricsPort` | Prometheus |

### HTTP API (`src/api/`)

An `axum` server exposes the REST API. Routes are split into **public**
(health, metrics) and **protected** (deploy, secrets, drain, token rotation).
Protected routes require a Bearer token, verified with Argon2id and a
constant-time comparison. The API binds to `127.0.0.1` by default. The daemon
performs a graceful shutdown on SIGINT/SIGTERM via a `CancellationToken`.

### Cluster gRPC (`src/cluster/`)

The control plane and workers communicate over the `ClusterService` defined in
`proto/cluster.proto`:

```
Register(RegisterRequest)              -> RegisterResponse   join the cluster
Heartbeat(stream Ping) -> stream Pong                        liveness + pending actions
AssignPod(AssignPodRequest)            -> AssignPodResponse  master -> worker: run a pod
StopPod / RemovePod                    -> *Response          lifecycle
ReportStatus(StatusReport)             -> google.protobuf.Empty
StreamLogs(LogsRequest) -> stream LogChunk                   forward container logs
```

Heartbeats are a bidirectional stream: a worker streams its status and resource
availability; the master replies with pending actions (start/stop/remove/restart
pods). When a worker stops heartbeating, the master reschedules its pods onto
healthy workers. gRPC can run over TLS using a self-signed CA and per-server
certificates generated with `rcgen`.

## 4. Request flow: `nexa deploy`

```
 nexa deploy app.yaml
      │  (HTTP POST /api/v1/deploy, Bearer token)
      ▼
 nexad API handler  ──►  Orchestrator::deploy (command)        [nexa-core]
      │                       │
      │                       ├─ spec.validate()               reject bad specs (400)
      │                       ├─ ensure project / upsert deployment
      │                       ├─ schedule pods (weighted scheduler)
      │                       └─ persist_* -> StateStore (SQLite)
      │                                          │
      │  single-node: run locally               │ cluster: AssignPod over gRPC
      ▼                                          ▼
 ContainerRuntime.create/start            worker nexad runs the container
      │
      ▼
 proxy + DNS updated for public deployments
```

## 5. Persistence

`nexad` uses **two** SQLite databases:

- `nexa.db` — cluster state (projects, deployments, pods, nodes, routes,
  certificates, subnet allocations). Managed by sqlx with versioned migrations
  under `nexad/migrations/`.
- `secrets.db` — application secrets, encrypted at rest (AES-256-GCM) with a
  master key. Managed separately via rusqlite.

Schema changes are forward-only migrations; never edit a released migration
(existing deployments verify checksums). The persisted state carries a
`STATE_SCHEMA_VERSION` for forward-compatibility checks.

## 6. Security model

- **API auth** — Bearer token, Argon2id-hashed, constant-time verification;
  default bind `127.0.0.1`.
- **Join tokens** — only the hash is persisted; the plaintext is shown once.
- **Secrets** — read from stdin (never CLI args); stored AES-256-GCM encrypted.
- **gRPC** — optional TLS (self-signed CA + server certs via `rcgen`).
- **CLI** — warns before sending sensitive data over non-localhost plain HTTP;
  HTTP client has connect/request timeouts.
- **Supply chain** — `install.sh` verifies SHA-256 checksums; releases publish
  `sha256sums.txt`; CI runs `cargo audit`.

## 7. Observability

- `nexad` exposes Prometheus metrics at `/metrics` (`nexa_*` series for HTTP,
  containers, scheduling, proxy, and gauges for node/pod/deployment counts).
- `deploy/prometheus/` ships a scrape config (static + file/DNS service
  discovery) and alert rules (error rate, latency, OOM, crash loops,
  split-brain, daemon-down, plus disk and TLS-expiry rules that depend on
  node_exporter / blackbox_exporter).
- `deploy/grafana/` ships a dashboard.
- Logs are structured via `tracing` (`tracing-subscriber`, JSON-capable).

## 8. Versioning

Crates are versioned independently. `nexad` and `nexa-cli` track the daemon/CLI
surface (currently `0.2.x`); `nexa-core` versions its domain/port API
(`0.1.x`). Downstream crates pin `nexa-core` by **git tag**, so the effective
contract is the tag, not the version field. When changing a port trait, prefer
additive, defaulted methods and bump `nexa-core`'s tag before bumping the
downstream pin. Pre-`1.0`, treat minor bumps as potentially breaking.
