# Audit Production-Readiness — NexaNet

**Date**: 2026-05-26  
**Scope**: 3 crates (~53K lignes Rust), infra, CI/CD, proto, monitoring  
**Auditeur**: Claude (automated deep audit)

---

## Vue d'ensemble

| Severite | nexa-core | nexad | nexa-cli | Infra | **Total** |
|----------|-----------|-------|----------|-------|-----------|
| CRITICAL | 1 | 4 | 1 | 2 | **8** |
| HIGH | 8 | 10 | 7 | 7 | **32** |
| MEDIUM | 13 | 9 | 10 | 14 | **46** |
| LOW | 8 | 9 | 12 | 11 | **40** |

**Verdict : pas production-ready en l'etat.** Les 8 critiques bloquent tout deploiement. Les 32 HIGH sont a traiter avant une beta publique. Note : nexa-proxy a ete supprime (repo supprime, crate retire, references nettoyees) et remplace par des reverse proxies etablis (Traefik par defaut, avec options Nginx et Caddy).

---

## CRITICAL (8) — Bloqueurs absolus

### Securite — API & Auth

| # | Crate | Issue | Fichier |
|---|-------|-------|---------|
| 1 | nexad | **Aucune authentification sur l'API HTTP** — deploy, secrets, drain, token rotation accessibles sans auth | `src/api/routes.rs` |
| 2 | nexad | **API ecoute sur 0.0.0.0 par defaut** — combine avec zero auth = daemon ouvert au reseau | `src/main.rs:35` |
| 3 | nexad | **Join token stocke en clair** dans SQLite — seul le hash devrait etre persiste | `src/api/handlers.rs:284` |
| 4 | nexa-cli | **Secrets passes en arguments CLI** — visibles via `ps aux`, shell history, audit logs | `src/main.rs:200-201` |

### Cluster — Pods fantomes

| # | Crate | Issue | Fichier |
|---|-------|-------|---------|
| 5 | nexad | **`stop_pod` et `remove_pod` sont des no-ops** dans ClusterServer — retournent `success: true` sans rien faire | `src/cluster/server.rs:251-273` |
| 6 | nexad | **Aucun graceful shutdown** du daemon — containers orphelins, etat incoherent | `src/main.rs` |

### Supply Chain

| # | Crate | Issue | Fichier |
|---|-------|-------|---------|
| 7 | infra | **Install script sans verification d'integrite** — binaires telecharges sans checksum/signature | `install.sh` |
| 8 | infra | **Release artifacts non signes** — aucun SHA-256, cosign, ou GPG dans les workflows | CI workflows |

---

## HIGH (32) — A corriger avant beta

### Securite (6)

- **nexad** — gRPC cluster sans TLS : tokens, pod specs et heartbeats en clair (`src/cluster/server.rs:320`)
- **nexad** — `import_cert` stocke la cle privee en clair malgre le champ `key_pem_enc` (`src/adapters/tls/acme.rs:43`)
- **nexa-cli** — Join tokens affiches en clair sans auth cote API (`src/commands/cluster.rs:19`)
- **nexa-cli** — Zero support d'authentification dans le client HTTP (`src/client.rs`)
- **nexa-cli** — Cle privee TLS envoyee en HTTP potentiellement non chiffre (`src/commands/route.rs:72-90`)
- **infra** — Aucun `cargo audit` / `cargo deny` dans la CI

### Correctness (9)

- **nexad** — `container_exists` utilise pour checker les networks Docker — check toujours false (`src/adapters/transport/local.rs:58`)
- **nexad** — Containerd ne redirige pas stdout/stderr vers les log files (`src/adapters/runtime/containerd.rs:203`)
- **nexad** — CNI `attach`/`detach` sont des stubs non implementes (`src/adapters/runtime/cni.rs:150`)
- **nexad** — WireGuard overlay est un no-op (`src/adapters/network/wireguard.rs:96`)
- **nexad** — Routes et certificats stockes en memoire seulement — perdus au restart (`src/adapters/state/memory_route_store.rs`)
- **nexad** — Reschedule callback est un TODO — pods perdus quand un worker meurt (`src/main.rs:429`)
- **nexad** — `stream_logs` retourne un stream vide dans ClusterServer (`src/cluster/server.rs:296`)
- **nexa-core** — `handle_create_project` ne persiste pas en state store (`src/domain/orchestrator.rs:702`)
- **nexa-core** — `TlsMode::Auto` avec email vide — ACME va echouer (`src/domain/orchestrator.rs:1717`)

