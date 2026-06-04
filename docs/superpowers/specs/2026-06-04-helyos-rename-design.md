# Design — Rename NexaNet → Helyos

**Date**: 2026-06-04
**Scope**: Full project rename across 4 git repos (org, repo names, crates, binaries, every code/doc reference).

## 1. Goal

Rename the entire project from **NexaNet → Helyos**, leaving no `nexa` reference behind:
GitHub org, repo names, crate names, binary names, identifiers, env vars, metric
names, proto package, paths, namespaces, service files, docs. Clean break — no
backward-compatibility shims.

## 2. Confirmed decisions

| Decision | Choice |
|---|---|
| GitHub org | `nexa-net` → **`helyos`** |
| CLI command/binary | `nexa` → **`helyos`** |
| Crates / repos / daemon | `helyos-core`, `helyosd`, `helyos-cli`, meta repo `helyos` |
| GitHub execution | **User renames the org** (web UI, owner-only); assistant does everything else (repo renames via `gh repo rename`, code, docs, re-tag/re-release) |
| Backward compatibility | **None** — old `NEXA_*` / `~/.nexa` are no longer read |
| Versions | **Continuity** — `helyos-core` v0.1.6 → **v0.1.7**; `helyosd`/`helyos-cli` stay `0.2.x` with the dependency tag bumped |

## 3. Canonical token map (applied everywhere)

| Current | New |
|---|---|
| `nexa-net` (org) | `helyos` |
| `NexaNet` / `nexanet` (display/slug) | `Helyos` / `helyos` |
| repos `nexa`, `nexa-core`, `nexad`, `nexa-cli` | `helyos`, `helyos-core`, `helyosd`, `helyos-cli` |
| binaries `nexa`, `nexad` | `helyos`, `helyosd` |
| crate idents `nexa_core`, lib `nexad` | `helyos_core`, `helyosd` |
| types `NexaError`, `NexaClient`, `Nexa*` | `HelyosError`, `HelyosClient`, `Helyos*` |
| env vars `NEXA_API_TOKEN`, `NEXA_CONFIG`, `NEXA_HOME`, `NEXA_ICONS`, `NEXA_NAMESPACE`, `NEXA_SERVER`, `NEXA_TEST_RUNTIME` | `HELYOS_*` (same suffixes) |
| metrics `nexa_*` (`nexa_http_requests_total`, `nexa_nodes_total`, `nexa_pods_total`, `nexa_deployments_total`, `nexa_deployment_ops_total`, `nexa_container_events_total`, `nexa_schedule_duration_seconds*`, `nexa_proxy_requests_total`, `nexa_proxy_errors_total`, `nexa_proxy_request_duration_seconds*`, `nexa_http_request_duration_seconds*`) | `helyos_*` (same suffixes) |
| alert rules `Nexa*` (`NexaHighErrorRate`, `NexaContainerOOM`, `NexaNodeDown`, `NexaHighAPILatency`, `NexaProxyUpstreamErrors`, `NexaSchedulerSlow`, `NexaDaemonDown`, `NexaContainerCrashLoop`, `NexaClusterSplitBrain`, `NexaNodeDiskSpace*`, `NexaTlsCertExpiring*`) | `Helyos*` |
| proto package `nexa.cluster.v1` (+ `include_proto!`) | `helyos.cluster.v1` |
| containerd namespace `"nexa"` | `"helyos"` |
| paths `~/.nexa`, `/var/lib/nexa`, `nexa.db` | `~/.helyos`, `/var/lib/helyos`, `helyos.db` |
| DNS `*.nexa.local`, `nexad.service.nexa.local` | `*.helyos.local`, `helyosd.service.helyos.local` |
| services `nexad.service`, `nexa.nexad.plist` | `helyosd.service`, `helyos.helyosd.plist` |
| dependency URLs `github.com/nexa-net/nexa-core` | `github.com/helyos/helyos-core` |
| files `deploy/grafana/nexanet-dashboard.json` | `deploy/grafana/helyos-dashboard.json` |
| LICENSE / NOTICE copyright holder `NexaNet` | `Helyos` |

