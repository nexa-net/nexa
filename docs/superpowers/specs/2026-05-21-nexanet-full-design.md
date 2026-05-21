# NexaNet — Full Design Specification

**Date:** 2026-05-21
**Status:** Approved
**Scope:** 12 feature specs covering Phase 1 (single-node), Phase 2 (multi-node), Phase 3 (networking/routing)

## Architectural Decisions (Cross-Cutting)

| Decision | Choice | Rationale |
|----------|--------|-----------|
| CLI ↔ nexad protocol | HTTP/REST | Simple, curl-friendly, easy to debug |
| Node-to-node protocol | gRPC (tonic) | Efficient, typed, streaming for heartbeats |
| State persistence | SQLite on master (Raft replication deferred to Phase 4+) | Persistence without external DB; HA via openraft is a future concern |
| Secrets | Encrypted at rest in SQLite (AES-256-GCM) | Self-contained, no external deps |
| Proxy architecture | Separate sidecar process | Modular; multiple backends supported |
| Proxy backends | Custom Rust proxy (default) + Caddy + Traefik + Nginx | User choice; all managed by nexad |
| Orchestrator model | Actor with command channel | No locks, no block_on, clean async |
| Volumes | Named volumes + bind mounts | Simple for users, flexible for power users |
| Overlay networking | Embedded WireGuard via boringtun | Userspace, no kernel module, zero config |
| Code architecture | Hexagonal (Ports & Adapters) | Domain logic isolated from infrastructure; adapters are swappable |

---

## Hexagonal Architecture

NexaNet follows hexagonal architecture (ports & adapters). The domain core contains pure business logic with zero infrastructure dependencies. All external systems are accessed through port traits, with concrete adapters that can be swapped independently.

### Layers

```
                    ┌─────────────────────────────────┐
                    │         Driving Adapters          │
                    │  (HTTP API, gRPC, CLI)            │
                    └──────────┬──────────────────────┘
                               │ calls
                    ┌──────────▼──────────────────────┐
                    │       Driving Ports (in)          │
                    │  (OrchestratorHandle, traits)     │
                    ├──────────────────────────────────┤
                    │                                   │
                    │         Domain Core                │
                    │                                   │
                    │  - Orchestrator loop               │
                    │  - Deployment logic                │
                    │  - Pod lifecycle                   │
                    │  - Scheduling                      │
                    │  - Health check logic              │
                    │  - Restart policy logic            │
                    │  - Project isolation rules         │
                    │                                   │
                    ├──────────────────────────────────┤
                    │       Driven Ports (out)           │
                    │  (trait ContainerRuntime,          │
                    │   trait StateStore,                │
                    │   trait ProxyBackend,              │
                    │   trait DnsProvider,               │
                    │   trait SecretStore,               │
                    │   trait ClusterTransport)          │
                    └──────────┬──────────────────────┘
                               │ implemented by
                    ┌──────────▼──────────────────────┐
                    │        Driven Adapters            │
                    │  Docker, containerd, SQLite,      │
                    │  Caddy, Traefik, Nginx,           │
                    │  nexa-proxy, hickory-dns,         │
                    │  boringtun, tonic gRPC            │
                    └──────────────────────────────────┘
```

### Driven Ports (outbound traits)

The domain core defines these traits. It never imports a concrete adapter.

| Port | Trait | Purpose | Adapters |
|------|-------|---------|----------|
| Container Runtime | `ContainerRuntime` | Create, start, stop, inspect containers | `DockerRuntime`, `ContainerdRuntime`, `MockRuntime` |
| State Store | `StateStore` | Persist and query projects, deployments, pods | `SqliteStore`, `InMemoryStore` (tests) |
| Secrets | `SecretStore` | Encrypt/decrypt/store secrets | `EncryptedSqliteSecretStore`, `PlaintextSecretStore` (tests) |
| Proxy | `ProxyBackend` | Apply routes, reload, TLS | `NexaProxyBackend`, `CaddyBackend`, `TraefikBackend`, `NginxBackend` |
| DNS | `DnsProvider` | Register/deregister service records | `HickoryDnsProvider`, `NoopDnsProvider` (single-node) |
| Cluster Transport | `ClusterTransport` | Node registration, heartbeats, pod assignment | `GrpcTransport`, `LocalTransport` (single-node) |

### Driving Ports (inbound interfaces)

| Port | Interface | Callers |
|------|-----------|---------|
| `OrchestratorHandle` | mpsc command channel | HTTP API handlers, gRPC server |
| HTTP API | axum routes | CLI (`nexa`), external tools |
| gRPC API | tonic service | Worker nodes |

### Crate / Module Mapping

