# NexaNet → Helyos Rename — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rename the entire project NexaNet → Helyos across all 4 repos (org, repos, crates, binaries, every identifier/env/metric/proto/path/doc), leaving zero `nexa` references.

**Architecture:** GitHub-first then code in dependency order (`helyos-core` → tag → `helyosd`/`helyos-cli` → meta). A single shared, ordered token-replacement script is applied per repo; per-crate `cargo build`/`test`/`clippy`/`fmt`/`audit` + a final `git grep` prove completeness. Clean break (no compat), version continuity (`helyos-core` v0.1.7; daemons stay 0.2.x with the dep tag bumped).

**Tech Stack:** Rust (cargo, git deps by tag), tonic/prost (proto), GitHub (gh CLI), perl for replacements.

Repo paths (local): `.`=meta, `nexa-core`, `nexad`, `nexa-cli` (directory names stay local; only GitHub repo names + remotes change).

---

## Task 0 (PREREQUISITE — user action, blocking)

The GitHub **org rename is owner-only via web UI** and cannot be scripted.

- [ ] **User** renames the org `nexa-net` → `helyos` (GitHub → org Settings → Rename organization). Confirm `helyos` is available/owned first. GitHub keeps redirects from old URLs.
- [ ] User signals "org renamed, go". Do not start Task 1 before this.

---

## Task 1: Rename GitHub repos, update remotes, create the replacement script

**Files:** Create `/tmp/helyos-rename.pl` (shared replacement tool, not committed).

- [ ] **Step 1: Rename the 4 repos via gh** (org is now `helyos`)

```bash
gh repo rename helyos-core --repo helyos/nexa-core --yes
gh repo rename helyosd     --repo helyos/nexad     --yes
gh repo rename helyos-cli  --repo helyos/nexa-cli  --yes
gh repo rename helyos      --repo helyos/nexa      --yes
```

- [ ] **Step 2: Point each local remote at its new URL**

```bash
cd /Users/nassime/GitHub/NexaNet
git remote set-url origin https://github.com/helyos/helyos.git
( cd nexa-core && git remote set-url origin https://github.com/helyos/helyos-core.git )
( cd nexad     && git remote set-url origin https://github.com/helyos/helyosd.git )
( cd nexa-cli  && git remote set-url origin https://github.com/helyos/helyos-cli.git )
git remote -v; (cd nexa-core && git remote -v)   # verify all point to helyos/*
```

- [ ] **Step 3: Create the canonical, ordered replacement script**

Write `/tmp/helyos-rename.pl` as a pure substitution body (longest/most-specific tokens first; bare `nexa` is `\b`-anchored so it never corrupts words that merely contain the substring). It is always invoked with `perl -i -p` (in-place, per-line) on a **text-only, Cargo.lock-excluded** file list — see Tasks 2-5:

```perl
s/NexaNet/Helyos/g;
s/nexanet/helyos/g;
s/nexa-net/helyos/g;            # org
s/nexa-core/helyos-core/g;
s/nexa-cli/helyos-cli/g;
s/nexa_core/helyos_core/g;
s/NexaError/HelyosError/g;
s/NexaClient/HelyosClient/g;
s/\bnexad\b/helyosd/g;          # crate / bin / lib / service
s/Nexa/Helyos/g;               # remaining PascalCase: Nexa* types & alert names
s/NEXA_/HELYOS_/g;             # env vars
s/nexa_/helyos_/g;            # metric names
s/nexa\.cluster/helyos.cluster/g;   # proto package
s/nexa\.(db|local|pod|project|deployment|svc)/helyos.$1/g;
s/\.nexa\b/.helyos/g;          # ~/.nexa
s{/nexa\b}{/helyos}g;          # /var/lib/nexa
s/"nexa"/"helyos"/g;          # containerd namespace literal
s/\bnexa\b/helyos/g;          # bare CLI command + remaining tokens (incl. nexa-foo)
```