## 4. Per-repo scope

- **helyos-core** (was nexa-core): `Cargo.toml` name → `helyos-core`, version → 0.1.7; all `nexa_core`→`helyos_core`, `NexaError`/`Nexa*` types; env-var reads; docs/README; CI workflow. Self-contained (no upstream dep) → renamed and tagged first.
- **helyosd** (was nexad): `Cargo.toml` package/lib/bin name → `helyosd`; dependency `nexa-core`→`helyos-core` (new URL + tag v0.1.7); imports `nexa_core::`→`helyos_core::`; metric names; proto package + `include_proto!`; containerd namespace; `nexa.db`→`helyos.db`; env vars; systemd/plist; README; CI.
- **helyos-cli** (was nexa-cli): `Cargo.toml` package name → `helyos-cli`, bin name → `helyos`; dependency `helyos-core` (new URL + tag); imports; env vars (`HELYOS_SERVER`/`HELYOS_API_TOKEN`/`HELYOS_CONFIG`/`HELYOS_ICONS`); config dir `~/.nexa`→`~/.helyos`; README; CI.
- **helyos** (was nexa, meta): READMEs, `install.sh` (binary names, service files, paths, download URLs), `deploy/prometheus/*` (metric + alert names, scrape comments), `deploy/grafana/*` (dashboard file + content), `docs/architecture.md`, `docs/audit/*`, CONTRIBUTING, LICENSE/NOTICE.

**Historical docs** (`docs/superpowers/specs/`, `docs/superpowers/plans/`): dated design records. **Decision: included** — they get the same mechanical token replacement for the project identity (so `git grep -i nexa` is empty everywhere), but are otherwise preserved (not rewritten on substance; dates and historical context stay).

## 5. Sequencing (approach A — GitHub-first, then code in dependency order)

1. **User** renames the GitHub org `nexa-net` → `helyos` (Settings → Rename organization). GitHub keeps redirects from old URLs.
2. **Assistant** renames the 4 repos via `gh repo rename` (`nexa-core`→`helyos-core`, `nexad`→`helyosd`, `nexa-cli`→`helyos-cli`, `nexa`→`helyos`) and updates each local `git remote set-url origin`.
3. **helyos-core**: rename code on a branch → PR → green CI → merge → tag **v0.1.7** + release.
4. **helyosd**: rename code + point dep at `helyos-core` v0.1.7 (new URL) → `cargo update` → PR → green CI → merge.
5. **helyos-cli**: same as helyosd → PR → green CI → merge.
6. **helyos (meta)**: rename docs/install/deploy → PR → merge.

Each crate stays buildable at every step; the org/repo redirects mean old URLs keep resolving during the transition.

## 6. Verification

- Per crate: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo audit` (with the existing documented ignores) all green; main-branch CI green after merge.
- Smoke: `helyos --help`, `helyos completions bash`, `helyosd --help`.
- Final sweep: `git grep -i nexa` returns **nothing** in any of the 4 repos (active code, docs, and historical records).

## 7. Risks & mitigations

- **Broken git-tag deps mid-rename** → mitigated by GitHub redirects + doing helyos-core (+ its tag) before the consumers.
- **Crate-name change is breaking** → consumers update `Cargo.toml` dep name + `helyos_core::` imports atomically in the same PR as the tag bump.
- **Over-eager replacement** corrupting unrelated strings (e.g., the word "annexe", or "next") → replacements are anchored to the specific token shapes in §3 (not a blind `s/nexa/helyos/`), and verified by per-crate compile + tests.
- **Org name availability** → `helyos` must be free/owned on GitHub before the user renames (user's responsibility).

## 8. Out of scope

- External references the project doesn't control (existing clones, anyone depending on the old crate URLs) — covered by GitHub redirects, not by this change.
- Publishing to crates.io (project uses git deps, not crates.io).
</content>
