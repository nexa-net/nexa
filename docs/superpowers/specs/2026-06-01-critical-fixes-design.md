# Critical Fixes Design — 8 Remaining CRITICAL Issues

**Date**: 2026-06-01
**Scope**: helyosd, helyos-cli, install.sh, CI workflows
**Ref**: `docs/audit/production-readiness-2026-05-26.md`

---

## Overview

8 CRITICAL issues remain after helyos-proxy removal. Grouped into 3 blocs:

| Bloc | Issues | Crates |
|------|--------|--------|
| 1 — Security | API auth, bind address, join token hash, secrets stdin | helyosd, helyos-cli |
| 2 — Correctness | stop/remove pod, graceful shutdown | helyosd |
| 3 — Supply chain | install.sh checksum, release signing | infra |

---

## Bloc 1 — Security (CRITICAL 1-4)

### CRITICAL 1: Bearer token authentication on HTTP API

**Problem**: All management endpoints (deploy, secrets, drain, token rotation) accessible without authentication.

**Solution**: Axum middleware extracting `Authorization: Bearer <token>` from request headers.

- **Token lifecycle**: Generated at first startup via `OsRng` (32 bytes, hex-encoded). Stored as argon2 hash in SQLite (`cluster_config` table, key `api_token_hash`). Displayed once at first startup in logs and stdout.
- **Override**: `helyosd --api-token <token>` sets a specific token (hashed before storage). Useful for CI/automation.
- **Excluded routes**: `/health` and `/metrics` bypass auth (health checks and monitoring must remain open).
- **Middleware placement**: Applied via `axum::middleware::from_fn_with_state` on all `/api/v1/*` routes.
- **Error response**: 401 Unauthorized with JSON body `{"error": "missing or invalid bearer token"}`.
- **Files**: `helyosd/src/api/auth.rs` (new), `helyosd/src/api/routes.rs`, `helyosd/src/api/mod.rs`, `helyosd/src/main.rs`
- **Dependencies**: `argon2` crate for hashing.

### CRITICAL 2: Default bind to 127.0.0.1

**Problem**: API listens on `0.0.0.0` by default — combined with zero auth, the daemon is open to the network.

**Solution**: Change `default_value = "0.0.0.0"` to `default_value = "127.0.0.1"` in `Cli` struct.

- Users can explicitly bind to `0.0.0.0` with `--host 0.0.0.0` when needed.
- Same change for DNS listen address: `0.0.0.0:15353` → `127.0.0.1:15353`.
- **Files**: `helyosd/src/main.rs` (2 lines)

### CRITICAL 3: Hash join token before storage

**Problem**: `cluster_init` stores the join token in plaintext in SQLite alongside the hash, defeating the purpose of hashing.

**Solution**: Only persist the hash. Show the plaintext token exactly once at creation.

- `cluster_init`: Generate token, persist only `join_token_hash`. Return plaintext in response. Remove the `set_cluster_config("join_token", &token)` call.
- `cluster_token_show`: Return `{"error": "token is only shown at creation. Use /api/v1/cluster/token/rotate to generate a new one."}` with 410 Gone.
- `cluster_token_rotate`: Generate new token, persist new hash, return plaintext once.
- **Files**: `helyosd/src/api/handlers.rs`

### CRITICAL 4: Read secrets from stdin instead of CLI arguments

**Problem**: `helyos secret set DB_PASS s3cret -p myapp` exposes the secret in `ps aux`, shell history, and audit logs.

**Solution**: Make secret value an optional flag. Read from stdin when not provided.

- Change `value: String` (positional) to `#[arg(long)] value: Option<String>` in `SecretCommands::Set`.
- When `value` is `None`: if stdin is a TTY, prompt interactively (hidden input via `rpassword`); if stdin is a pipe, read one line.
- Usage: `echo $SECRET | helyos secret set DB_PASS -p myapp` or `helyos secret set DB_PASS -p myapp` (interactive prompt).
- Deprecation: if a positional value is still passed, accept it with a stderr warning "passing secrets as arguments is deprecated, use stdin or --value".
- **Files**: `helyos-cli/src/main.rs`, `helyos-cli/src/commands/secret.rs`
- **Dependencies**: `rpassword` crate for hidden TTY input.