- [ ] **Step 4: Commit (remotes are config-only; nothing to commit yet)** — no commit; proceed.

---

## Task 2: Rename `helyos-core` (was nexa-core) + release v0.1.7

**Files:** `nexa-core/Cargo.toml`, all `nexa-core/src/**`, `nexa-core/.github/workflows/ci.yml`, `nexa-core/README.md`.

- [ ] **Step 1: Branch**

```bash
cd /Users/nassime/GitHub/NexaNet/nexa-core && git checkout -b rename/helyos
```

- [ ] **Step 2: Apply the replacement to all tracked text files**

```bash
git grep -I -l -z -i nexa -- . ':(exclude)Cargo.lock' | xargs -0 perl -i -p /tmp/helyos-rename.pl
```

- [ ] **Step 3: Bump version 0.1.6 → 0.1.7**

```bash
perl -pi -e 's/^version = "0.1.6"/version = "0.1.7"/' Cargo.toml
grep -E '^name|^version' Cargo.toml   # expect name = "helyos-core", version = "0.1.7"
```

- [ ] **Step 4: Rebuild lockfile + verify build/test/clippy/fmt/audit**

```bash
cargo build --offline 2>/dev/null || cargo build      # refresh Cargo.lock (helyos-core entry)
cargo test 2>&1 | tail -3                              # all pass
cargo clippy --all-targets 2>&1 | grep -E 'warning:|error' | head   # empty
cargo fmt --check                                      # clean (or run `cargo fmt`)
cargo audit                                            # exit 0, zero advisories (serde_yaml_ng)
git grep -i nexa || echo "CLEAN: no nexa references"
```
Fix any compile errors (rare; e.g., a doc-test referencing an old name). Re-run until green.

- [ ] **Step 5: Commit, push, PR, wait for green CI, merge, tag**

```bash
git add -A && git commit -m "rename: nexa-core -> helyos-core (v0.1.7)"
git push -u origin rename/helyos
gh pr create --base main --title "Rename nexa-core -> helyos-core" --body "Project rename NexaNet -> Helyos (see spec)."
gh pr checks <N> --watch --interval 20         # green
gh pr merge <N> --squash --delete-branch
git checkout main && git pull --ff-only
gh release create v0.1.7 --target main --title "v0.1.7" --notes "Rename to helyos-core (Helyos)."
```

---

## Task 3: Rename `helyosd` (was nexad) + adopt helyos-core v0.1.7

**Files:** `nexad/Cargo.toml`, `nexad/build.rs`, `nexad/proto/cluster.proto`, all `nexad/src/**`, `nexad/migrations/*`, `nexad/.github/workflows/*`, `nexad/README.md`, any `nexad/deploy` service files.

- [ ] **Step 1: Branch + apply replacement**

```bash
cd /Users/nassime/GitHub/NexaNet/nexad && git checkout -b rename/helyos
git grep -I -l -z -i nexa -- . ':(exclude)Cargo.lock' | xargs -0 perl -i -p /tmp/helyos-rename.pl
```

- [ ] **Step 2: Fix the dependency (name + URL + tag) — the script renamed the key but verify**

`Cargo.toml` dependency line must read:
```toml
helyos-core = { git = "https://github.com/helyos/helyos-core", tag = "v0.1.7" }
```
(The script turned `nexa-core`→`helyos-core` and `nexa-net`→`helyos`; only the tag needs bumping.)
```bash
perl -pi -e 's/tag = "v0.1.6"/tag = "v0.1.7"/' Cargo.toml
grep -nE 'helyos-core|name = |tag =' Cargo.toml | head
```

- [ ] **Step 3: Rename nexa-named files (proto include path is by package, not filename)**

```bash
git ls-files | grep -i nexa            # list files whose NAME contains nexa
# git mv each (e.g. a service/plist file) to its helyos name, if any
```
Note: `proto/cluster.proto` keeps its filename; only its `package` (now `helyos.cluster.v1`) and the matching `include_proto!("helyos.cluster.v1")` in `src/cluster/mod.rs` changed (done by the script — verify both).