```
crates/
  nexa-core/
    src/
      domain/              ← Pure domain logic (NO infra imports)
        mod.rs
        orchestrator.rs    ← Actor loop, business rules
        scheduler.rs       ← Weighted scoring logic
        health.rs          ← Health check state machine
        restart.rs         ← Restart policy + backoff logic
        models/            ← Domain models (Deployment, Pod, Project, etc.)
      ports/               ← Trait definitions (interfaces)
        mod.rs
        runtime.rs         ← trait ContainerRuntime
        state.rs           ← trait StateStore
        secrets.rs         ← trait SecretStore
        proxy.rs           ← trait ProxyBackend
        dns.rs             ← trait DnsProvider
        cluster.rs         ← trait ClusterTransport
      config.rs            ← YAML parsing, validation
      error.rs             ← Error types

  nexad/
    src/
      adapters/            ← All infrastructure implementations
        runtime/
          docker.rs        ← DockerRuntime (bollard)
          containerd.rs    ← ContainerdRuntime (containerd-client + CNI)
        state/
          sqlite.rs        ← SqliteStore (sqlx)
        secrets/
          encrypted.rs     ← AES-256-GCM encrypted store
        proxy/
          nexa_proxy.rs    ← Custom Rust proxy management
          caddy.rs         ← Caddyfile generation + reload
          traefik.rs       ← Traefik YAML generation
          nginx.rs         ← Nginx conf generation + reload
        dns/
          hickory.rs       ← hickory-dns embedded server
        cluster/
          grpc.rs          ← tonic gRPC server + client
          local.rs         ← In-process local transport (single-node)
        network/
          wireguard.rs     ← boringtun overlay mesh
      api/                 ← HTTP API (driving adapter)
        handlers.rs
        routes.rs
      main.rs              ← Wires ports to adapters, starts daemon

  nexa-cli/                ← CLI driving adapter (unchanged)
    src/
      main.rs
      client.rs
      commands.rs
      output.rs

  nexa-proxy/              ← Standalone reverse proxy binary
    src/
      main.rs
      proxy.rs
      acme.rs
```

### Key Rules

1. **`nexa-core/src/domain/`** has zero `use` of bollard, sqlx, tonic, hickory, boringtun, or any infrastructure crate. It only depends on standard library, serde, chrono, uuid, and its own `ports/` traits.

2. **`nexa-core/src/ports/`** defines traits only. No implementations. No infrastructure imports.

3. **`nexad/src/adapters/`** implements the port traits. Each adapter depends on its specific infrastructure crate. Adapters are leaf modules — they don't import each other.

4. **`nexad/src/main.rs`** is the composition root. It wires concrete adapters to port traits and starts the system:

```rust
// main.rs — composition root
let runtime: Arc<dyn ContainerRuntime> = match cli.runtime {
    RuntimeChoice::Docker => Arc::new(DockerRuntime::new()?),
    RuntimeChoice::Containerd => Arc::new(ContainerdRuntime::new()?),
};

let state: Arc<dyn StateStore> = Arc::new(SqliteStore::new(&db_path).await?);
let secrets: Arc<dyn SecretStore> = Arc::new(EncryptedSqliteSecretStore::new(&master_key, pool.clone()));
let dns: Arc<dyn DnsProvider> = match cli.mode {
    Mode::SingleNode => Arc::new(NoopDnsProvider),
    _ => Arc::new(HickoryDnsProvider::new(port_53).await?),
};
let proxy: Arc<dyn ProxyBackend> = match proxy_config.backend {
    ProxyChoice::NexaProxy => Arc::new(NexaProxyBackend::new()?),
    ProxyChoice::Caddy => Arc::new(CaddyBackend::new(caddy_path)?),
    ProxyChoice::Traefik => Arc::new(TraefikBackend::new(traefik_path)?),
    ProxyChoice::Nginx => Arc::new(NginxBackend::new(nginx_path)?),
};

let handle = Orchestrator::spawn(runtime, state, secrets, dns, proxy);
api::serve(handle, &addr).await?;
```

5. **Testing:** Domain logic is tested with mock adapters (`MockRuntime`, `InMemoryStore`, etc.) — no Docker, no SQLite needed. Integration tests wire real adapters.

### Impact on Existing Specs

This architecture is compatible with all 12 specs. The main changes:

- **Spec #1** (Orchestrator): The actor loop lives in `domain/orchestrator.rs`. It receives port trait objects via constructor injection.
- **Spec #3** (SQLite): Becomes the `SqliteStore` adapter implementing `trait StateStore`. The domain never sees sqlx.
- **Spec #4** (Health Checking): Health check logic (state machine, threshold) lives in `domain/health.rs`. The HTTP probing is a utility in the domain (uses only `reqwest` or raw TCP — lightweight enough for domain).
- **Spec #5** (Restart): Business logic in `domain/restart.rs`. The event watcher adapter pushes events into the orchestrator via the command channel.
- **Spec #6** (Secrets): `trait SecretStore` in ports, `EncryptedSqliteSecretStore` adapter in nexad.
- **Spec #10** (DNS): `trait DnsProvider` in ports, `HickoryDnsProvider` adapter in nexad.
- **Spec #11** (Proxy): `trait ProxyBackend` already defined in the spec. Each backend is an adapter.
- **Spec #12** (Runtime): `trait ContainerRuntime` already exists. Docker and containerd are adapters.

---

## Spec #1: Orchestrator Async Redesign

### Problem

The current orchestrator uses `DashMap<Uuid, Arc<RwLock<T>>>` with multiple `block_on()` calls inside async contexts. This causes potential deadlocks, poor performance, and won't scale to multi-node. Every subsequent feature builds on this layer.

### Design