### Robustesse (4)

- **nexa-core** — Mutex `.unwrap()` partout (25+ sites) — crash en cas de poison (`src/ports/state_memory.rs`)
- **nexad** — Idem, RwLock `.unwrap()` dans DNS, route store, CNI, proxy adapter
- **nexa-cli** — Aucun timeout HTTP client — commandes bloquees indefiniment (`src/client.rs:43`)
- **nexa-cli** — TUI panic laisse le terminal en raw mode (`src/tui/mod.rs:20`)

### Testing (4)

- **nexa-cli** — Zero tests d'integration, zero tests sur les 16 commandes
- **nexad** — Zero tests pour la communication gRPC cluster
- **infra** — Release workflow publie meme si le build echoue (`if: always()`)
- **infra** — `nexa-core` et `nexad` pointent vers des commits git differents de nexa-core

### Deps & Infra (5)

- **nexa-core** — `serde_yaml 0.9` est deprecated
- **infra** — Pas de Cargo workspace — pas de resolution unifiee des deps
- **infra** — Aucun Dockerfile dans tout le projet
- **infra** — Aucun manifest de deploiement (Helm, docker-compose, systemd hardened)
- **infra** — nexa-core reference par git sans `rev`/`tag` — builds non reproductibles

---

## MEDIUM (46) — A traiter pour la qualite production

### nexa-core (13)

- In-memory adapters dans `ports/` au lieu de `adapters/` (leaky architecture) (`src/ports/state_memory.rs`)
- `Orchestrator::spawn` a 9 parametres `Option` positionnels — builder pattern necessaire (`src/domain/orchestrator.rs:412`)
- Regex recompilee a chaque appel de validation (`src/config.rs:30,76`)
- Error variants stringly-typed (`NexaError::Runtime(String)`) (`src/error.rs:17`)
- Persistence failures logguees en warning, pas d'alerte metrique (`src/domain/orchestrator.rs:1764`)
- Scale-down trie par UUID (random) au lieu de creation order (`src/domain/orchestrator.rs:903`)
- Pas de property-based testing ni fuzz testing
- `Command` enum public expose le protocole interne de l'actor (`src/domain/orchestrator.rs:26`)
- `SchedulerConfig.strategy` est un String, pas un enum (`src/domain/scheduler.rs:157`)
- O(n) lookup par nom de deployment (`src/domain/orchestrator.rs:1525`)
- O(n) filtrage des pods par deployment_id (multiple sites)
- `tokio features = ["full"]` dans un library crate (`Cargo.toml:30`)
- Pas de validation de configuration au demarrage

### nexad (9)

- Token verification avec comparaison non constant-time (`src/cluster/token.rs:19`)
- Silent error swallowing dans heartbeat monitor (`src/cluster/heartbeat.rs:77,97`)
- Metric registration `.unwrap()` au startup (`src/adapters/metrics/prometheus.rs:37+`)
- Migration rollback risquee avec `PRAGMA foreign_keys=OFF` (`migrations/...cascade_delete.sql`)
- DNS server sans rate limiting (`src/adapters/dns/hickory.rs:46`)
- DNS upstream cree un socket par query — epuise les FD (`src/adapters/dns/hickory.rs:273`)
- Pas de tests containerd runtime
- Health checker interval hardcode a 1s (`src/adapters/health/mod.rs:27`)
- `node_stats` handler bloque 200ms par appel (`src/api/handlers.rs:615`)

### nexa-cli (10)

- `print_json` utilise `unwrap()` sur la serialisation (`src/output/mod.rs:35`)
- Detection d'erreur connexion par string matching fragile (`src/main.rs:420`)
- SSE event stream cree son propre client hors `NexaClient` (`src/tui/event.rs:39`)
- URL path parameters non percent-encoded (multiple fichiers)
- Query parameters non encodes (`src/commands/pods.rs:10`)
- Pas de support `--no-color` / `NO_COLOR` explicite
- Pas de fichier de configuration (`~/.nexa/config`)
- `nexa logs` sans interruption gracieuse (`src/commands/logs.rs:28`)
- Loose version pinning sur toutes les deps (`Cargo.toml`)
- Pas de tests pour `client.rs` error handling