- [ ] **Step 4: Fetch helyos-core v0.1.7 + verify everything**

```bash
cargo update -p helyos-core
grep -A2 'name = "helyos-core"' Cargo.lock | grep -E 'version|source'   # v0.1.7, helyos URL
cargo test --lib 2>&1 | tail -3                       # 192 pass
cargo clippy --all-targets 2>&1 | grep -E 'warning:|error' | head       # empty
cargo fmt --check
cargo audit --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2025-0134       # exit 0
git grep -i nexa || echo "CLEAN"
```
Check key renames landed: metric names `helyos_*` (src/adapters/metrics/prometheus.rs), containerd namespace `"helyos"` (containerd.rs), `helyos.db` (main.rs), env `HELYOS_*`, proto `helyos.cluster.v1`.

- [ ] **Step 5: Commit, push, PR, green CI, merge**

```bash
git add -A && git commit -m "rename: nexad -> helyosd; adopt helyos-core v0.1.7"
git push -u origin rename/helyos
gh pr create --base main --title "Rename nexad -> helyosd" --body "Project rename."
gh pr checks <N> --watch --interval 20 ; gh pr merge <N> --squash --delete-branch
git checkout main && git pull --ff-only
```

---

## Task 4: Rename `helyos-cli` (was nexa-cli) + adopt helyos-core v0.1.7

**Files:** `nexa-cli/Cargo.toml`, all `nexa-cli/src/**`, `nexa-cli/.github/workflows/*`, `nexa-cli/README.md`.

- [ ] **Step 1: Branch + apply replacement**

```bash
cd /Users/nassime/GitHub/NexaNet/nexa-cli && git checkout -b rename/helyos
git grep -I -l -z -i nexa -- . ':(exclude)Cargo.lock' | xargs -0 perl -i -p /tmp/helyos-rename.pl
```

- [ ] **Step 2: Verify Cargo.toml package + bin + dep**

```toml
[package] name = "helyos-cli"
[[bin]]   name = "helyos"            # was "nexa"
helyos-core = { git = "https://github.com/helyos/helyos-core", tag = "v0.1.7" }
```
```bash
perl -pi -e 's/tag = "v0.1.6"/tag = "v0.1.7"/' Cargo.toml
grep -nE 'name = "helyos|^name|tag =|bin' Cargo.toml | head
```

- [ ] **Step 3: Fetch + verify (incl. config dir ~/.helyos, env HELYOS_*)**

```bash
cargo update -p helyos-core
cargo test 2>&1 | tail -3            # 44 pass
cargo clippy --all-targets 2>&1 | grep -E 'warning:|error' | head    # empty
cargo fmt --check
cargo audit --ignore RUSTSEC-2025-0119 --ignore RUSTSEC-2024-0436 --ignore RUSTSEC-2026-0002   # exit 0
grep -rn 'helyos/config.toml\|HELYOS_SERVER\|HELYOS_API_TOKEN\|HELYOS_CONFIG\|\.helyos' src/config.rs src/main.rs | head
git grep -i nexa || echo "CLEAN"
```

- [ ] **Step 4: Smoke the renamed binary**

```bash
cargo run -- --help | head -3       # shows `helyos` usage
cargo run -- completions bash | head -2
```

- [ ] **Step 5: Commit, push, PR, green CI, merge**

```bash
git add -A && git commit -m "rename: nexa-cli -> helyos-cli, bin nexa -> helyos; adopt helyos-core v0.1.7"
git push -u origin rename/helyos
gh pr create --base main --title "Rename nexa-cli -> helyos-cli" --body "Project rename."
gh pr checks <N> --watch --interval 20 ; gh pr merge <N> --squash --delete-branch
git checkout main && git pull --ff-only
```