The orchestrator becomes a single `tokio::spawn`ed task that owns all state exclusively. External callers communicate via `tokio::sync::mpsc` command channel and receive responses through `oneshot` channels.

```
                  ┌─────────────┐
  API Handler ───▶│             │
                  │  mpsc::     │     ┌──────────────────────┐
  Health Loop ───▶│  Sender<    │────▶│  Orchestrator Loop   │
                  │  Command>   │     │  (owns all state)    │
  Scheduler  ───▶│             │     │  - projects: HashMap  │
                  └─────────────┘     │  - deployments: HashMap│
                                      │  - pods: HashMap      │
                        ▲             │  - runtime: Arc<dyn>  │
                        │             │  - db: SqlitePool     │
                  oneshot::Sender     └──────────────────────┘
                  (response back)
```

### Key Types

```rust
enum Command {
    Deploy { spec: DeploymentSpec, reply: oneshot::Sender<Result<Deployment>> },
    Stop { project: String, name: String, reply: oneshot::Sender<Result<()>> },
    Scale { project: String, name: String, replicas: u32, reply: oneshot::Sender<Result<Deployment>> },
    ListPods { project: Option<String>, reply: oneshot::Sender<Vec<Pod>> },
    ListDeployments { project: Option<String>, reply: oneshot::Sender<Vec<Deployment>> },
    PodLogs { project: String, name: String, tail: Option<u64>, reply: oneshot::Sender<Result<LogStream>> },
    CreateProject { name: String, reply: oneshot::Sender<Result<Project>> },
    ListProjects { reply: oneshot::Sender<Vec<Project>> },
    RemoveDeployment { project: String, name: String, reply: oneshot::Sender<Result<()>> },
    HealthReport { pod_id: Uuid, healthy: bool },
    ContainerExited { pod_id: Uuid, exit_code: i64 },
    RestartPod { pod_id: Uuid },
}

struct OrchestratorHandle {
    tx: mpsc::Sender<Command>,
}
```

`OrchestratorHandle` is `Clone + Send + Sync` — passed as axum state. Methods on it are thin wrappers that send a command and await the oneshot response.

### Benefits

- No locks, no `block_on`, no `DashMap`
- Single owner of mutable state — no data races by construction
- Easy to add persistence: the loop writes to SQLite after each mutation
- Easy to add events: the loop can emit to health checker, metrics, etc.
- `OrchestratorHandle` is trivially `Clone` for axum state sharing

### Hexagonal Integration

The orchestrator loop lives in `nexa-core/src/domain/orchestrator.rs`. It receives all external dependencies as port trait objects (`Arc<dyn ContainerRuntime>`, `Arc<dyn StateStore>`, etc.) via constructor injection. The loop has zero knowledge of Docker, SQLite, or any concrete adapter.

```rust
impl Orchestrator {
    pub fn spawn(
        runtime: Arc<dyn ContainerRuntime>,
        state: Arc<dyn StateStore>,
        secrets: Arc<dyn SecretStore>,
        dns: Arc<dyn DnsProvider>,
    ) -> OrchestratorHandle {
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let mut orch = Self { runtime, state, secrets, dns, /* in-memory maps */ };
            orch.run(rx).await;
        });
        OrchestratorHandle { tx }
    }
}
```

### Migration Path

Replace `Orchestrator` struct with `OrchestratorHandle` + background loop. Move domain logic to `nexa-core/src/domain/`, move Docker impl to `nexad/src/adapters/runtime/docker.rs`. API layer changes minimally.

---

## Spec #2: YAML Schema Design

### Problem

The `DeploymentSpec` needs to be the finalized, documented contract. Every feature adds fields. We nail it down now so all specs reference a stable format.

### Final Schema

```yaml
project: ecommerce

deployment:
  name: api

replicas: 3
image: ghcr.io/company/api:latest

ports:
  - 3000
  - 8080

env:
  NODE_ENV: production
  LOG_LEVEL: info

secrets:
  - DATABASE_URL
  - STRIPE_KEY

volumes:
  - name: data
    mount: /app/data
  - path: /host/uploads
    mount: /app/uploads
    readonly: true

network:
  public: true
  domain: api.example.com
  https: true

healthcheck:
  path: /health
  interval: 10s
  timeout: 5s
  retries: 3

restart: always

resources:
  memory: 512m
  cpu: 0.5
```

### Design Decisions

- **`secrets`** — list of secret *names*, not values. Values stored encrypted in NexaNet. Set via `nexa secret set`. Injected as env vars at runtime.
- **`volumes`** — two forms: named volumes (`name: data`) managed by NexaNet, and bind mounts (`path: /host/...`). Optional `readonly` flag.
- **`resources`** — optional. Only used for scheduling in Phase 2. Shorthand format (`512m`, `0.5` CPU cores).
- **`restart`** — simple string: `always`, `on-failure`, `never`.
- **`ports`** — container ports. Host mapping is automatic (same port for single-replica, random for multi-replica).
- **`env` vs `secrets`** — env is plaintext in YAML, secrets are references to encrypted values.

### Validation Rules