### Infra (14)

- Scrape config Prometheus utilise seulement `static_configs` (pas de service discovery)
- Alertes manquantes : disk space, cert expiry, split-brain, crash loops, process down
- Proto sans versioning (`nexa.cluster` au lieu de `nexa.cluster.v1`)
- String-typed enumerations dans le proto (status, action)
- `bytes` fields pour des donnees structurees dans le proto
- Version misalignment entre crates (0.1.0 vs 0.2.0)
- Shared dependency version drift sans workspace
- Service units user-level seulement (pas de hardening systemd)
- Pas de mecanisme d'uninstall
- nexa-core CI ne lance que `cargo test --lib`
- Pas de CI multi-plateforme (macOS non teste)
- Pas de documentation architecture
- Pas de CONTRIBUTING.md
- README reference des features non implementees (WireGuard, CNI)

---

## LOW (40) — Polish et dette technique

### nexa-core (8)

- Glob re-export dans models (`pub use deployment::*`) — risque de collision
- Clippy suppression blanket (`too_many_arguments`)
- `HealthTracker` non Send+Sync
- Cloning excessif dans le path de persistence
- Channel buffer size hardcode a 256
- Tests avec timing sensible (`sleep(100ms)`)
- CI `cargo test --lib` rate les integration tests
- Pas de schema versioning dans l'etat persiste

### nexad (9)

- Master key genere avec `thread_rng()` au lieu de `OsRng` (`src/crypto/master_key.rs:41`)
- Dead schema : table `secrets` dans les migrations jamais utilisee
- Subnet allocator wrap correct mais message misleading
- Event watcher reconnection sans backoff (`src/adapters/event_watcher.rs:29`)
- Heartbeat sender reconnection sans backoff (`src/cluster/worker.rs:86`)
- `expect()` dans HealthChecker constructor (`src/adapters/health/mod.rs:19`)
- Dual SQLite (sqlx + rusqlite) augmente la surface d'attaque
- `rand` 0.8 outdated (0.9 disponible)
- Broadcast channel capacity hardcode a 256 (`src/main.rs:301`)

### nexa-cli (12)

- Pas de validation du `--server` URL
- Pas de shell completion (`clap_complete`)
- `nexa rm` / `project delete` sans confirmation `--yes`
- `event::poll()` unwrap dans le thread TUI
- `spinner expect()` sur template
- Status message TUI jamais cleared
- TUI log view est un snapshot statique, pas un live tail
- `status` commande hardcode "single-node"
- `nexa setup cni` hardcode `linux` dans l'URL de telechargement
- `nexa deploy` timeout hardcode a 60s
- `tracing` et `uuid` declares mais jamais utilises
- `nexa-core` git dependency sans rev/tag

### Infra (10)

- `set -e` sans `-u` dans install.sh
- Pas de code coverage en CI
- Benchmarks sur chaque push (bruyant)
- Custom `Empty` message dans le proto au lieu de `google.protobuf.Empty`
- Pas de proto linting (`buf`)
- Pas de `cargo doc` en CI
- Example YAML avec credentials hardcodes (`nexad/examples/app.yaml:19`)
- Pas de fichier NOTICE (Apache-2.0 section 4d)
- Pas de SBOM generation
- OOM alert sans `for` duration

---

## Plan d'action prioritise

### Phase 1 — Securite (semaine 1-2)

1. Ajouter un middleware d'authentification Bearer token sur l'API nexad
2. Hasher le join token avant stockage, ne montrer le clair qu'une seule fois
3. Lire les secrets depuis stdin au lieu d'arguments CLI : `echo $SECRET | nexa secret set KEY -p app`
4. Activer TLS sur le gRPC cluster (tonic supporte TLS nativement)
5. Mettre `/metrics` sur un listener interne separe
6. Ajouter `cargo audit` a tous les CI workflows

### Phase 2 — Correctness (semaine 2-3)