---

## Task 5: Rename the meta repo (`helyos`, was nexa)

**Files:** `README.md`, `CONTRIBUTING.md`, `install.sh`, `LICENSE`, `NOTICE`, `deploy/prometheus/*`, `deploy/grafana/*`, `docs/**` (incl. historical `docs/superpowers/**`).

- [ ] **Step 1: Branch + apply replacement to all tracked files**

```bash
cd /Users/nassime/GitHub/NexaNet && git checkout -b rename/helyos
git grep -I -l -z -i nexa -- . ':(exclude)Cargo.lock' | xargs -0 perl -i -p /tmp/helyos-rename.pl
```

- [ ] **Step 2: Rename nexa-named files**

```bash
git ls-files | grep -i nexa     # expect: deploy/grafana/nexanet-dashboard.json, docs/superpowers/specs/2026-05-21-nexanet-full-design.md
git mv deploy/grafana/nexanet-dashboard.json deploy/grafana/helyos-dashboard.json
git mv docs/superpowers/specs/2026-05-21-nexanet-full-design.md docs/superpowers/specs/2026-05-21-helyos-full-design.md
```

- [ ] **Step 3: Validate configs + grep clean**

```bash
ruby -ryaml -e 'Dir["deploy/prometheus/*.yml"].each{|f| YAML.load_file(f); puts "OK #{f}"}'
ruby -rjson -e 'JSON.parse(File.read("deploy/grafana/helyos-dashboard.json")); puts "OK dashboard"'
grep -n 'helyos_\|Helyos' deploy/prometheus/alerts.yml | head           # metric + alert names renamed
grep -n 'helyos\|helyosd' install.sh | head                             # binaries, paths, services
git grep -i nexa || echo "CLEAN: meta repo has no nexa references"
```

- [ ] **Step 4: Commit, push, PR, merge** (meta repo has no CI gate)

```bash
git add -A && git commit -m "rename: NexaNet -> Helyos (docs, install, deploy, license)"
git push -u origin rename/helyos
gh pr create --base main --title "Rename NexaNet -> Helyos (meta)" --body "Project rename."
gh pr merge <N> --squash --delete-branch
git checkout main && git pull --ff-only
```

---

## Task 6: Final cross-repo verification

- [ ] **Step 1: Zero nexa references anywhere**

```bash
cd /Users/nassime/GitHub/NexaNet
for d in . nexa-core nexad nexa-cli; do
  ( cd $d && git grep -i nexa >/dev/null 2>&1 && echo "$d: STILL HAS nexa" || echo "$d: CLEAN" )
done
```

- [ ] **Step 2: Main-branch CI green on all 3 crates**

```bash
for r in helyos-core helyosd helyos-cli; do
  gh run list --repo helyos/$r --branch main --limit 1 --json status,conclusion -q '.[]|"'$r': \(.status)/\(.conclusion)"'
done   # all completed/success
```

- [ ] **Step 3: Binaries smoke**

```bash
( cd nexa-cli && cargo run -q -- --help | head -3 )    # helyos
( cd nexad && cargo run -q -- --help | head -3 )       # helyosd
```

- [ ] **Step 4: Clean up** — `rm /tmp/helyos-rename.pl`.

---

## Notes / gotchas

- **Order matters in the perl script** — specific compound tokens before bare `\bnexa\b`; that anchoring is what prevents corrupting unrelated substrings.
- **git redirects** keep old URLs resolving during the transition, so a consumer briefly pointing at an old URL still builds — but we update every URL explicitly.
- **proto**: the `include_proto!` argument must equal the `package` in `cluster.proto` (both `helyos.cluster.v1`) or the build fails fast — good early signal.
- If `cargo fmt --check` flags lines the replacement lengthened (helyos is longer than nexa), run `cargo fmt` and re-commit.
- Each crate's existing `cargo audit` ignore list is preserved verbatim (only the crate names around it changed).
</content>