- `project` and `deployment.name`: required, match `[a-z0-9][a-z0-9-]*`, max 63 chars (DNS-safe)
- `image`: required, non-empty
- `replicas`: defaults to 1, must be >= 1
- `ports`: 1–65535
- `resources.memory`: parsed as bytes (`m`, `g` suffixes)
- `resources.cpu`: parsed as float (cores)

### Intentionally Excluded

- No `kind`/`apiVersion` — one resource type only
- No labels/annotations — projects handle grouping
- No affinity/tolerations — too complex
- No init containers or sidecars — YAGNI

---

## Spec #3: State Persistence (SQLite)

### Problem

All state is in-memory. Restarting nexad loses everything. We need durable persistence that the actor loop writes to after each mutation and reads from on startup.

### Design

SQLite via sqlx at `{data_dir}/nexa.db`. WAL mode for concurrent reads. The orchestrator loop accesses storage through the `StateStore` port trait — it never imports sqlx.

**Port trait** (in `nexa-core/src/ports/state.rs`):

```rust
#[async_trait]
pub trait StateStore: Send + Sync {
    // Projects
    async fn insert_project(&self, project: &Project) -> Result<()>;
    async fn get_project(&self, name: &str) -> Result<Option<Project>>;
    async fn list_projects(&self) -> Result<Vec<Project>>;
    async fn update_project_status(&self, name: &str, status: ProjectStatus) -> Result<()>;
    async fn delete_project(&self, name: &str) -> Result<()>;

    // Deployments
    async fn insert_deployment(&self, deployment: &Deployment) -> Result<()>;
    async fn get_deployment(&self, project: &str, name: &str) -> Result<Option<Deployment>>;
    async fn list_deployments(&self, project: Option<&str>) -> Result<Vec<Deployment>>;
    async fn update_deployment(&self, deployment: &Deployment) -> Result<()>;
    async fn delete_deployment(&self, id: &Uuid) -> Result<()>;

    // Pods
    async fn insert_pod(&self, pod: &Pod) -> Result<()>;
    async fn list_pods(&self, project: Option<&str>) -> Result<Vec<Pod>>;
    async fn update_pod(&self, pod: &Pod) -> Result<()>;
    async fn delete_pod(&self, id: &Uuid) -> Result<()>;
    async fn pods_by_deployment(&self, deployment_id: &Uuid) -> Result<Vec<Pod>>;
}
```

**Adapter** (in `nexad/src/adapters/state/sqlite.rs`): `SqliteStore` implements `StateStore` using sqlx.

### Schema

```sql
CREATE TABLE projects (
    name        TEXT PRIMARY KEY,
    status      TEXT NOT NULL DEFAULT 'active',
    created_at  TEXT NOT NULL
);

CREATE TABLE deployments (
    id          TEXT PRIMARY KEY,
    project     TEXT NOT NULL REFERENCES projects(name),
    name        TEXT NOT NULL,
    spec_json   TEXT NOT NULL,
    status      TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    UNIQUE(project, name)
);

CREATE TABLE pods (
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

CREATE TABLE secrets (
    project     TEXT NOT NULL,
    name        TEXT NOT NULL,
    value_enc   BLOB NOT NULL,
    nonce       BLOB NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    PRIMARY KEY (project, name)
);
```

### Write Path

```
Receive Command → Mutate in-memory state → Write to SQLite → Perform container action → Reply
```

If SQLite write fails, in-memory mutation is rolled back. Container actions happen after persistence.

### Startup Reconciliation

```
1. Open SQLite, run migrations (sqlx::migrate!())
2. Load all projects, deployments, pods into memory
3. For each pod with status Running:
   a. Query container runtime: container exists and running?
   b. If yes → keep Running
   c. If stopped/gone → mark Failed, trigger restart policy
4. Recalculate deployment statuses from pod states
5. Clean up orphaned containers (labeled managed-by=nexanet, no matching pod)
```

### Read Path

A read-only `SqlitePool` clone can be exposed for direct API reads, avoiding the actor bottleneck for list queries.

---

## Spec #4: Health Checking

### Problem

Pods can crash or become unresponsive without NexaNet knowing. We need active health monitoring that detects failures and feeds into the restart policy system.

### Design

A dedicated `tokio::spawn`ed health checker task runs alongside the orchestrator loop. It probes pods with healthcheck specs and reports results via the command channel.

### Health Check Type (Phase 1: HTTP only)

`GET http://<container_ip>:<first_port><path>` — 2xx means healthy. Anything else is a failure.

### State Machine Per Pod

```
Healthy ──(failure)──▶ Failing(count=1)
   ▲                        │
   │                   (failure)
(success)                   │
   │                   Failing(count=2)
   │                        │
   │                   (count >= retries)
   │                        ▼
   └──────────────── Unhealthy → triggers restart policy
```

Health state resets to Healthy on any success.

### How It Works

1. Health checker wakes every 1 second
2. Collects pods due for a check (based on interval)
3. Spawns parallel sub-tasks for each probe (respecting timeout)
4. Sends `Command::HealthReport { pod_id, healthy }` to orchestrator
5. Orchestrator updates tracking and triggers restart if threshold crossed

### Requirements