7. Implementer `stop_pod`/`remove_pod` dans ClusterServer (forward vers le runtime local)
8. Fixer le check reseau Docker (utiliser l'API network, pas `container_exists`)
9. Persister les routes/certs en SQLite au lieu de l'in-memory store
10. Implementer le graceful shutdown (`tokio::signal` + `CancellationToken`) dans nexad
11. Fixer `handle_create_project` pour persister en state store

### Phase 3 — Robustesse (semaine 3-4)

12. Remplacer tous les `.lock().unwrap()` par `parking_lot::Mutex` (pas de poison)
13. Ajouter timeouts HTTP : connect 5s, request 30s (CLI)
14. Fixer TUI panic recovery avec `std::panic::set_hook`
15. Ajouter retry avec backoff exponentiel dans le CLI

### Phase 4 — Infra & Testing (semaine 4-6)

16. Creer un Cargo workspace unifie
17. Ajouter Dockerfiles multi-stage pour nexad
18. Ajouter checksums SHA-256 aux releases + verification dans install.sh
19. Ecrire des tests pour les commandes CLI et le gRPC cluster
20. Implementer le pod rescheduling quand un worker meurt

### Phase 5 — Polish (semaine 6-8)

21. Proto versioning (`nexa.cluster.v1`), remplacer les string-typed enums par des proto enums
22. Builder pattern pour `Orchestrator::spawn`
23. Lazy regex compilation, index secondaires pour les lookups O(n)
24. Documentation architecture, runbooks, CONTRIBUTING.md
25. Rate limiting (API, DNS)

---

## Annexes — Rapports detailles par crate

### A. nexa-core

**Crate**: `nexa-core` v0.1.0  
**Lignes**: ~3,300 Rust | **Tests**: ~130

#### Architecture

L'architecture hexagonale est correctement implementee : `domain/models/` definit les types purs, `ports/` definit les interfaces `async_trait`, et `domain/orchestrator.rs` assemble le tout via injection de dependances (`Arc<dyn Trait>`).

Points faibles :
- In-memory adapters dans `ports/` au lieu d'un module `adapters/`
- `Orchestrator::spawn` a 9 parametres positionnels `Option<Arc<dyn Trait>>`
- `Command` enum public expose le protocole interne de l'actor mailbox

#### Error Handling

- Regex `unwrap()` sur des patterns constants — safe mais recompile a chaque appel
- 25+ sites de `Mutex.lock().unwrap()` dans les stores in-memory
- `persist_*` methods logguent les erreurs mais ne les remontent pas
- `NexaError` variants transportent seulement des `String`

#### Performance

- Regex recompilee a chaque validation — utiliser `LazyLock<Regex>`
- O(n) lookup deployment par nom — ajouter un index `HashMap<(String, String), Uuid>`
- O(n) filtrage pods par deployment_id — ajouter un index secondaire
- Cloning excessif dans le path de persistence

#### Production

- Pas de graceful shutdown (l'actor loop tourne jusqu'a fermeture du channel)
- `handle_create_project` ne persiste pas
- `TlsMode::Auto` avec email vide
- Pas de validation de config au demarrage
- Pas de rate limiting sur le command channel

---

### B. nexad

**Crate**: `nexad` v0.2.0  
**Architecture**: Adapters pour runtime (Docker/containerd), DNS (hickory/noop), proxy (nginx/caddy/traefik), state (SQLite), secrets (encrypted SQLite), transport (gRPC/local), networking (WireGuard/CNI), health, metrics (Prometheus)

#### Securite

- **Aucune auth** sur l'API HTTP — tous les endpoints management accessibles sans credentials
- **API sur 0.0.0.0** par defaut
- **Join token en clair** dans SQLite
- **gRPC sans TLS** — communication cluster en plaintext
- **import_cert** stocke la cle privee non chiffree malgre le champ `key_pem_enc`
- Token verification non constant-time

#### Container Runtime

- `stop_pod`/`remove_pod` dans ClusterServer sont des **no-ops** qui retournent success
- `stream_logs` retourne un **stream vide**
- Containerd ne redirige pas stdout/stderr vers les fichiers de log
- CNI `attach`/`detach` sont des stubs `bail!("not implemented")`
- `container_exists` utilise pour checker les networks Docker (toujours false)

#### Networking

- WireGuard `create_tunnel()` est un **no-op** — log seulement
- DNS upstream cree un socket par query
- DNS server sans rate limiting

#### State

- Routes et certificats en memoire seulement — perdus au restart
- Tables SQLite existent dans les migrations mais l'in-memory store est utilise
- Migration rollback risquee avec `PRAGMA foreign_keys=OFF`

#### Production

- **Aucun graceful shutdown** — pas de handler SIGTERM/SIGINT
- Reschedule callback est un **TODO** — pods perdus quand un worker meurt
- Health checker interval hardcode a 1s
- `node_stats` bloque 200ms par appel

---

### C. nexa-cli

**Crate**: `nexa-cli` v0.2.0  
**Lignes**: ~3,600 Rust | **Tests**: 24 unit tests

#### Points forts

- Clean clap v4 derive-based commands avec `propagate_version`
- Excellent systeme de sortie panel/table/spinner avec palette GitHub-Dark
- Support `--json` consistant sur toutes les commandes
- `NEXA_ICONS=nerd` pour les utilisateurs Nerd Font
- Error hints actionables (`error_hint()` pour 401/403/404/500/502/503)

#### Securite

- **Secrets en arguments CLI** — visibles dans `ps aux` et shell history
- **Aucun support d'authentification** dans le client HTTP
- Cle privee TLS et secrets envoyes potentiellement en HTTP non chiffre

#### Client

- **Aucun timeout** HTTP — commandes bloquees indefiniment
- **Aucun retry** pour les erreurs transitoires
- URL path parameters non percent-encoded
- SSE event stream cree son propre client hors `NexaClient`

#### UX

- Pas de fichier de configuration (`~/.nexa/config`)
- Pas de shell completion
- TUI panic laisse le terminal casse
- Pas de signal handling pendant `deploy` polling
- `nexa logs` sans interruption gracieuse

#### Testing

- Zero tests d'integration
- 16 commandes avec zero couverture de test
- Client error handling non teste

---

### D. Infrastructure & Cross-Cutting

#### Install Script (`install.sh`)

- Telecharge et installe des binaires **sans verification de checksum**
- `pkill -x nexad` peut tuer le process d'un autre utilisateur
- Pas de `set -u` — variables non definies silencieusement vides

#### CI/CD

- **Aucun security scanning** (`cargo audit`, `cargo deny`) dans aucun pipeline
- Release workflow publie meme si le build echoue (`if: always()`)
- Aucune signature d'artifacts
- nexa-core CI ne lance que `cargo test --lib`
- Pas de CI multi-plateforme (macOS non teste)
- `cross` installe depuis git sans version pin

#### Proto (`cluster.proto`)

- Pas de versioning du package (`nexa.cluster` au lieu de `nexa.cluster.v1`)
- String-typed enumerations (status, action) au lieu de proto enums
- `bytes` fields pour des donnees structurees (pod_spec, pod_data)
- Custom `Empty` message au lieu de `google.protobuf.Empty`

#### Cross-Crate

- **Pas de Cargo workspace** — 3 crates independants sans resolution unifiee
- `nexa-core` pointe vers des **commits git differents** dans nexad vs nexa-cli
- `nexa-core` reference par git **sans rev/tag** — builds non reproductibles
- Version misalignment (0.1.0 vs 0.2.0 sans semver stable)

#### Deploiement

- **Aucun Dockerfile** dans tout le projet
- Aucun manifest de deploiement (Helm, docker-compose, Terraform)
- Service units user-level seulement (pas de hardening systemd)
- Pas de mecanisme d'uninstall

#### Documentation

- Pas de documentation architecture
- Pas de CONTRIBUTING.md
- README reference des features non implementees
- Pas de rustdoc en CI
- Pas de fichier NOTICE (Apache-2.0 section 4d)

---

## Methodologie

Cet audit a ete realise par lecture exhaustive de chaque fichier source, configuration, CI workflow, proto, et test du projet. Chaque finding a ete verifie par reference directe au code (fichier et numero de ligne). Les severites suivent cette echelle :

- **CRITICAL** : Bloqueur de production — vulnerabilite de securite exploitable, perte de donnees, ou fonctionnalite cassee
- **HIGH** : Probleme serieux — affecte la fiabilite, securite, ou operabilite en production
- **MEDIUM** : Probleme de qualite — affecte la maintenabilite, performance, ou UX
- **LOW** : Dette technique — amelioration souhaitable mais non bloquante
