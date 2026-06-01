# NexaNet Testing Suite Design Spec

## Overview

Add a comprehensive, layered test suite across all NexaNet repositories: integration tests, end-to-end tests with real Docker containers, and Criterion performance benchmarks with automated regression detection in CI.

**Current state:** 349 unit tests + 7 integration tests (ignored in CI). Zero benchmarks. Very light CLI and proxy test coverage.

**Target state:** Integration tests running in CI with real Docker, full-stack E2E scenarios, Criterion benchmarks with CI regression gates, and significantly improved coverage for nexa-cli.

## Scope

This spec covers testing infrastructure only. Observability and Prometheus integration are a separate spec.

**Repositories affected:** nexa-core, nexad, nexa-cli.

---

## Layer 1: Integration Tests

### nexa-core

Current coverage is strong (177 unit tests covering orchestrator, scheduler, health, restart, models). No new integration tests needed. The existing mock implementations (MockRuntime, ConfigurableMockRuntime) are sufficient.

### nexad

#### API Integration Tests

**File:** `tests/api_integration.rs`

Test the full HTTP API stack (axum router + handlers + SQLite store) without requiring Docker. Use MockRuntime for the container runtime and a temporary SQLite database for state.

**Setup pattern:**
- Create a temporary directory for SQLite
- Build the axum router with real SqliteStore + MockRuntime + EncryptedSqliteSecretStore
- Spawn the axum router on a real TCP listener bound to `127.0.0.1:0` (random port)
- Each test gets a fresh database

**Test scenarios:**

| Scenario | Endpoints tested |
|---|---|
| Project CRUD | POST/GET/DELETE `/api/v1/projects`, suspend/resume |
| Deploy lifecycle | POST `/api/v1/deploy`, GET deployments, scale, stop, DELETE |
| Pod listing | GET `/api/v1/pods` with and without project filter |
| Secrets CRUD | POST/GET/DELETE `/api/v1/projects/{p}/secrets/{n}` |
| Route management | POST/GET/DELETE `/api/v1/routes` |
| Certificate import | POST `/api/v1/certs/import` |
| Health endpoint | GET `/health` returns 200 |
| Scheduler config | GET/POST `/api/v1/cluster/scheduler` |
| Error responses | 404 on unknown project, 400 on invalid deploy spec |

#### Runtime Integration Tests (activate in CI)

The existing `tests/runtime_integration.rs` (6 tests) already tests Docker container lifecycle. Currently ignored in CI.

**Change:** Add a new CI job `integration` that runs these tests with real Docker:

```yaml
integration:
  name: Integration
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - name: Install protoc
      run: sudo apt-get update && sudo apt-get install -y protobuf-compiler
    - uses: dtolnay/rust-toolchain@stable
    - uses: Swatinem/rust-cache@v2
    - run: docker pull busybox:latest
    - run: cargo test --test runtime_integration -- --ignored
    - run: cargo test --test api_integration
```

#### SQLite Integration Enrichment

**File:** `tests/sqlite_integration.rs` (extend existing)

Add scenarios:
- Cascade delete: delete project, verify deployments and pods are gone
- Concurrent writes: spawn multiple tokio tasks inserting pods simultaneously
- Node CRUD: create, update status, drain, remove
- Route store: insert, list with project filter, delete, certificate operations
- Large dataset: insert 1000 pods, verify list and filter performance is acceptable (< 100ms)

### nexa-cli

**File:** `src/commands/mod.rs` (inline tests) and `tests/cli_integration.rs`

**Command parsing tests** (unit, inline in each command module):
- Each subcommand parses valid arguments correctly
- Required arguments fail with helpful errors
- `--json` flag is recognized on all commands
- `--server` flag overrides default URL
- `--project` / `-p` shorthand works

**Output formatting tests** (unit, in `src/output/`):
- Table rendering produces expected column layout
- JSON output is valid JSON matching expected schema
- Age formatting for various durations (already has 6 tests, add edge cases)
- Status colorization maps correct colors to pod/node statuses

---

## Layer 2: End-to-End Tests

**File:** `nexad/tests/e2e.rs`

Full-stack tests that start a real nexad instance with Docker runtime, perform operations via HTTP, and verify real containers are created/destroyed.

### Setup

```rust
struct TestServer {
    addr: SocketAddr,
    shutdown: tokio::sync::oneshot::Sender<()>,
    data_dir: tempfile::TempDir,
}
```

- Start nexad's full server stack (API + orchestrator + Docker runtime) on a random port
- Use a temporary data directory for SQLite + secrets
- Wait for `/health` to return 200 before running tests
- On drop: send shutdown signal, clean up any leftover Docker containers with a `nexa-test-` prefix

### Container naming

All E2E test containers use the prefix `nexa-test-` followed by a UUID. Teardown cleans up any containers matching this prefix to prevent leaks.

### Test scenarios

#### 1. Deploy Lifecycle

1. POST `/api/v1/projects` to create project `test-{uuid}`
2. POST `/api/v1/deploy` with a spec: `busybox:latest`, 1 replica, command `sleep 3600`
3. GET `/api/v1/pods?project=test-{uuid}` and verify 1 pod with status `Running`
4. Verify the Docker container actually exists via `docker ps`
5. POST `.../stop` to stop the deployment
6. Verify pod status is `Stopped` and container is removed
7. DELETE the deployment, DELETE the project
8. Verify no orphan containers remain

#### 2. Scale Up/Down

1. Deploy with 1 replica
2. Scale to 3, wait, verify 3 pods running + 3 Docker containers
3. Scale to 1, wait, verify 2 pods removed + 2 Docker containers removed
4. Cleanup

#### 3. Health Check and Restart