- `container_ip: Option<String>` added to `Pod` model, populated after creation via container inspect
- Health state tracked in-memory (transient, not persisted)
- On startup, all pods begin Healthy; checker starts probing
- Pods without healthcheck are assumed healthy while container is running

---

## Spec #5: Restart Policies

### Problem

When a pod fails, NexaNet marks it Failed and does nothing. We need automatic restart with backoff to avoid restart storms.

### Event Sources

1. **Health checker** reports `Unhealthy`
2. **Container event watcher** detects container exit

### Container Event Watcher

New `tokio::spawn`ed task that calls `runtime.events()` from the `ContainerRuntime` trait (Spec #12). Listens for `RuntimeEvent::ContainerDied` events for containers labeled `managed-by=nexanet`. Sends `Command::ContainerExited(pod_id, exit_code)` to orchestrator. This works for both Docker (bollard events stream) and containerd (task wait API).

### Decision Logic

```rust
fn should_restart(policy: RestartPolicy, exit_code: i64) -> bool {
    match policy {
        Never => false,
        Always => true,
        OnFailure => exit_code != 0,
    }
}
```

### Exponential Backoff

```
delay = min(1s * 2^restart_count, 5m)
```

Reset to 0 after 10 minutes of healthy runtime.

### Restart Flow

```
1. Receive ContainerExited or HealthReport(unhealthy)
2. Look up restart policy from deployment spec
3. should_restart() → false: mark Failed, done
4. Increment restart_count, persist to SQLite
5. Calculate backoff delay
6. Set pod status to Restarting
7. Schedule delayed Command::RestartPod via tokio::spawn + sleep
8. On RestartPod: remove old container, create new one
9. Success → Running, health checker starts monitoring
```

### Crash Loop Protection

After 10 consecutive restarts without 10 minutes healthy, pod is marked Failed permanently (`CrashLoopBackoff`). User must intervene.

### Per-Pod Tracking

```rust
struct RestartState {
    count: u32,
    last_restart: Option<DateTime<Utc>>,
    last_healthy_since: Option<DateTime<Utc>>,
}
```

`count` persisted in `pods.restart_count` column.

---

## Spec #6: Project System

### Problem

Projects are just a name string. They need to be the primary isolation boundary — logical, security, networking, deployment.

### Design

Every resource belongs to exactly one project. No global scope. Default project: `default` (auto-created on first use).

### Project Model

```rust
struct Project {
    name: String,           // [a-z0-9][a-z0-9-]*, max 63 chars
    created_at: DateTime<Utc>,
    status: ProjectStatus,  // Active, Suspended
}
```

### Isolation Rules

| Resource    | Isolation |
|-------------|-----------|
| Deployments | `project + name` is unique |
| Pods        | Inherit deployment's project |
| Secrets     | Per-project, same name in different projects are separate |
| Networks    | One Docker network per project: `nexa-{project}` |
| Volumes     | Prefixed: `nexa-{project}-{volume_name}` |
| Routes      | Domain bindings are global (domain → one project/deployment) |
| Logs        | Filtered by project |

### Secrets Management

```bash
nexa secret set DATABASE_URL "postgres://..." -p ecommerce
nexa secret list -p ecommerce
nexa secret rm DATABASE_URL -p ecommerce
```

**Encryption:** AES-256-GCM. Master key from `{data_dir}/master.key`, generated on first run (random 32 bytes, file permissions 0600). Optional: derive from passphrase via Argon2id.

```rust
struct SecretStore {
    cipher: Aes256Gcm,
}

impl SecretStore {
    fn encrypt(&self, plaintext: &[u8]) -> (Vec<u8>, Vec<u8>);  // (ciphertext, nonce)
    fn decrypt(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>>;
}
```

**Injection:** Orchestrator reads secrets from DB, decrypts, merges into container env vars. Secrets override `env` keys if same name. Missing secret → deploy fails with clear error.

### Project Lifecycle

```bash
nexa project create staging
nexa project list
nexa project suspend staging     # stop all pods, block new deploys
nexa project resume staging      # re-enable, reconcile
nexa project delete staging      # must be empty first
```

### Cross-Project Access

None by default. No linking mechanism. Network isolation enforced by Docker bridge separation.

---

## Spec #7: CLI UX & `nexa init`

### Problem

CLI is functional but bare-bones. No colors, no progress, no scaffolding. CLI UX is a major differentiator.

### Design

Use `console` crate for styling, `indicatif` for spinners/progress.

### Output Conventions

```
✓ success — green
✗ error — red
⠋ spinner during async ops
⚠ warning — yellow
```

Tables: whitespace-aligned columns, bold/dim header, colored status column.

```
NAME          PROJECT     STATUS    REPLICAS  IMAGE                          AGE
api           ecommerce   Running   3/3       ghcr.io/company/api:latest     2h
worker        ecommerce   Degraded  2/3       ghcr.io/company/worker:v2      45m
```

### `nexa init`

```bash
nexa init                    # interactive
nexa init myapp              # creates myapp/app.yaml
nexa init myapp --image nginx:alpine
```

Generated template includes commented-out optional sections (secrets, healthcheck, network). Post-init prints next-steps guidance.

### Deploy Progress (CLI Polls)

Deploy endpoint returns immediately (status: Pending). CLI polls pod status every 500ms until all Running or 60s timeout.

```
Deploying api to project 'ecommerce'...
  ✓ Pod api-0 running
  ✓ Pod api-1 running
  ✓ Pod api-2 running
✓ Deployment 'api' is running (3/3 replicas)
```

### `nexa status` (New Command)

```
Cluster: single-node
Projects: 3
Deployments: 5 (4 running, 1 stopped)
Pods: 12 (11 running, 1 restarting)
Runtime: Docker 24.0.7
```

### Error Messages

Every error tells what went wrong AND what to do:

```
✗ Secret 'DB_URL' referenced in app.yaml but not set
  Set it with: nexa secret set DB_URL "value" -p ecommerce
```

### `--json` Flag

Global flag on all commands for scripting:

```bash
nexa pods --json | jq '.[] | .status'
```

---

## Spec #8: Multi-Node Cluster

### Problem

NexaNet runs single-node. We need master/worker architecture for distributed orchestration.

### Topology

```
┌──────────────────────────────────┐
│           Master Node            │
│  nexad (master + local worker)   │
│  - HTTP API (6443)               │
│  - gRPC Server (6444)            │
│  - Scheduler                     │
│  - SQLite state                  │
└──────────┬───────────────────────┘
           │ gRPC
    ┌──────┴──────┐
┌───▼───┐   ┌────▼──┐
│Worker │   │Worker │
│Node 1 │   │Node 2 │
└───────┘   └───────┘
```

### Modes

```bash
nexad                          # single-node (default)
nexad --mode master            # master + local worker
nexad --mode worker --join <ip>:6444 --token <token>
```

### Join Tokens

Random 32-byte hex, prefixed `nxa_`. Stored hashed (SHA-256) in SQLite.

```bash
nexa cluster init              # generates token
nexa cluster token rotate
nexa cluster token show
```

### gRPC Service

```protobuf
service ClusterService {
    rpc Register(RegisterRequest) returns (RegisterResponse);
    rpc Heartbeat(stream HeartbeatPing) returns (stream HeartbeatPong);
    rpc AssignPod(AssignPodRequest) returns (AssignPodResponse);
    rpc StopPod(StopPodRequest) returns (StopPodResponse);
    rpc RemovePod(RemovePodRequest) returns (RemovePodResponse);
    rpc ReportStatus(StatusReport) returns (Empty);
    rpc StreamLogs(LogsRequest) returns (stream LogChunk);
}
```

### Node Model

```rust
struct Node {
    id: Uuid,
    name: String,           // hostname
    address: String,        // ip:port
    role: NodeRole,         // Master, Worker
    status: NodeStatus,     // Ready, NotReady, Draining
    resources: NodeResources,
    last_heartbeat: DateTime<Utc>,
    joined_at: DateTime<Utc>,
}
```

### Heartbeat Protocol

- Worker sends ping every 5s with resource usage + pod statuses
- No heartbeat for 30s → NotReady
- No heartbeat for 60s → all pods on node marked Failed, rescheduled

### Registration Flow

```
Worker sends Register(token, hostname, resources)
→ Master validates token hash
→ Creates Node in SQLite
→ Worker opens Heartbeat stream
→ Node is Ready
```

### Master as Worker

Master registers itself as in-process local worker. Single-node mode = master with only local worker.

### SQLite Additions

```sql
CREATE TABLE nodes (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    address         TEXT NOT NULL,
    role            TEXT NOT NULL,
    status          TEXT NOT NULL,
    cpu_cores       REAL NOT NULL,
    memory_bytes    INTEGER NOT NULL,
    joined_at       TEXT NOT NULL,
    last_heartbeat  TEXT NOT NULL
);

ALTER TABLE pods ADD COLUMN node_id TEXT REFERENCES nodes(id);

CREATE TABLE cluster_config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

### Node Management CLI

```bash
nexa nodes
nexa node drain worker-1
nexa node rm worker-1
```

---

## Spec #9: Scheduler (Weighted Scoring)

### Problem

With multiple nodes, we need to decide where pods run. Simple but intelligent.

### Design: Weighted Scoring Function

For each candidate node, compute a score. Highest score wins.

```
score = w_cpu    * (cpu_available / cpu_total)
      + w_memory * (memory_available / memory_total)
      - w_load   * (running_pods / max_pods)
      - w_fail   * failure_penalty(recent_failures)
```

All terms normalized to 0.0–1.0.

### Scoring Terms

| Term | Measures | Purpose |
|------|----------|---------|
| `cpu_available / cpu_total` | CPU headroom ratio | Favor nodes with more CPU |
| `memory_available / memory_total` | Memory headroom ratio | Favor nodes with more RAM |
| `running_pods / max_pods` | Pod density | Penalize overloaded nodes |
| `failure_penalty(recent_failures)` | Recent failures | Avoid unstable nodes |

### Failure Penalty (Exponential Decay)

```rust
fn failure_penalty(failures: &[DateTime<Utc>], now: DateTime<Utc>) -> f64 {
    failures.iter()
        .map(|t| {
            let age_minutes = (now - *t).num_minutes() as f64;
            (-age_minutes / 10.0).exp()
        })
        .sum::<f64>()
        .min(1.0)
}
```

### Default Weights

```
spread (default): cpu=0.35, memory=0.35, load=0.15, failure=0.15
binpack:          cpu=-0.30, memory=-0.30, load=-0.10, failure=0.15
```

Binpack inverts resource weights to prefer busy nodes.

### Configuration

```bash
nexa cluster config set scheduler spread
nexa cluster config set scheduler binpack
nexa cluster config set scheduler.weights.cpu 0.4
```

### Scheduler Trait

```rust
trait Scheduler: Send + Sync {
    fn select_node(&self, pod: &PodRequest, nodes: &[NodeSnapshot]) -> Result<Uuid>;
}

struct WeightedScheduler {
    weights: SchedulerWeights,
}
```

### Resource Accounting

Scheduling uses `reserved` (sum of placed pods' requests), not `used` (actual from heartbeat). Prevents over-commitment. Pods without resource spec treated as 0 (best-effort).

### Scheduling Failure

Pod stays Pending. Retried on each heartbeat (event-driven). CLI shows node scores in error output.

### Single-Node Mode

Scheduler exists but trivially returns the local node. Same code path.

---

## Spec #10: Service Discovery (Internal DNS)

### Problem

Containers need to find each other by name. We need `api.ecommerce.internal` to resolve automatically.

### Design: Embedded DNS Server

Lightweight DNS server inside nexad master using `hickory-dns`. Listens on port 53 (UDP + TCP).

### Naming Convention

```
<deployment>.<project>.internal          → round-robin across replicas
<deployment>-<index>.<project>.internal  → specific replica
```

### Record Store

```rust
struct DnsRecordStore {
    entries: HashMap<String, HashMap<String, Vec<IpAddr>>>,  // project → deployment → IPs
}
```

Updated event-driven: pod created → add IP, pod removed → remove IP.

### Container DNS Configuration

```rust
// Added to ContainerConfig
dns: Vec<String>,        // master node IP
dns_search: Vec<String>, // ["ecommerce.internal"] for short names
```

Containers in `ecommerce` can use `curl http://api:3000` (resolves via search domain).

### Cross-Project Resolution

DNS resolves `api.other-project.internal` to an IP, but Docker network isolation blocks the traffic. DNS is not a security boundary.

### Multi-Node Limitations (Before Overlay)

- Same-node: container IPs work via Docker bridge
- Cross-node: returns node IP + host port mapping (limited)
- Full container-IP resolution requires overlay network (Spec #11)

### External DNS Fallback

Queries not matching `*.internal` forwarded to system's upstream DNS.

---

## Spec #11: Networking & Routing

### Problem

Exposing services requires manual setup. We need `network: { public: true, domain: ..., https: true }` to just work.

### Three Layers

### Layer 1: Overlay Network (WireGuard)

Embedded userspace WireGuard via `boringtun` crate. Each node gets a WireGuard interface tunneling container traffic.

**Subnet allocation:** Master assigns each node a `/24` from cluster CIDR (default `172.20.0.0/16`).

```sql
CREATE TABLE subnet_allocations (
    node_id     TEXT NOT NULL REFERENCES nodes(id),
    project     TEXT NOT NULL,
    subnet      TEXT NOT NULL,
    PRIMARY KEY (node_id, project)
);
```

**Flow:**
1. Worker joins → master generates WireGuard keypair, assigns subnets
2. Master distributes peer configs via gRPC
3. Each node configures WireGuard interface via boringtun
4. Containers on different nodes reach each other by IP

Single-node: no WireGuard activated.

### Layer 2: Proxy Abstraction

```rust
#[async_trait]
trait ProxyBackend: Send + Sync {
    async fn apply_routes(&self, routes: &[RouteConfig]) -> Result<()>;
    async fn remove_route(&self, domain: &str) -> Result<()>;
    async fn reload(&self) -> Result<()>;
    async fn health(&self) -> Result<bool>;
}

struct RouteConfig {
    domain: String,
    upstream: Vec<Upstream>,
    tls: TlsConfig,
}

enum TlsConfig {
    None,
    Auto { email: String },
    Manual { cert: PathBuf, key: PathBuf },
}
```

**Four backends:**

| Backend | Management | TLS |
|---------|-----------|-----|
| `nexa-proxy` (default) | Child process, custom Rust proxy | Built-in ACME |
| `caddy` | Generate Caddyfile, signal reload | Native auto HTTPS |
| `traefik` | Generate YAML config, hot-reload | Built-in ACME |
| `nginx` | Generate conf.d/*.conf, `nginx -s reload` | Paired with certbot |

**nexa-proxy:** New crate `crates/nexa-proxy/`. Minimal reverse proxy using `hyper` + `rustls` + `instant-acme`. HTTP/1.1, HTTP/2, round-robin LB, health-aware routing, graceful reload.

```bash
nexa cluster config set proxy.backend nexa-proxy  # default
nexa cluster config set proxy.backend caddy
nexa cluster config set proxy.backend traefik
nexa cluster config set proxy.backend nginx
```

### Layer 3: TLS Automation

```
1. Deploy with https: true
2. Proxy serves HTTP on :80, responds to ACME challenges
3. Certificate issued, stored encrypted in SQLite
4. Proxy serves HTTPS on :443
5. Auto-renewal daily, 30 days before expiry
```

```sql
CREATE TABLE certificates (
    domain      TEXT PRIMARY KEY,
    cert_pem    BLOB NOT NULL,
    key_pem_enc BLOB NOT NULL,   -- encrypted with master key (same as secrets)
    key_nonce   BLOB NOT NULL,   -- 12-byte nonce for AES-256-GCM
    issued_at   TEXT NOT NULL,
    expires_at  TEXT NOT NULL,
    acme_account TEXT
);
```

```bash
nexa cluster config set proxy.acme.email admin@example.com
nexa cert import api.example.com --cert cert.pem --key key.pem
```

### Route Model

```sql
CREATE TABLE routes (
    domain      TEXT PRIMARY KEY,
    project     TEXT NOT NULL,
    deployment  TEXT NOT NULL,
    tls_mode    TEXT NOT NULL,
    created_at  TEXT NOT NULL
);
```

One domain → one deployment. Conflict on duplicate.

```bash
nexa routes
nexa route add api.example.com -p ecommerce --deployment api --https
nexa route rm api.example.com
```

### Port Allocation

Proxy listens on 80 (HTTP) and 443 (HTTPS). All routing is domain-based. Internal services reachable only via overlay + DNS.

---

## Spec #12: Container Runtime Abstraction (containerd)

### Problem

Only Docker is implemented. containerd support needed for lightweight/edge deployments.

### Runtime Auto-Detection

```
1. Check /run/containerd/containerd.sock → containerd available
2. Check /var/run/docker.sock → Docker available
3. Both → prefer Docker
4. Neither → fail with clear error
```

Override: `nexad --runtime docker` or `nexad --runtime containerd`.

### Key Differences

| Concern | Docker | containerd |
|---------|--------|------------|
| Image pull | bollard API | transfer service gRPC |
| Container create | single API call | container + task |
| Networking | built-in bridge | CNI plugins |
| Logs | streaming API | stdio fifos |

### CNI Integration

containerd has no built-in networking. nexad manages CNI configs per project:

```json
{
  "cniVersion": "1.0.0",
  "name": "nexa-ecommerce",
  "plugins": [
    { "type": "bridge", "bridge": "nexa-ecommerce", "isGateway": true,
      "ipam": { "type": "host-local", "subnet": "172.20.0.0/24" } },
    { "type": "loopback" }
  ]
}
```

CNI plugins at `{data_dir}/cni/bin/`. Missing → `nexa setup cni` downloads standard CNI plugins.

### Trait Additions

```rust
#[async_trait]
pub trait ContainerRuntime: Send + Sync {
    // ... existing methods unchanged ...

    // New
    async fn container_ip(&self, id: &str, network: &str) -> Result<IpAddr>;
    async fn events(&self) -> Result<EventStream>;
    fn runtime_name(&self) -> &'static str;
}

pub enum RuntimeEvent {
    ContainerDied { container_id: String, exit_code: i64 },
    ContainerStarted { container_id: String },
    ContainerOom { container_id: String },
}
```

- `container_ip` — needed by DNS server and health checker
- `events` — needed by restart policy system
- `runtime_name` — for `nexa status` display

### ContainerdRuntime

```rust
pub struct ContainerdRuntime {
    client: containerd_client::Client,
    cni: CniManager,
    namespace: String,  // "nexa"
}
```

All NexaNet containers in the `nexa` containerd namespace.

### Image Handling

containerd resolves Docker Hub images natively. Private registry auth via `{data_dir}/registries.json` (shared format for both runtimes).

### Logs

containerd: nexad configures log output to `{data_dir}/logs/{container_id}/stdout.log`. The `logs()` method tails these files.

### Testing

`MockRuntime` implementing the trait for unit tests. Integration tests run the same suite against both Docker and containerd.

---

## Dependency Graph

```
Spec #1 (Orchestrator Redesign)
  └──▶ Spec #3 (SQLite Persistence)
         ├──▶ Spec #4 (Health Checking)
         │      └──▶ Spec #5 (Restart Policies)
         ├──▶ Spec #6 (Project System)
         └──▶ Spec #2 (YAML Schema) ←── referenced by all
  
Spec #7 (CLI UX) ←── can proceed in parallel

Spec #8 (Multi-Node Cluster)
  └──▶ Spec #9 (Scheduler)
  └──▶ Spec #10 (Service Discovery)

Spec #11 (Networking & Routing) ←── depends on #8, #10
Spec #12 (Runtime Abstraction) ←── can proceed in parallel after #1
```

## Implementation Order

1. Spec #1 — Orchestrator Async Redesign
2. Spec #2 — YAML Schema Design
3. Spec #3 — State Persistence (SQLite)
4. Spec #4 — Health Checking
5. Spec #5 — Restart Policies
6. Spec #6 — Project System
7. Spec #7 — CLI UX & `nexa init`
8. Spec #8 — Multi-Node Cluster
9. Spec #9 — Scheduler (Weighted Scoring)
10. Spec #10 — Service Discovery
11. Spec #11 — Networking & Routing
12. Spec #12 — Container Runtime Abstraction