---

## Bloc 2 — Correctness (CRITICAL 5-6)

### CRITICAL 5: Implement stop_pod and remove_pod in ClusterServer

**Problem**: Both methods return `success: true` without doing anything. Pods are never actually stopped or removed on workers.

**Solution**: Forward to the container runtime.

- `stop_pod`: Call `self.runtime.stop_container(&req.pod_id).await`. Update pod state to `Stopped` in state store. Return error if container not found.
- `remove_pod`: Call `self.runtime.stop_container(&req.pod_id).await` (if running), then `self.runtime.remove_container(&req.pod_id).await`. Remove pod from state store. Return error if not found.
- Error mapping: Runtime errors → `tonic::Status::internal(...)`.
- **Files**: `helyosd/src/cluster/server.rs`

### CRITICAL 6: Graceful shutdown

**Problem**: No SIGTERM/SIGINT handling. Kill leaves containers orphaned and state inconsistent.

**Solution**: `CancellationToken` pattern with coordinated shutdown.

- Create a `CancellationToken` at startup in `main()`.
- Spawn a signal handler task: `tokio::signal::ctrl_c()` + `SIGTERM` → cancel the token.
- Pass the token to: axum server (via `axum::serve(...).with_graceful_shutdown()`), gRPC server (via `tonic`'s graceful shutdown), health checker, heartbeat sender, DNS server, ACME renewal task.
- Shutdown sequence: (1) stop accepting new connections, (2) wait for in-flight requests (30s timeout), (3) cancel background tasks, (4) flush state to SQLite, (5) exit.
- **Files**: `helyosd/src/main.rs` (all 3 modes: single, master, worker)
- **Dependencies**: `tokio-util` for `CancellationToken` (likely already present).

---

## Bloc 3 — Supply Chain (CRITICAL 7-8)

### CRITICAL 7: Checksum verification in install.sh

**Problem**: Binaries downloaded and executed without integrity verification.

**Solution**: Publish `sha256sums.txt` with releases. Verify in install script.

- Release workflow generates `sha256sums.txt` listing all release artifacts.
- `install.sh` downloads both the binary and `sha256sums.txt`, verifies with `sha256sum -c` (Linux) or `shasum -a 256 -c` (macOS).
- Abort with clear error if checksum fails.
- **Files**: `install.sh`, `.github/workflows/release.yml` (in helyosd repo)

### CRITICAL 8: Sign release artifacts

**Problem**: No cryptographic signature on release artifacts.

**Solution**: Cosign keyless signing via GitHub Actions OIDC.

- Add `sigstore/cosign-installer` action to the release workflow.
- After building artifacts, sign each with `cosign sign-blob --yes --oidc-issuer=https://token.actions.githubusercontent.com`.
- Upload `.sig` and `.bundle` files alongside binaries.
- `install.sh` optionally verifies cosign signature if `cosign` is available on the system.
- **Files**: `.github/workflows/release.yml` (in helyosd repo), `install.sh`

---

## Implementation order

1. CRITICAL 2 (bind address) — 1 minute, zero risk
2. CRITICAL 3 (join token hash) — small, isolated change
3. CRITICAL 1 (auth middleware) — core security, needs new file
4. CRITICAL 4 (secrets stdin) — helyos-cli change, independent
5. CRITICAL 5 (stop/remove pod) — isolated to cluster server
6. CRITICAL 6 (graceful shutdown) — touches main.rs in all modes
7. CRITICAL 7 (checksums) — infra, independent
8. CRITICAL 8 (cosign signing) — infra, depends on 7

---

## Testing strategy

- **CRITICAL 1**: Unit test the auth middleware (valid token, invalid token, missing header, excluded routes).
- **CRITICAL 2**: Verify CLI parsing default is `127.0.0.1`.
- **CRITICAL 3**: Unit test that `cluster_init` only stores hash, `cluster_token_show` returns 410.
- **CRITICAL 4**: Test stdin reading with piped input and `--value` flag.
- **CRITICAL 5**: Unit test `stop_pod`/`remove_pod` with mock runtime.
- **CRITICAL 6**: Integration test that SIGTERM triggers clean shutdown.
- **CRITICAL 7-8**: Manual verification of release workflow output.