1. Deploy a container with a healthcheck pointing to an invalid HTTP path
2. Set `restart: on_failure` in the spec
3. Wait for the health checker to detect failure and trigger restart
4. Verify the pod was restarted (restart count > 0 or new container ID)

#### 4. Secrets Injection

1. Create project, set a secret `MY_SECRET=testvalue123`
2. Deploy `busybox:latest` with command `env` and capture logs
3. Verify `MY_SECRET=testvalue123` appears in the container's environment

#### 5. Route Management (no proxy needed)

1. POST `/api/v1/routes` to add a route `test.example.com` for a deployment
2. GET `/api/v1/routes` and verify it appears
3. DELETE `/api/v1/routes/test.example.com` and verify it's gone

### CI Configuration

```yaml
e2e:
  name: E2E
  runs-on: ubuntu-latest
  needs: [check, test]
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

E2E tests run with `--test-threads=1` to avoid Docker resource contention. Only on push to `main`, not on PRs (too slow).

All E2E tests are marked `#[ignore]` so `cargo test` locally skips them by default.

---

## Layer 3: Performance Benchmarks (Criterion)

### nexa-core benchmarks

**File:** `benches/scheduler.rs`

| Benchmark | Parameters |
|---|---|
| `schedule_spread` | 10, 100, 1000 pods on 5 nodes |
| `schedule_binpack` | 10, 100, 1000 pods on 5 nodes |
| `schedule_with_weights` | 100 pods on 20 nodes with varying weights |

**File:** `benches/orchestrator.rs`

| Benchmark | What it measures |
|---|---|
| `command_dispatch` | Throughput of sending commands through the orchestrator mpsc channel |
| `state_query` | Time to query deployment/pod status from in-memory state |

**File:** `benches/config_parsing.rs`

| Benchmark | Parameters |
|---|---|
| `parse_minimal_spec` | Minimal YAML with just image + name |
| `parse_full_spec` | Full YAML with healthcheck, volumes, env, network |

### nexad benchmarks

**File:** `benches/sqlite_store.rs`

| Benchmark | Parameters |
|---|---|
| `insert_pod` | Insert 1 pod (measures single-row insert) |
| `list_pods_100` | List pods from a table with 100 rows |
| `list_pods_1000` | List pods from a table with 1000 rows |
| `update_pod_status` | Update single pod status in 1000-row table |
| `insert_deployment` | Insert 1 deployment |
| `query_deployments_by_project` | Filter deployments by project name |

**File:** `benches/crypto.rs`

| Benchmark | Parameters |
|---|---|
| `encrypt_64b` | AES-256-GCM encrypt 64 bytes |
| `encrypt_1kb` | AES-256-GCM encrypt 1 KB |
| `encrypt_64kb` | AES-256-GCM encrypt 64 KB |
| `decrypt_64b` | AES-256-GCM decrypt 64 bytes |
| `decrypt_1kb` | AES-256-GCM decrypt 1 KB |

**File:** `benches/dns.rs`

| Benchmark | Parameters |
|---|---|
| `lookup_10_records` | DNS lookup with 10 registered services |
| `lookup_100_records` | DNS lookup with 100 registered services |
| `lookup_1000_records` | DNS lookup with 1000 registered services |
| `register_deregister` | Register + deregister throughput |

### Cargo.toml changes

Each repo adds to `[dev-dependencies]`:

```toml
criterion = { version = "0.5", features = ["html_reports"] }
```

And a `[[bench]]` section per benchmark file:

```toml
[[bench]]
name = "scheduler"
harness = false
```

### CI Regression Detection

**File:** `.github/workflows/bench.yml` (in each repo that has benchmarks)

```yaml
name: Benchmarks

on:
  push:
    branches: [main]

jobs:
  bench:
    name: Performance
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Run benchmarks
        run: cargo bench --bench '*' -- --output-format bencher | tee output.txt
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

This uses `github-action-benchmark` to:
- Parse Criterion output
- Compare against previous runs stored in the `gh-pages` branch
- Fail CI if any benchmark regresses more than 15%
- Auto-push new baseline results

---

## CI Workflow Summary

After implementation, each repo's CI pipeline will have these jobs:

### nexa-core

| Job | Trigger | Tests |
|---|---|---|
| `check` | push + PR | fmt, clippy |
| `test` | push + PR | `cargo test` (all unit tests) |
| `bench` | push to main only | Criterion benchmarks with regression gate |

### nexad

| Job | Trigger | Tests |
|---|---|---|
| `check` | push + PR | fmt, clippy (with protoc) |
| `test` | push + PR | `cargo test --lib` (unit tests) |
| `integration` | push + PR | API integration + runtime integration (with Docker) |
| `e2e` | push to main only | Full-stack E2E with Docker (--test-threads=1) |
| `bench` | push to main only | Criterion benchmarks with regression gate |

### nexa-cli

| Job | Trigger | Tests |
|---|---|---|
| `check` | push + PR | fmt, clippy |
| `test` | push + PR | `cargo test` (all tests including new command/output tests) |

---

## Dependencies Added

| Repo | Crate | Purpose |
|---|---|---|
| nexad | `reqwest = "0.12"` (dev) | HTTP client for API/E2E tests |
| nexad | `criterion = "0.5"` (dev) | Performance benchmarks |
| nexad | `tokio-test = "0.4"` (dev) | Async test utilities |
| nexa-core | `criterion = "0.5"` (dev) | Performance benchmarks |

---

## Out of Scope

- Code coverage reporting (can be added later with `cargo-llvm-cov`)
- Mutation testing
- Fuzz testing
- Load/stress testing (beyond Criterion micro-benchmarks)
- nexa-cli E2E tests (would need a running nexad, better tested from nexad's E2E suite)
- Observability/Prometheus (separate spec)
