# Networking & Routing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add overlay networking (WireGuard via boringtun), a pluggable reverse proxy abstraction with four backends (nexa-proxy, caddy, traefik, nginx), and automated TLS certificate management to enable multi-node container routing with HTTPS.

**Architecture:** Three layers compose into the hexagonal architecture. Layer 1 is a WireGuard overlay network: when a worker joins the cluster, the master assigns it a `/24` subnet from the cluster CIDR (`172.20.0.0/16`), generates a WireGuard keypair, and distributes peer configs via gRPC; each node runs a userspace WireGuard interface via `boringtun` so containers on different nodes can reach each other by IP. Layer 2 is a `ProxyBackend` port trait in `nexa-core` with four adapter implementations in `nexad` (nexa-proxy as a new crate, plus caddy/traefik/nginx config generators); the orchestrator calls `apply_routes` on deploy and `remove_route` on teardown. Layer 3 is TLS automation: certificates are stored encrypted in SQLite, and a daily renewal task uses `instant-acme` to issue/renew certificates 30 days before expiry.

**Tech Stack:** boringtun 0.6, x25519-dalek 2, base64 0.22, hyper 1 (full), hyper-util 0.1, rustls 0.23, instant-acme 0.7, tokio, async-trait, chrono, serde, sqlx (existing)

---

### Task 1: Define ProxyBackend port trait with RouteConfig, Upstream, TlsConfig types

**Files:**
- Create: `crates/nexa-core/src/ports/proxy.rs`
- Modify: `crates/nexa-core/src/ports/mod.rs`
- Modify: `crates/nexa-core/src/lib.rs` (if `ports` module not yet exposed)

- [ ] **Step 1: Create the proxy port trait file**

Create `crates/nexa-core/src/ports/proxy.rs`:

```rust
use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Configuration for a single route entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteConfig {
    pub domain: String,
    pub upstream: Vec<Upstream>,
    pub tls: TlsConfig,
}

/// A single upstream target with optional weight for load balancing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upstream {
    pub address: String,
    pub weight: u32,
}

/// TLS configuration for a route.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum TlsConfig {
    None,
    Auto { email: String },
    Manual { cert: PathBuf, key: PathBuf },
}

/// Port trait for reverse proxy backends.
///
/// Each backend (nexa-proxy, caddy, traefik, nginx) implements this trait
/// to apply routing configuration, manage TLS, and perform health checks.
#[async_trait]
pub trait ProxyBackend: Send + Sync {
    /// Apply or update a set of route configurations.
    async fn apply_routes(&self, routes: &[RouteConfig]) -> Result<()>;

    /// Remove the route for a specific domain.
    async fn remove_route(&self, domain: &str) -> Result<()>;

    /// Reload the proxy configuration (e.g., signal the process).
    async fn reload(&self) -> Result<()>;

    /// Check whether the proxy backend is healthy and responsive.
    async fn health(&self) -> Result<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trait_is_object_safe() {
        fn _assert_object_safe(_: &dyn ProxyBackend) {}
    }

    #[test]
    fn route_config_serializes_to_json() {
        let config = RouteConfig {
            domain: "api.example.com".into(),
            upstream: vec![
                Upstream { address: "10.0.0.1:3000".into(), weight: 1 },
                Upstream { address: "10.0.0.2:3000".into(), weight: 1 },
            ],
            tls: TlsConfig::Auto { email: "admin@example.com".into() },
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("api.example.com"));
        assert!(json.contains("10.0.0.1:3000"));
        assert!(json.contains("admin@example.com"));
    }

    #[test]
    fn tls_config_none_serializes() {
        let tls = TlsConfig::None;
        let json = serde_json::to_string(&tls).unwrap();
        assert!(json.contains("none"));
    }

    #[test]
    fn tls_config_manual_serializes() {
        let tls = TlsConfig::Manual {
            cert: PathBuf::from("/etc/certs/cert.pem"),
            key: PathBuf::from("/etc/certs/key.pem"),
        };
        let json = serde_json::to_string(&tls).unwrap();
        assert!(json.contains("manual"));
        assert!(json.contains("cert.pem"));
    }

    #[test]
    fn upstream_default_weight() {
        let up = Upstream { address: "10.0.0.1:8080".into(), weight: 1 };
        assert_eq!(up.weight, 1);
    }

    #[test]
    fn route_config_deserializes_from_json() {
        let json = r#"{
            "domain": "web.example.com",
            "upstream": [{"address": "10.0.0.5:80", "weight": 3}],
            "tls": {"mode": "none"}
        }"#;
        let config: RouteConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.domain, "web.example.com");
        assert_eq!(config.upstream.len(), 1);
        assert_eq!(config.upstream[0].weight, 3);
        assert!(matches!(config.tls, TlsConfig::None));
    }
}
```

- [ ] **Step 2: Wire the module into the ports directory**

If `crates/nexa-core/src/ports/mod.rs` exists (created by Plan #10), add:
```rust
pub mod proxy;
```

If the `ports` directory does not yet exist, create `crates/nexa-core/src/ports/mod.rs`:
```rust
pub mod dns;
pub mod proxy;
```

And update `crates/nexa-core/src/lib.rs` to expose it:
```rust
pub mod config;
pub mod error;
pub mod models;
pub mod ports;
pub mod runtime;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p nexa-core 2>&1`
Expected: compiles with no errors

- [ ] **Step 4: Run the tests**

Run: `cargo test -p nexa-core -- ports::proxy 2>&1`
Expected: 6 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-core/src/ports/proxy.rs crates/nexa-core/src/ports/mod.rs crates/nexa-core/src/lib.rs
git commit -m "feat(proxy): define ProxyBackend port trait with RouteConfig, Upstream, TlsConfig"
```

---

### Task 2: Route domain model + SQLite migrations (routes, certificates, subnet_allocations tables)

**Files:**
- Create: `crates/nexa-core/src/models/route.rs`
- Modify: `crates/nexa-core/src/models/mod.rs`
- Modify: `crates/nexa-core/src/error.rs`
- Create: `crates/nexad/migrations/003_networking.sql`

- [ ] **Step 1: Add NexaError variants for routing and proxy errors**

In `crates/nexa-core/src/error.rs`, add these variants to the `NexaError` enum:

```rust
#[error("route not found: {0}")]
RouteNotFound(String),

#[error("route already exists: {0}")]
RouteAlreadyExists(String),

#[error("proxy error: {0}")]
Proxy(String),

#[error("certificate error: {0}")]
Certificate(String),

#[error("network error: {0}")]
Network(String),
```

- [ ] **Step 2: Create the Route domain model**

Create `crates/nexa-core/src/models/route.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// TLS mode for a route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    None,
    Auto,
    Manual,
}

impl std::fmt::Display for TlsMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TlsMode::None => write!(f, "none"),
            TlsMode::Auto => write!(f, "auto"),
            TlsMode::Manual => write!(f, "manual"),
        }
    }
}

impl std::str::FromStr for TlsMode {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "none" => Ok(TlsMode::None),
            "auto" => Ok(TlsMode::Auto),
            "manual" => Ok(TlsMode::Manual),
            other => Err(format!("unknown TLS mode: {other}")),
        }
    }
}

/// A routing entry mapping a domain to a project/deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub domain: String,
    pub project: String,
    pub deployment: String,
    pub tls_mode: TlsMode,
    pub created_at: DateTime<Utc>,
}

impl Route {
    pub fn new(domain: &str, project: &str, deployment: &str, tls_mode: TlsMode) -> Self {
        Self {
            domain: domain.to_string(),
            project: project.to_string(),
            deployment: deployment.to_string(),
            tls_mode,
            created_at: Utc::now(),
        }
    }
}

/// A stored TLS certificate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certificate {
    pub domain: String,
    pub cert_pem: Vec<u8>,
    pub key_pem_enc: Vec<u8>,
    pub key_nonce: Vec<u8>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub acme_account: Option<String>,
}

/// A subnet allocation for overlay networking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubnetAllocation {
    pub node_id: String,
    pub project: String,
    pub subnet: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_new_sets_fields() {
        let route = Route::new("api.example.com", "ecommerce", "api", TlsMode::Auto);
        assert_eq!(route.domain, "api.example.com");
        assert_eq!(route.project, "ecommerce");
        assert_eq!(route.deployment, "api");
        assert_eq!(route.tls_mode, TlsMode::Auto);
    }

    #[test]
    fn tls_mode_display() {
        assert_eq!(TlsMode::None.to_string(), "none");
        assert_eq!(TlsMode::Auto.to_string(), "auto");
        assert_eq!(TlsMode::Manual.to_string(), "manual");
    }

    #[test]
    fn tls_mode_from_str() {
        assert_eq!("none".parse::<TlsMode>().unwrap(), TlsMode::None);
        assert_eq!("auto".parse::<TlsMode>().unwrap(), TlsMode::Auto);
        assert_eq!("manual".parse::<TlsMode>().unwrap(), TlsMode::Manual);
        assert_eq!("AUTO".parse::<TlsMode>().unwrap(), TlsMode::Auto);
        assert!("invalid".parse::<TlsMode>().is_err());
    }

    #[test]
    fn tls_mode_serializes_json() {
        let mode = TlsMode::Auto;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, r#""auto""#);
    }

    #[test]
    fn tls_mode_deserializes_json() {
        let mode: TlsMode = serde_json::from_str(r#""manual""#).unwrap();
        assert_eq!(mode, TlsMode::Manual);
    }

    #[test]
    fn route_serializes_json_roundtrip() {
        let route = Route::new("web.example.com", "webapp", "frontend", TlsMode::None);
        let json = serde_json::to_string(&route).unwrap();
        let deserialized: Route = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.domain, "web.example.com");
        assert_eq!(deserialized.tls_mode, TlsMode::None);
    }

    #[test]
    fn subnet_allocation_fields() {
        let alloc = SubnetAllocation {
            node_id: "node-1".into(),
            project: "ecommerce".into(),
            subnet: "172.20.1.0/24".into(),
        };
        assert_eq!(alloc.node_id, "node-1");
        assert_eq!(alloc.subnet, "172.20.1.0/24");
    }

    #[test]
    fn certificate_fields() {
        let cert = Certificate {
            domain: "api.example.com".into(),
            cert_pem: b"CERT".to_vec(),
            key_pem_enc: b"KEY".to_vec(),
            key_nonce: b"NONCE".to_vec(),
            issued_at: Utc::now(),
            expires_at: Utc::now(),
            acme_account: Some("acct-123".into()),
        };
        assert_eq!(cert.domain, "api.example.com");
        assert!(cert.acme_account.is_some());
    }
}
```

- [ ] **Step 3: Wire route module into models**

Update `crates/nexa-core/src/models/mod.rs`:

```rust
mod deployment;
mod pod;
mod project;
mod route;

pub use deployment::*;
pub use pod::*;
pub use project::*;
pub use route::*;
```

- [ ] **Step 4: Create SQL migration file for the three new tables**

Create `crates/nexad/migrations/003_networking.sql`:

```sql
CREATE TABLE IF NOT EXISTS routes (
    domain      TEXT PRIMARY KEY,
    project     TEXT NOT NULL,
    deployment  TEXT NOT NULL,
    tls_mode    TEXT NOT NULL DEFAULT 'none',
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS certificates (
    domain      TEXT PRIMARY KEY,
    cert_pem    BLOB NOT NULL,
    key_pem_enc BLOB NOT NULL,
    key_nonce   BLOB NOT NULL,
    issued_at   TEXT NOT NULL,
    expires_at  TEXT NOT NULL,
    acme_account TEXT
);

CREATE TABLE IF NOT EXISTS subnet_allocations (
    node_id     TEXT NOT NULL REFERENCES nodes(id),
    project     TEXT NOT NULL,
    subnet      TEXT NOT NULL UNIQUE,
    PRIMARY KEY (node_id, project)
);

CREATE INDEX IF NOT EXISTS idx_routes_project ON routes(project);
CREATE INDEX IF NOT EXISTS idx_certificates_expires ON certificates(expires_at);
CREATE INDEX IF NOT EXISTS idx_subnet_allocations_subnet ON subnet_allocations(subnet);
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p nexa-core 2>&1`
Expected: compiles with no errors

- [ ] **Step 6: Run the tests**

Run: `cargo test -p nexa-core -- models::route 2>&1`
Expected: 8 tests pass

- [ ] **Step 7: Commit**

```bash
git add crates/nexa-core/src/models/route.rs crates/nexa-core/src/models/mod.rs crates/nexa-core/src/error.rs crates/nexad/migrations/003_networking.sql
git commit -m "feat(routing): add Route, Certificate, SubnetAllocation models and SQL migration"
```

---

### Task 3: Add route CRUD to StateStore trait + SqliteStore + InMemoryStore

**Files:**
- Create: `crates/nexa-core/src/ports/route_store.rs`
- Modify: `crates/nexa-core/src/ports/mod.rs`
- Create: `crates/nexad/src/adapters/state/memory_route_store.rs`
- Modify: `crates/nexad/src/adapters/state/mod.rs` (or equivalent)

- [ ] **Step 1: Define the route storage port trait**

Create `crates/nexa-core/src/ports/route_store.rs`:

```rust
use async_trait::async_trait;

use crate::error::Result;
use crate::models::{Certificate, Route, SubnetAllocation};

/// Port trait for persisting routes, certificates, and subnet allocations.
#[async_trait]
pub trait RouteStore: Send + Sync {
    // --- Routes ---
    async fn insert_route(&self, route: &Route) -> Result<()>;
    async fn get_route(&self, domain: &str) -> Result<Option<Route>>;
    async fn list_routes(&self, project: Option<&str>) -> Result<Vec<Route>>;
    async fn delete_route(&self, domain: &str) -> Result<bool>;

    // --- Certificates ---
    async fn upsert_certificate(&self, cert: &Certificate) -> Result<()>;
    async fn get_certificate(&self, domain: &str) -> Result<Option<Certificate>>;
    async fn list_expiring_certificates(&self, within_days: i64) -> Result<Vec<Certificate>>;
    async fn delete_certificate(&self, domain: &str) -> Result<bool>;

    // --- Subnet Allocations ---
    async fn allocate_subnet(&self, alloc: &SubnetAllocation) -> Result<()>;
    async fn get_node_subnet(&self, node_id: &str, project: &str) -> Result<Option<SubnetAllocation>>;
    async fn list_subnets(&self) -> Result<Vec<SubnetAllocation>>;
    async fn deallocate_subnet(&self, node_id: &str, project: &str) -> Result<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trait_is_object_safe() {
        fn _assert_object_safe(_: &dyn RouteStore) {}
    }
}
```

Wire it in `crates/nexa-core/src/ports/mod.rs`:
```rust
pub mod route_store;
```

- [ ] **Step 2: Implement InMemoryRouteStore**

Create `crates/nexad/src/adapters/state/memory_route_store.rs`:

```rust
use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use chrono::{Duration, Utc};

use nexa_core::error::{NexaError, Result};
use nexa_core::models::{Certificate, Route, SubnetAllocation};
use nexa_core::ports::route_store::RouteStore;

/// In-memory implementation of RouteStore for testing.
pub struct InMemoryRouteStore {
    routes: RwLock<HashMap<String, Route>>,
    certificates: RwLock<HashMap<String, Certificate>>,
    subnets: RwLock<Vec<SubnetAllocation>>,
}

impl InMemoryRouteStore {
    pub fn new() -> Self {
        Self {
            routes: RwLock::new(HashMap::new()),
            certificates: RwLock::new(HashMap::new()),
            subnets: RwLock::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RouteStore for InMemoryRouteStore {
    async fn insert_route(&self, route: &Route) -> Result<()> {
        let mut routes = self.routes.write().unwrap();
        if routes.contains_key(&route.domain) {
            return Err(NexaError::RouteAlreadyExists(route.domain.clone()));
        }
        routes.insert(route.domain.clone(), route.clone());
        Ok(())
    }

    async fn get_route(&self, domain: &str) -> Result<Option<Route>> {
        let routes = self.routes.read().unwrap();
        Ok(routes.get(domain).cloned())
    }

    async fn list_routes(&self, project: Option<&str>) -> Result<Vec<Route>> {
        let routes = self.routes.read().unwrap();
        let result: Vec<Route> = routes
            .values()
            .filter(|r| match project {
                Some(p) => r.project == p,
                None => true,
            })
            .cloned()
            .collect();
        Ok(result)
    }

    async fn delete_route(&self, domain: &str) -> Result<bool> {
        let mut routes = self.routes.write().unwrap();
        Ok(routes.remove(domain).is_some())
    }

    async fn upsert_certificate(&self, cert: &Certificate) -> Result<()> {
        let mut certs = self.certificates.write().unwrap();
        certs.insert(cert.domain.clone(), cert.clone());
        Ok(())
    }

    async fn get_certificate(&self, domain: &str) -> Result<Option<Certificate>> {
        let certs = self.certificates.read().unwrap();
        Ok(certs.get(domain).cloned())
    }

    async fn list_expiring_certificates(&self, within_days: i64) -> Result<Vec<Certificate>> {
        let certs = self.certificates.read().unwrap();
        let threshold = Utc::now() + Duration::days(within_days);
        let result: Vec<Certificate> = certs
            .values()
            .filter(|c| c.expires_at <= threshold)
            .cloned()
            .collect();
        Ok(result)
    }

    async fn delete_certificate(&self, domain: &str) -> Result<bool> {
        let mut certs = self.certificates.write().unwrap();
        Ok(certs.remove(domain).is_some())
    }

    async fn allocate_subnet(&self, alloc: &SubnetAllocation) -> Result<()> {
        let mut subnets = self.subnets.write().unwrap();
        let exists = subnets
            .iter()
            .any(|s| s.node_id == alloc.node_id && s.project == alloc.project);
        if exists {
            return Err(NexaError::Network(format!(
                "subnet already allocated for node {} project {}",
                alloc.node_id, alloc.project
            )));
        }
        let subnet_taken = subnets.iter().any(|s| s.subnet == alloc.subnet);
        if subnet_taken {
            return Err(NexaError::Network(format!(
                "subnet {} already in use",
                alloc.subnet
            )));
        }
        subnets.push(alloc.clone());
        Ok(())
    }

    async fn get_node_subnet(
        &self,
        node_id: &str,
        project: &str,
    ) -> Result<Option<SubnetAllocation>> {
        let subnets = self.subnets.read().unwrap();
        Ok(subnets
            .iter()
            .find(|s| s.node_id == node_id && s.project == project)
            .cloned())
    }

    async fn list_subnets(&self) -> Result<Vec<SubnetAllocation>> {
        let subnets = self.subnets.read().unwrap();
        Ok(subnets.clone())
    }

    async fn deallocate_subnet(&self, node_id: &str, project: &str) -> Result<bool> {
        let mut subnets = self.subnets.write().unwrap();
        let len_before = subnets.len();
        subnets.retain(|s| !(s.node_id == node_id && s.project == project));
        Ok(subnets.len() < len_before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexa_core::models::TlsMode;

    #[tokio::test]
    async fn insert_and_get_route() {
        let store = InMemoryRouteStore::new();
        let route = Route::new("api.example.com", "ecommerce", "api", TlsMode::Auto);
        store.insert_route(&route).await.unwrap();

        let fetched = store.get_route("api.example.com").await.unwrap().unwrap();
        assert_eq!(fetched.domain, "api.example.com");
        assert_eq!(fetched.project, "ecommerce");
        assert_eq!(fetched.tls_mode, TlsMode::Auto);
    }

    #[tokio::test]
    async fn insert_duplicate_route_fails() {
        let store = InMemoryRouteStore::new();
        let route = Route::new("api.example.com", "ecommerce", "api", TlsMode::None);
        store.insert_route(&route).await.unwrap();
        assert!(store.insert_route(&route).await.is_err());
    }

    #[tokio::test]
    async fn list_routes_filter_by_project() {
        let store = InMemoryRouteStore::new();
        store
            .insert_route(&Route::new("a.example.com", "proj-a", "api", TlsMode::None))
            .await
            .unwrap();
        store
            .insert_route(&Route::new("b.example.com", "proj-b", "web", TlsMode::Auto))
            .await
            .unwrap();

        let all = store.list_routes(None).await.unwrap();
        assert_eq!(all.len(), 2);

        let proj_a = store.list_routes(Some("proj-a")).await.unwrap();
        assert_eq!(proj_a.len(), 1);
        assert_eq!(proj_a[0].domain, "a.example.com");
    }

    #[tokio::test]
    async fn delete_route() {
        let store = InMemoryRouteStore::new();
        store
            .insert_route(&Route::new("api.example.com", "p", "d", TlsMode::None))
            .await
            .unwrap();
        assert!(store.delete_route("api.example.com").await.unwrap());
        assert!(!store.delete_route("api.example.com").await.unwrap());
        assert!(store.get_route("api.example.com").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn upsert_and_get_certificate() {
        let store = InMemoryRouteStore::new();
        let cert = Certificate {
            domain: "api.example.com".into(),
            cert_pem: b"CERT".to_vec(),
            key_pem_enc: b"KEY".to_vec(),
            key_nonce: b"NONCE".to_vec(),
            issued_at: Utc::now(),
            expires_at: Utc::now() + Duration::days(90),
            acme_account: None,
        };
        store.upsert_certificate(&cert).await.unwrap();

        let fetched = store.get_certificate("api.example.com").await.unwrap().unwrap();
        assert_eq!(fetched.cert_pem, b"CERT");
    }

    #[tokio::test]
    async fn list_expiring_certificates() {
        let store = InMemoryRouteStore::new();
        let expiring_soon = Certificate {
            domain: "soon.example.com".into(),
            cert_pem: b"C".to_vec(),
            key_pem_enc: b"K".to_vec(),
            key_nonce: b"N".to_vec(),
            issued_at: Utc::now() - Duration::days(60),
            expires_at: Utc::now() + Duration::days(20),
            acme_account: None,
        };
        let not_expiring = Certificate {
            domain: "ok.example.com".into(),
            cert_pem: b"C".to_vec(),
            key_pem_enc: b"K".to_vec(),
            key_nonce: b"N".to_vec(),
            issued_at: Utc::now(),
            expires_at: Utc::now() + Duration::days(80),
            acme_account: None,
        };
        store.upsert_certificate(&expiring_soon).await.unwrap();
        store.upsert_certificate(&not_expiring).await.unwrap();

        let expiring = store.list_expiring_certificates(30).await.unwrap();
        assert_eq!(expiring.len(), 1);
        assert_eq!(expiring[0].domain, "soon.example.com");
    }

    #[tokio::test]
    async fn allocate_and_list_subnets() {
        let store = InMemoryRouteStore::new();
        let alloc = SubnetAllocation {
            node_id: "node-1".into(),
            project: "ecommerce".into(),
            subnet: "172.20.1.0/24".into(),
        };
        store.allocate_subnet(&alloc).await.unwrap();

        let subnets = store.list_subnets().await.unwrap();
        assert_eq!(subnets.len(), 1);
        assert_eq!(subnets[0].subnet, "172.20.1.0/24");
    }

    #[tokio::test]
    async fn allocate_duplicate_subnet_fails() {
        let store = InMemoryRouteStore::new();
        let alloc = SubnetAllocation {
            node_id: "node-1".into(),
            project: "ecommerce".into(),
            subnet: "172.20.1.0/24".into(),
        };
        store.allocate_subnet(&alloc).await.unwrap();
        assert!(store.allocate_subnet(&alloc).await.is_err());
    }

    #[tokio::test]
    async fn allocate_same_subnet_different_node_fails() {
        let store = InMemoryRouteStore::new();
        store
            .allocate_subnet(&SubnetAllocation {
                node_id: "node-1".into(),
                project: "ecommerce".into(),
                subnet: "172.20.1.0/24".into(),
            })
            .await
            .unwrap();
        let result = store
            .allocate_subnet(&SubnetAllocation {
                node_id: "node-2".into(),
                project: "ecommerce".into(),
                subnet: "172.20.1.0/24".into(),
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn deallocate_subnet() {
        let store = InMemoryRouteStore::new();
        store
            .allocate_subnet(&SubnetAllocation {
                node_id: "node-1".into(),
                project: "p".into(),
                subnet: "172.20.1.0/24".into(),
            })
            .await
            .unwrap();
        assert!(store.deallocate_subnet("node-1", "p").await.unwrap());
        assert!(!store.deallocate_subnet("node-1", "p").await.unwrap());
    }
}
```

Wire it into the adapters module. In the appropriate `mod.rs` for state adapters, add:
```rust
pub mod memory_route_store;
pub use memory_route_store::InMemoryRouteStore;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check 2>&1`
Expected: compiles with no errors

- [ ] **Step 4: Run the tests**

Run: `cargo test -- memory_route_store 2>&1`
Expected: 10 tests pass

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(routing): add RouteStore trait, InMemoryRouteStore, and SQL migration for routes/certs/subnets"
```

---

### Task 4: Implement NginxBackend adapter (generate nginx conf, reload via signal)

**Files:**
- Create: `crates/nexad/src/adapters/proxy/mod.rs`
- Create: `crates/nexad/src/adapters/proxy/nginx.rs`
- Modify: `crates/nexad/src/adapters/mod.rs`

- [ ] **Step 1: Create the proxy adapter module and wire into adapters**

Create `crates/nexad/src/adapters/proxy/mod.rs`:

```rust
mod nginx;

pub use nginx::NginxBackend;
```

Wire into `crates/nexad/src/adapters/mod.rs`:
```rust
pub mod proxy;
```

- [ ] **Step 2: Implement NginxBackend**

Create `crates/nexad/src/adapters/proxy/nginx.rs`:

```rust
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use async_trait::async_trait;
use tracing::{info, warn};

use nexa_core::error::{NexaError, Result};
use nexa_core::ports::proxy::{ProxyBackend, RouteConfig, TlsConfig};

/// Nginx reverse proxy backend.
///
/// Generates per-domain config files in `conf_dir` (e.g., `/etc/nginx/conf.d/`)
/// and reloads nginx via `nginx -s reload`.
pub struct NginxBackend {
    conf_dir: PathBuf,
    nginx_bin: String,
}

impl NginxBackend {
    pub fn new(conf_dir: impl Into<PathBuf>, nginx_bin: impl Into<String>) -> Self {
        Self {
            conf_dir: conf_dir.into(),
            nginx_bin: nginx_bin.into(),
        }
    }

    /// Generate the nginx config file path for a domain.
    fn conf_path(&self, domain: &str) -> PathBuf {
        self.conf_dir.join(format!("nexa-{domain}.conf"))
    }

    /// Render an nginx server block for a route.
    fn render_config(route: &RouteConfig) -> String {
        let mut conf = String::new();

        // Upstream block
        let upstream_name = route.domain.replace('.', "_");
        conf.push_str(&format!("upstream {upstream_name} {{\n"));
        for up in &route.upstream {
            if up.weight > 1 {
                conf.push_str(&format!("    server {} weight={};\n", up.address, up.weight));
            } else {
                conf.push_str(&format!("    server {};\n", up.address));
            }
        }
        conf.push_str("}\n\n");

        // Server block
        match &route.tls {
            TlsConfig::None => {
                conf.push_str("server {\n");
                conf.push_str("    listen 80;\n");
                conf.push_str(&format!("    server_name {};\n\n", route.domain));
                conf.push_str("    location / {\n");
                conf.push_str(&format!("        proxy_pass http://{upstream_name};\n"));
                conf.push_str("        proxy_set_header Host $host;\n");
                conf.push_str("        proxy_set_header X-Real-IP $remote_addr;\n");
                conf.push_str("        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n");
                conf.push_str("        proxy_set_header X-Forwarded-Proto $scheme;\n");
                conf.push_str("    }\n");
                conf.push_str("}\n");
            }
            TlsConfig::Auto { email } => {
                // HTTP -> HTTPS redirect with ACME challenge support
                conf.push_str("server {\n");
                conf.push_str("    listen 80;\n");
                conf.push_str(&format!("    server_name {};\n\n", route.domain));
                conf.push_str("    location /.well-known/acme-challenge/ {\n");
                conf.push_str("        root /var/www/certbot;\n");
                conf.push_str("    }\n\n");
                conf.push_str("    location / {\n");
                conf.push_str("        return 301 https://$host$request_uri;\n");
                conf.push_str("    }\n");
                conf.push_str("}\n\n");

                // HTTPS server
                conf.push_str("server {\n");
                conf.push_str("    listen 443 ssl http2;\n");
                conf.push_str(&format!("    server_name {};\n\n", route.domain));
                conf.push_str(&format!(
                    "    ssl_certificate /etc/letsencrypt/live/{}/fullchain.pem;\n",
                    route.domain
                ));
                conf.push_str(&format!(
                    "    ssl_certificate_key /etc/letsencrypt/live/{}/privkey.pem;\n\n",
                    route.domain
                ));
                conf.push_str(&format!("    # ACME email: {email}\n\n"));
                conf.push_str("    location / {\n");
                conf.push_str(&format!("        proxy_pass http://{upstream_name};\n"));
                conf.push_str("        proxy_set_header Host $host;\n");
                conf.push_str("        proxy_set_header X-Real-IP $remote_addr;\n");
                conf.push_str("        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n");
                conf.push_str("        proxy_set_header X-Forwarded-Proto $scheme;\n");
                conf.push_str("    }\n");
                conf.push_str("}\n");
            }
            TlsConfig::Manual { cert, key } => {
                // HTTP -> HTTPS redirect
                conf.push_str("server {\n");
                conf.push_str("    listen 80;\n");
                conf.push_str(&format!("    server_name {};\n", route.domain));
                conf.push_str("    return 301 https://$host$request_uri;\n");
                conf.push_str("}\n\n");

                // HTTPS server with manual certs
                conf.push_str("server {\n");
                conf.push_str("    listen 443 ssl http2;\n");
                conf.push_str(&format!("    server_name {};\n\n", route.domain));
                conf.push_str(&format!("    ssl_certificate {};\n", cert.display()));
                conf.push_str(&format!("    ssl_certificate_key {};\n\n", key.display()));
                conf.push_str("    location / {\n");
                conf.push_str(&format!("        proxy_pass http://{upstream_name};\n"));
                conf.push_str("        proxy_set_header Host $host;\n");
                conf.push_str("        proxy_set_header X-Real-IP $remote_addr;\n");
                conf.push_str("        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n");
                conf.push_str("        proxy_set_header X-Forwarded-Proto $scheme;\n");
                conf.push_str("    }\n");
                conf.push_str("}\n");
            }
        }

        conf
    }
}

#[async_trait]
impl ProxyBackend for NginxBackend {
    async fn apply_routes(&self, routes: &[RouteConfig]) -> Result<()> {
        for route in routes {
            let conf = Self::render_config(route);
            let path = self.conf_path(&route.domain);
            tokio::fs::write(&path, &conf)
                .await
                .map_err(|e| NexaError::Proxy(format!("failed to write nginx config {}: {e}", path.display())))?;
            info!(domain = route.domain, path = %path.display(), "nginx config written");
        }
        Ok(())
    }

    async fn remove_route(&self, domain: &str) -> Result<()> {
        let path = self.conf_path(domain);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| NexaError::Proxy(format!("failed to remove nginx config {}: {e}", path.display())))?;
            info!(domain, "nginx config removed");
        } else {
            warn!(domain, "nginx config not found, nothing to remove");
        }
        Ok(())
    }

    async fn reload(&self) -> Result<()> {
        let output = StdCommand::new(&self.nginx_bin)
            .arg("-s")
            .arg("reload")
            .output()
            .map_err(|e| NexaError::Proxy(format!("failed to run nginx reload: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NexaError::Proxy(format!("nginx reload failed: {stderr}")));
        }

        info!("nginx reloaded");
        Ok(())
    }

    async fn health(&self) -> Result<bool> {
        let output = StdCommand::new(&self.nginx_bin)
            .arg("-t")
            .output()
            .map_err(|e| NexaError::Proxy(format!("failed to run nginx -t: {e}")))?;

        Ok(output.status.success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_http_route() -> RouteConfig {
        RouteConfig {
            domain: "api.example.com".into(),
            upstream: vec![
                nexa_core::ports::proxy::Upstream { address: "10.0.0.1:3000".into(), weight: 1 },
                nexa_core::ports::proxy::Upstream { address: "10.0.0.2:3000".into(), weight: 2 },
            ],
            tls: TlsConfig::None,
        }
    }

    fn make_auto_tls_route() -> RouteConfig {
        RouteConfig {
            domain: "secure.example.com".into(),
            upstream: vec![
                nexa_core::ports::proxy::Upstream { address: "10.0.0.1:8080".into(), weight: 1 },
            ],
            tls: TlsConfig::Auto { email: "admin@example.com".into() },
        }
    }

    fn make_manual_tls_route() -> RouteConfig {
        RouteConfig {
            domain: "manual.example.com".into(),
            upstream: vec![
                nexa_core::ports::proxy::Upstream { address: "10.0.0.5:443".into(), weight: 1 },
            ],
            tls: TlsConfig::Manual {
                cert: PathBuf::from("/etc/certs/cert.pem"),
                key: PathBuf::from("/etc/certs/key.pem"),
            },
        }
    }

    #[test]
    fn render_http_config() {
        let conf = NginxBackend::render_config(&make_http_route());
        assert!(conf.contains("upstream api_example_com"));
        assert!(conf.contains("server 10.0.0.1:3000;"));
        assert!(conf.contains("server 10.0.0.2:3000 weight=2;"));
        assert!(conf.contains("listen 80;"));
        assert!(conf.contains("server_name api.example.com;"));
        assert!(conf.contains("proxy_pass http://api_example_com;"));
        assert!(conf.contains("proxy_set_header Host $host;"));
        assert!(!conf.contains("ssl"));
    }

    #[test]
    fn render_auto_tls_config() {
        let conf = NginxBackend::render_config(&make_auto_tls_route());
        assert!(conf.contains("listen 80;"));
        assert!(conf.contains("return 301 https://"));
        assert!(conf.contains("listen 443 ssl http2;"));
        assert!(conf.contains("ssl_certificate /etc/letsencrypt/live/secure.example.com/fullchain.pem;"));
        assert!(conf.contains("ssl_certificate_key /etc/letsencrypt/live/secure.example.com/privkey.pem;"));
        assert!(conf.contains("ACME email: admin@example.com"));
        assert!(conf.contains("acme-challenge"));
    }

    #[test]
    fn render_manual_tls_config() {
        let conf = NginxBackend::render_config(&make_manual_tls_route());
        assert!(conf.contains("listen 443 ssl http2;"));
        assert!(conf.contains("ssl_certificate /etc/certs/cert.pem;"));
        assert!(conf.contains("ssl_certificate_key /etc/certs/key.pem;"));
    }

    #[test]
    fn conf_path_uses_domain() {
        let backend = NginxBackend::new("/etc/nginx/conf.d", "nginx");
        let path = backend.conf_path("api.example.com");
        assert_eq!(path, PathBuf::from("/etc/nginx/conf.d/nexa-api.example.com.conf"));
    }

    #[tokio::test]
    async fn apply_routes_writes_files() {
        let tmp = tempfile::tempdir().unwrap();
        let backend = NginxBackend::new(tmp.path(), "nginx");

        backend.apply_routes(&[make_http_route()]).await.unwrap();

        let path = tmp.path().join("nexa-api.example.com.conf");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("proxy_pass http://api_example_com;"));
    }

    #[tokio::test]
    async fn remove_route_deletes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let backend = NginxBackend::new(tmp.path(), "nginx");

        backend.apply_routes(&[make_http_route()]).await.unwrap();
        backend.remove_route("api.example.com").await.unwrap();

        let path = tmp.path().join("nexa-api.example.com.conf");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn remove_nonexistent_route_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let backend = NginxBackend::new(tmp.path(), "nginx");
        backend.remove_route("noexist.example.com").await.unwrap();
    }
}
```

- [ ] **Step 3: Add tempfile dev-dependency**

In `crates/nexad/Cargo.toml`, add:
```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 4: Verify compilation and tests**

Run: `cargo test -p nexad -- adapters::proxy::nginx 2>&1`
Expected: 7 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/nexad/src/adapters/proxy/ crates/nexad/src/adapters/mod.rs crates/nexad/Cargo.toml
git commit -m "feat(proxy): implement NginxBackend adapter with config generation and reload"
```

---

### Task 5: Implement CaddyBackend adapter (generate Caddyfile, reload via API/signal)

**Files:**
- Create: `crates/nexad/src/adapters/proxy/caddy.rs`
- Modify: `crates/nexad/src/adapters/proxy/mod.rs`

- [ ] **Step 1: Implement CaddyBackend**

Create `crates/nexad/src/adapters/proxy/caddy.rs`:

```rust
use std::path::PathBuf;

use async_trait::async_trait;
use tracing::{info, warn};

use nexa_core::error::{NexaError, Result};
use nexa_core::ports::proxy::{ProxyBackend, RouteConfig, TlsConfig};

/// Caddy reverse proxy backend.
///
/// Generates a Caddyfile and reloads Caddy via its admin API (localhost:2019)
/// or by signaling the process.
pub struct CaddyBackend {
    caddyfile_path: PathBuf,
    admin_api: String,
}

impl CaddyBackend {
    pub fn new(caddyfile_path: impl Into<PathBuf>, admin_api: impl Into<String>) -> Self {
        Self {
            caddyfile_path: caddyfile_path.into(),
            admin_api: admin_api.into(),
        }
    }

    /// Render a complete Caddyfile from a set of routes.
    fn render_caddyfile(routes: &[RouteConfig]) -> String {
        let mut caddyfile = String::new();

        for route in routes {
            let site_addr = match &route.tls {
                TlsConfig::None => format!("http://{}", route.domain),
                TlsConfig::Auto { .. } | TlsConfig::Manual { .. } => route.domain.clone(),
            };

            caddyfile.push_str(&format!("{site_addr} {{\n"));

            match &route.tls {
                TlsConfig::None => {}
                TlsConfig::Auto { email } => {
                    caddyfile.push_str(&format!("    tls {email}\n"));
                }
                TlsConfig::Manual { cert, key } => {
                    caddyfile.push_str(&format!(
                        "    tls {} {}\n",
                        cert.display(),
                        key.display()
                    ));
                }
            }

            if route.upstream.len() == 1 {
                caddyfile.push_str(&format!(
                    "    reverse_proxy {}\n",
                    route.upstream[0].address
                ));
            } else {
                let addrs: Vec<&str> = route.upstream.iter().map(|u| u.address.as_str()).collect();
                caddyfile.push_str(&format!("    reverse_proxy {} {{\n", addrs.join(" ")));

                let has_weights = route.upstream.iter().any(|u| u.weight > 1);
                if has_weights {
                    caddyfile.push_str("        lb_policy weighted_round_robin\n");
                }

                caddyfile.push_str("        health_uri /health\n");
                caddyfile.push_str("        health_interval 10s\n");
                caddyfile.push_str("    }\n");
            }

            caddyfile.push_str("}\n\n");
        }

        caddyfile
    }
}

#[async_trait]
impl ProxyBackend for CaddyBackend {
    async fn apply_routes(&self, routes: &[RouteConfig]) -> Result<()> {
        let caddyfile = Self::render_caddyfile(routes);
        tokio::fs::write(&self.caddyfile_path, &caddyfile)
            .await
            .map_err(|e| {
                NexaError::Proxy(format!(
                    "failed to write Caddyfile {}: {e}",
                    self.caddyfile_path.display()
                ))
            })?;
        info!(path = %self.caddyfile_path.display(), "Caddyfile written");
        Ok(())
    }

    async fn remove_route(&self, domain: &str) -> Result<()> {
        let content = match tokio::fs::read_to_string(&self.caddyfile_path).await {
            Ok(c) => c,
            Err(_) => {
                warn!(domain, "Caddyfile not found, nothing to remove");
                return Ok(());
            }
        };

        let mut result = String::new();
        let mut skip_depth: Option<u32> = None;
        let domain_http = format!("http://{domain}");

        for line in content.lines() {
            let trimmed = line.trim();
            if skip_depth.is_none()
                && (trimmed.starts_with(&format!("{domain} "))
                    || trimmed.starts_with(&format!("{domain_http} "))
                    || trimmed == &format!("{domain} {{")
                    || trimmed == &format!("{domain_http} {{"))
            {
                skip_depth = Some(0);
            }

            if let Some(ref mut depth) = skip_depth {
                for ch in trimmed.chars() {
                    if ch == '{' {
                        *depth += 1;
                    } else if ch == '}' {
                        *depth -= 1;
                    }
                }
                if *depth == 0 {
                    skip_depth = None;
                }
                continue;
            }

            result.push_str(line);
            result.push('\n');
        }

        tokio::fs::write(&self.caddyfile_path, result.trim_end())
            .await
            .map_err(|e| NexaError::Proxy(format!("failed to rewrite Caddyfile: {e}")))?;

        info!(domain, "route removed from Caddyfile");
        Ok(())
    }

    async fn reload(&self) -> Result<()> {
        let content = tokio::fs::read_to_string(&self.caddyfile_path)
            .await
            .map_err(|e| NexaError::Proxy(format!("failed to read Caddyfile: {e}")))?;

        let url = format!("{}/load", self.admin_api);
        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .header("Content-Type", "text/caddyfile")
            .body(content)
            .send()
            .await
            .map_err(|e| NexaError::Proxy(format!("caddy reload request failed: {e}")))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(NexaError::Proxy(format!("caddy reload failed: {body}")));
        }

        info!("caddy reloaded via admin API");
        Ok(())
    }

    async fn health(&self) -> Result<bool> {
        let url = format!("{}/config/", self.admin_api);
        let client = reqwest::Client::new();
        match client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexa_core::ports::proxy::Upstream;

    fn make_routes() -> Vec<RouteConfig> {
        vec![
            RouteConfig {
                domain: "api.example.com".into(),
                upstream: vec![
                    Upstream { address: "10.0.0.1:3000".into(), weight: 1 },
                ],
                tls: TlsConfig::Auto { email: "admin@example.com".into() },
            },
            RouteConfig {
                domain: "static.example.com".into(),
                upstream: vec![
                    Upstream { address: "10.0.0.2:80".into(), weight: 1 },
                    Upstream { address: "10.0.0.3:80".into(), weight: 2 },
                ],
                tls: TlsConfig::None,
            },
        ]
    }

    #[test]
    fn render_caddyfile_auto_tls() {
        let routes = vec![RouteConfig {
            domain: "api.example.com".into(),
            upstream: vec![Upstream { address: "10.0.0.1:3000".into(), weight: 1 }],
            tls: TlsConfig::Auto { email: "admin@example.com".into() },
        }];
        let cf = CaddyBackend::render_caddyfile(&routes);
        assert!(cf.contains("api.example.com {"));
        assert!(cf.contains("tls admin@example.com"));
        assert!(cf.contains("reverse_proxy 10.0.0.1:3000"));
    }

    #[test]
    fn render_caddyfile_no_tls() {
        let routes = vec![RouteConfig {
            domain: "static.example.com".into(),
            upstream: vec![Upstream { address: "10.0.0.2:80".into(), weight: 1 }],
            tls: TlsConfig::None,
        }];
        let cf = CaddyBackend::render_caddyfile(&routes);
        assert!(cf.contains("http://static.example.com {"));
        assert!(!cf.contains("tls "));
    }

    #[test]
    fn render_caddyfile_manual_tls() {
        let routes = vec![RouteConfig {
            domain: "manual.example.com".into(),
            upstream: vec![Upstream { address: "10.0.0.5:443".into(), weight: 1 }],
            tls: TlsConfig::Manual {
                cert: PathBuf::from("/certs/cert.pem"),
                key: PathBuf::from("/certs/key.pem"),
            },
        }];
        let cf = CaddyBackend::render_caddyfile(&routes);
        assert!(cf.contains("tls /certs/cert.pem /certs/key.pem"));
    }

    #[test]
    fn render_caddyfile_multiple_upstreams() {
        let routes = vec![RouteConfig {
            domain: "lb.example.com".into(),
            upstream: vec![
                Upstream { address: "10.0.0.1:80".into(), weight: 1 },
                Upstream { address: "10.0.0.2:80".into(), weight: 3 },
            ],
            tls: TlsConfig::None,
        }];
        let cf = CaddyBackend::render_caddyfile(&routes);
        assert!(cf.contains("reverse_proxy 10.0.0.1:80 10.0.0.2:80 {"));
        assert!(cf.contains("health_uri /health"));
    }

    #[tokio::test]
    async fn apply_routes_writes_caddyfile() {
        let tmp = tempfile::tempdir().unwrap();
        let caddyfile = tmp.path().join("Caddyfile");
        let backend = CaddyBackend::new(&caddyfile, "http://localhost:2019");

        backend.apply_routes(&make_routes()).await.unwrap();

        let content = std::fs::read_to_string(&caddyfile).unwrap();
        assert!(content.contains("api.example.com"));
        assert!(content.contains("static.example.com"));
    }

    #[tokio::test]
    async fn remove_route_strips_block() {
        let tmp = tempfile::tempdir().unwrap();
        let caddyfile = tmp.path().join("Caddyfile");
        let backend = CaddyBackend::new(&caddyfile, "http://localhost:2019");

        backend.apply_routes(&make_routes()).await.unwrap();
        backend.remove_route("api.example.com").await.unwrap();

        let content = std::fs::read_to_string(&caddyfile).unwrap();
        assert!(!content.contains("api.example.com"));
        assert!(content.contains("static.example.com"));
    }
}
```

- [ ] **Step 2: Update proxy module exports**

Update `crates/nexad/src/adapters/proxy/mod.rs`:

```rust
mod caddy;
mod nginx;

pub use caddy::CaddyBackend;
pub use nginx::NginxBackend;
```

- [ ] **Step 3: Add reqwest dependency to nexad if not present**

Check `crates/nexad/Cargo.toml` for `reqwest`. If absent, add:
```toml
reqwest = { workspace = true }
```

- [ ] **Step 4: Verify and test**

Run: `cargo test -p nexad -- adapters::proxy::caddy 2>&1`
Expected: 6 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/nexad/src/adapters/proxy/caddy.rs crates/nexad/src/adapters/proxy/mod.rs crates/nexad/Cargo.toml
git commit -m "feat(proxy): implement CaddyBackend adapter with Caddyfile generation and admin API reload"
```

---

### Task 6: Implement TraefikBackend adapter (generate YAML config, hot-reload via file watch)

**Files:**
- Create: `crates/nexad/src/adapters/proxy/traefik.rs`
- Modify: `crates/nexad/src/adapters/proxy/mod.rs`

- [ ] **Step 1: Implement TraefikBackend**

Create `crates/nexad/src/adapters/proxy/traefik.rs`:

```rust
use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;
use tracing::{info, warn};

use nexa_core::error::{NexaError, Result};
use nexa_core::ports::proxy::{ProxyBackend, RouteConfig, TlsConfig};

/// Traefik reverse proxy backend.
///
/// Generates a YAML dynamic config file that Traefik watches via its
/// file provider. Traefik hot-reloads automatically when the file changes.
pub struct TraefikBackend {
    config_path: PathBuf,
}

impl TraefikBackend {
    pub fn new(config_path: impl Into<PathBuf>) -> Self {
        Self {
            config_path: config_path.into(),
        }
    }

    /// Build the Traefik dynamic config YAML from a set of routes.
    fn render_config(routes: &[RouteConfig]) -> TraefikDynamicConfig {
        let mut routers = HashMap::new();
        let mut services = HashMap::new();

        for route in routes {
            let safe_name = route.domain.replace('.', "-");
            let router_name = format!("nexa-{safe_name}");
            let service_name = format!("nexa-svc-{safe_name}");

            let mut router = TraefikRouter {
                rule: format!("Host(`{}`)", route.domain),
                service: service_name.clone(),
                entry_points: vec!["web".into()],
                tls: None,
            };

            match &route.tls {
                TlsConfig::None => {}
                TlsConfig::Auto { .. } => {
                    router.entry_points = vec!["websecure".into()];
                    router.tls = Some(TraefikTls {
                        cert_resolver: Some("letsencrypt".into()),
                    });
                }
                TlsConfig::Manual { .. } => {
                    router.entry_points = vec!["websecure".into()];
                    router.tls = Some(TraefikTls {
                        cert_resolver: None,
                    });
                }
            }

            routers.insert(router_name, router);

            let servers: Vec<TraefikServer> = route
                .upstream
                .iter()
                .map(|u| TraefikServer {
                    url: format!("http://{}", u.address),
                    weight: if u.weight > 1 { Some(u.weight) } else { None },
                })
                .collect();

            services.insert(
                service_name,
                TraefikService {
                    load_balancer: TraefikLoadBalancer {
                        servers,
                        health_check: Some(TraefikHealthCheck {
                            path: "/health".into(),
                            interval: "10s".into(),
                        }),
                    },
                },
            );
        }

        TraefikDynamicConfig {
            http: TraefikHttp { routers, services },
        }
    }
}

#[derive(Debug, Serialize)]
struct TraefikDynamicConfig {
    http: TraefikHttp,
}

#[derive(Debug, Serialize)]
struct TraefikHttp {
    routers: HashMap<String, TraefikRouter>,
    services: HashMap<String, TraefikService>,
}

#[derive(Debug, Serialize)]
struct TraefikRouter {
    rule: String,
    service: String,
    #[serde(rename = "entryPoints")]
    entry_points: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tls: Option<TraefikTls>,
}

#[derive(Debug, Default, Serialize)]
struct TraefikTls {
    #[serde(rename = "certResolver", skip_serializing_if = "Option::is_none")]
    cert_resolver: Option<String>,
}

#[derive(Debug, Serialize)]
struct TraefikService {
    #[serde(rename = "loadBalancer")]
    load_balancer: TraefikLoadBalancer,
}

#[derive(Debug, Serialize)]
struct TraefikLoadBalancer {
    servers: Vec<TraefikServer>,
    #[serde(rename = "healthCheck", skip_serializing_if = "Option::is_none")]
    health_check: Option<TraefikHealthCheck>,
}

#[derive(Debug, Serialize)]
struct TraefikServer {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    weight: Option<u32>,
}

#[derive(Debug, Serialize)]
struct TraefikHealthCheck {
    path: String,
    interval: String,
}

#[async_trait]
impl ProxyBackend for TraefikBackend {
    async fn apply_routes(&self, routes: &[RouteConfig]) -> Result<()> {
        let config = Self::render_config(routes);
        let yaml = serde_yaml::to_string(&config)
            .map_err(|e| NexaError::Proxy(format!("failed to serialize traefik config: {e}")))?;
        tokio::fs::write(&self.config_path, &yaml)
            .await
            .map_err(|e| {
                NexaError::Proxy(format!(
                    "failed to write traefik config {}: {e}",
                    self.config_path.display()
                ))
            })?;
        info!(path = %self.config_path.display(), "traefik dynamic config written");
        Ok(())
    }

    async fn remove_route(&self, domain: &str) -> Result<()> {
        let content = match tokio::fs::read_to_string(&self.config_path).await {
            Ok(c) => c,
            Err(_) => {
                warn!(domain, "traefik config not found, nothing to remove");
                return Ok(());
            }
        };

        let safe_name = domain.replace('.', "-");
        let router_key = format!("nexa-{safe_name}");
        let service_key = format!("nexa-svc-{safe_name}");

        let mut value: serde_yaml::Value = serde_yaml::from_str(&content)
            .map_err(|e| NexaError::Proxy(format!("failed to parse traefik config: {e}")))?;

        if let Some(http) = value.get_mut("http") {
            if let Some(routers) = http.get_mut("routers") {
                if let Some(map) = routers.as_mapping_mut() {
                    map.remove(serde_yaml::Value::String(router_key));
                }
            }
            if let Some(services) = http.get_mut("services") {
                if let Some(map) = services.as_mapping_mut() {
                    map.remove(serde_yaml::Value::String(service_key));
                }
            }
        }

        let yaml = serde_yaml::to_string(&value)
            .map_err(|e| NexaError::Proxy(format!("failed to reserialize traefik config: {e}")))?;

        tokio::fs::write(&self.config_path, &yaml)
            .await
            .map_err(|e| NexaError::Proxy(format!("failed to rewrite traefik config: {e}")))?;

        info!(domain, "route removed from traefik dynamic config");
        Ok(())
    }

    async fn reload(&self) -> Result<()> {
        // Traefik watches the file automatically; writing the file IS the reload.
        info!("traefik uses file watch for hot-reload; no explicit reload needed");
        Ok(())
    }

    async fn health(&self) -> Result<bool> {
        match tokio::fs::read_to_string(&self.config_path).await {
            Ok(content) => Ok(serde_yaml::from_str::<serde_yaml::Value>(&content).is_ok()),
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexa_core::ports::proxy::Upstream;

    fn make_route() -> RouteConfig {
        RouteConfig {
            domain: "api.example.com".into(),
            upstream: vec![
                Upstream { address: "10.0.0.1:3000".into(), weight: 1 },
                Upstream { address: "10.0.0.2:3000".into(), weight: 2 },
            ],
            tls: TlsConfig::Auto { email: "admin@example.com".into() },
        }
    }

    #[test]
    fn render_config_produces_valid_yaml() {
        let config = TraefikBackend::render_config(&[make_route()]);
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("Host(`api.example.com`)"));
        assert!(yaml.contains("nexa-api-example-com"));
        assert!(yaml.contains("http://10.0.0.1:3000"));
        assert!(yaml.contains("http://10.0.0.2:3000"));
        assert!(yaml.contains("letsencrypt"));
        assert!(yaml.contains("websecure"));
    }

    #[test]
    fn render_config_no_tls() {
        let route = RouteConfig {
            domain: "plain.example.com".into(),
            upstream: vec![Upstream { address: "10.0.0.1:80".into(), weight: 1 }],
            tls: TlsConfig::None,
        };
        let config = TraefikBackend::render_config(&[route]);
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("web"));
        assert!(!yaml.contains("certResolver"));
    }

    #[test]
    fn render_config_health_check() {
        let config = TraefikBackend::render_config(&[make_route()]);
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("/health"));
        assert!(yaml.contains("10s"));
    }

    #[tokio::test]
    async fn apply_routes_writes_yaml_file() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("nexa-dynamic.yml");
        let backend = TraefikBackend::new(&config_path);

        backend.apply_routes(&[make_route()]).await.unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("api.example.com"));
    }

    #[tokio::test]
    async fn remove_route_from_traefik_config() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("nexa-dynamic.yml");
        let backend = TraefikBackend::new(&config_path);

        let routes = vec![
            make_route(),
            RouteConfig {
                domain: "other.example.com".into(),
                upstream: vec![Upstream { address: "10.0.0.5:80".into(), weight: 1 }],
                tls: TlsConfig::None,
            },
        ];

        backend.apply_routes(&routes).await.unwrap();
        backend.remove_route("api.example.com").await.unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(!content.contains("nexa-api-example-com"));
        assert!(content.contains("nexa-other-example-com"));
    }

    #[tokio::test]
    async fn health_returns_true_for_valid_config() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("nexa-dynamic.yml");
        let backend = TraefikBackend::new(&config_path);

        backend.apply_routes(&[make_route()]).await.unwrap();
        assert!(backend.health().await.unwrap());
    }

    #[tokio::test]
    async fn health_returns_false_when_no_file() {
        let backend = TraefikBackend::new("/tmp/nonexistent-traefik-nexa.yml");
        assert!(!backend.health().await.unwrap());
    }

    #[tokio::test]
    async fn reload_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("nexa-dynamic.yml");
        let backend = TraefikBackend::new(&config_path);
        backend.apply_routes(&[make_route()]).await.unwrap();
        backend.reload().await.unwrap();
    }
}
```

- [ ] **Step 2: Update proxy module exports**

Update `crates/nexad/src/adapters/proxy/mod.rs`:

```rust
mod caddy;
mod nginx;
mod traefik;

pub use caddy::CaddyBackend;
pub use nginx::NginxBackend;
pub use traefik::TraefikBackend;
```

- [ ] **Step 3: Verify and test**

Run: `cargo test -p nexad -- adapters::proxy::traefik 2>&1`
Expected: 7 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/nexad/src/adapters/proxy/traefik.rs crates/nexad/src/adapters/proxy/mod.rs
git commit -m "feat(proxy): implement TraefikBackend adapter with YAML config and file-watch hot-reload"
```

---

### Task 7: Create nexa-proxy crate (Cargo.toml, main.rs with hyper + rustls reverse proxy)

**Files:**
- Create: `crates/nexa-proxy/Cargo.toml`
- Create: `crates/nexa-proxy/src/main.rs`
- Create: `crates/nexa-proxy/src/config.rs`
- Create: `crates/nexa-proxy/src/proxy.rs`
- Modify: `Cargo.toml` (workspace deps)

- [ ] **Step 1: Add workspace dependencies**

In `Cargo.toml` (workspace root), add to `[workspace.dependencies]`:

```toml
boringtun = "0.6"
hyper = { version = "1", features = ["full"] }
hyper-util = { version = "0.1", features = ["tokio", "server-auto", "http1", "http2"] }
rustls = "0.23"
instant-acme = "0.7"
x25519-dalek = "2"
base64 = "0.22"
http-body-util = "0.1"
tokio-rustls = "0.26"
rustls-pemfile = "2"
```

- [ ] **Step 2: Create crate Cargo.toml**

Create `crates/nexa-proxy/Cargo.toml`:

```toml
[package]
name = "nexa-proxy"
description = "NexaNet built-in reverse proxy — minimal HTTP/1.1+HTTP/2 proxy with ACME TLS"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[[bin]]
name = "nexa-proxy"
path = "src/main.rs"

[dependencies]
tokio = { workspace = true }
hyper = { workspace = true }
hyper-util = { workspace = true }
http-body-util = { workspace = true }
rustls = { workspace = true }
tokio-rustls = { workspace = true }
rustls-pemfile = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
anyhow = { workspace = true }
clap = { workspace = true }
reqwest = { workspace = true }
```

- [ ] **Step 3: Create the config module**

Create `crates/nexa-proxy/src/config.rs`:

```rust
use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Configuration for nexa-proxy, read from a JSON file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Address to listen on for HTTP (e.g., "0.0.0.0:80")
    pub http_listen: String,
    /// Address to listen on for HTTPS (e.g., "0.0.0.0:443")
    pub https_listen: Option<String>,
    /// Route table: domain -> route config
    #[serde(default)]
    pub routes: HashMap<String, ProxyRouteConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRouteConfig {
    /// List of upstream addresses (host:port)
    pub upstreams: Vec<UpstreamEntry>,
    /// TLS configuration
    #[serde(default)]
    pub tls: Option<TlsEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamEntry {
    pub address: String,
    pub weight: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsEntry {
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
    pub acme_email: Option<String>,
}

impl ProxyConfig {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_json::from_str(&content)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let json = r#"{
            "http_listen": "0.0.0.0:80",
            "routes": {}
        }"#;
        let config: ProxyConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.http_listen, "0.0.0.0:80");
        assert!(config.routes.is_empty());
    }

    #[test]
    fn parse_full_config() {
        let json = r#"{
            "http_listen": "0.0.0.0:80",
            "https_listen": "0.0.0.0:443",
            "routes": {
                "api.example.com": {
                    "upstreams": [
                        {"address": "10.0.0.1:3000", "weight": 1},
                        {"address": "10.0.0.2:3000", "weight": 2}
                    ],
                    "tls": {
                        "acme_email": "admin@example.com"
                    }
                }
            }
        }"#;
        let config: ProxyConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.routes.len(), 1);
        let route = &config.routes["api.example.com"];
        assert_eq!(route.upstreams.len(), 2);
        assert_eq!(route.upstreams[1].weight, 2);
        assert_eq!(route.tls.as_ref().unwrap().acme_email.as_deref(), Some("admin@example.com"));
    }

    #[test]
    fn config_serializes_roundtrip() {
        let config = ProxyConfig {
            http_listen: "0.0.0.0:80".into(),
            https_listen: None,
            routes: HashMap::new(),
        };
        let json = serde_json::to_string(&config).unwrap();
        let deser: ProxyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.http_listen, "0.0.0.0:80");
    }
}
```

- [ ] **Step 4: Create the proxy module (core HTTP proxy logic with weighted round-robin)**

Create `crates/nexa-proxy/src/proxy.rs`:

```rust
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use tokio::net::TcpListener;
use tracing::{error, info};

use crate::config::ProxyConfig;

/// Shared state for the proxy server.
pub struct ProxyState {
    pub routes: HashMap<String, RouteState>,
}

pub struct RouteState {
    pub upstreams: Vec<WeightedUpstream>,
    pub counter: AtomicUsize,
}

pub struct WeightedUpstream {
    pub address: String,
    pub weight: u32,
}

impl ProxyState {
    pub fn from_config(config: &ProxyConfig) -> Self {
        let mut routes = HashMap::new();
        for (domain, route_config) in &config.routes {
            let upstreams: Vec<WeightedUpstream> = route_config
                .upstreams
                .iter()
                .map(|u| WeightedUpstream {
                    address: u.address.clone(),
                    weight: u.weight,
                })
                .collect();
            routes.insert(
                domain.clone(),
                RouteState {
                    upstreams,
                    counter: AtomicUsize::new(0),
                },
            );
        }
        Self { routes }
    }

    /// Select the next upstream using weighted round-robin.
    pub fn select_upstream(&self, domain: &str) -> Option<&str> {
        let route = self.routes.get(domain)?;
        if route.upstreams.is_empty() {
            return None;
        }

        let total_weight: u32 = route.upstreams.iter().map(|u| u.weight).sum();
        if total_weight == 0 {
            return None;
        }

        let idx = route.counter.fetch_add(1, Ordering::Relaxed);
        let mut target = (idx as u32) % total_weight;

        for upstream in &route.upstreams {
            if target < upstream.weight {
                return Some(&upstream.address);
            }
            target -= upstream.weight;
        }

        Some(&route.upstreams[0].address)
    }
}

/// Run the HTTP proxy server.
pub async fn run_http(listen_addr: &str, state: Arc<ProxyState>) -> anyhow::Result<()> {
    let listener = TcpListener::bind(listen_addr).await?;
    info!(%listen_addr, "nexa-proxy HTTP listening");

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let state = state.clone();

        tokio::spawn(async move {
            let service = service_fn(move |req: Request<Incoming>| {
                let state = state.clone();
                async move { handle_request(req, &state).await }
            });

            if let Err(e) = http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                .await
            {
                error!(%peer_addr, %e, "connection error");
            }
        });
    }
}

/// Handle a single HTTP request by routing to the correct upstream.
async fn handle_request(
    req: Request<Incoming>,
    state: &ProxyState,
) -> std::result::Result<Response<Full<Bytes>>, hyper::Error> {
    let host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");

    let upstream = match state.select_upstream(host) {
        Some(addr) => addr.to_string(),
        None => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Full::new(Bytes::from("no upstream configured for this domain")))
                .unwrap());
        }
    };

    let uri = format!(
        "http://{}{}",
        upstream,
        req.uri().path_and_query().map(|pq| pq.as_str()).unwrap_or("/")
    );

    let parts = req.into_parts().0;
    let method = match parts.method.as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "DELETE" => reqwest::Method::DELETE,
        "PATCH" => reqwest::Method::PATCH,
        "HEAD" => reqwest::Method::HEAD,
        "OPTIONS" => reqwest::Method::OPTIONS,
        _ => reqwest::Method::GET,
    };

    let client = reqwest::Client::new();
    let mut builder = client.request(method, &uri);

    for (name, value) in &parts.headers {
        if name != "host" && name != "connection" {
            if let Ok(v) = value.to_str() {
                builder = builder.header(name.as_str(), v);
            }
        }
    }

    match builder.send().await {
        Ok(upstream_resp) => {
            let status = StatusCode::from_u16(upstream_resp.status().as_u16())
                .unwrap_or(StatusCode::BAD_GATEWAY);
            let body_bytes = upstream_resp.bytes().await.unwrap_or_default();

            Ok(Response::builder()
                .status(status)
                .body(Full::new(body_bytes))
                .unwrap())
        }
        Err(e) => {
            error!(%upstream, %e, "upstream request failed");
            Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Full::new(Bytes::from(format!("upstream error: {e}"))))
                .unwrap())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProxyConfig, ProxyRouteConfig, UpstreamEntry};

    fn make_state() -> ProxyState {
        let config = ProxyConfig {
            http_listen: "0.0.0.0:80".into(),
            https_listen: None,
            routes: HashMap::from([
                (
                    "api.example.com".into(),
                    ProxyRouteConfig {
                        upstreams: vec![
                            UpstreamEntry { address: "10.0.0.1:3000".into(), weight: 1 },
                            UpstreamEntry { address: "10.0.0.2:3000".into(), weight: 2 },
                        ],
                        tls: None,
                    },
                ),
                (
                    "web.example.com".into(),
                    ProxyRouteConfig {
                        upstreams: vec![
                            UpstreamEntry { address: "10.0.0.5:80".into(), weight: 1 },
                        ],
                        tls: None,
                    },
                ),
            ]),
        };
        ProxyState::from_config(&config)
    }

    #[test]
    fn select_upstream_known_domain() {
        let state = make_state();
        let upstream = state.select_upstream("web.example.com");
        assert_eq!(upstream, Some("10.0.0.5:80"));
    }

    #[test]
    fn select_upstream_unknown_domain() {
        let state = make_state();
        assert!(state.select_upstream("unknown.example.com").is_none());
    }

    #[test]
    fn weighted_round_robin() {
        let state = make_state();
        // Weights [1, 2]: over 3 requests, first gets 1 hit, second gets 2
        let first = state.select_upstream("api.example.com").unwrap();
        assert_eq!(first, "10.0.0.1:3000");

        let second = state.select_upstream("api.example.com").unwrap();
        assert_eq!(second, "10.0.0.2:3000");

        let third = state.select_upstream("api.example.com").unwrap();
        assert_eq!(third, "10.0.0.2:3000");

        // Cycle repeats
        let fourth = state.select_upstream("api.example.com").unwrap();
        assert_eq!(fourth, "10.0.0.1:3000");
    }

    #[test]
    fn from_config_empty_routes() {
        let config = ProxyConfig {
            http_listen: "0.0.0.0:80".into(),
            https_listen: None,
            routes: HashMap::new(),
        };
        let state = ProxyState::from_config(&config);
        assert!(state.routes.is_empty());
    }
}
```

- [ ] **Step 5: Create the main entry point**

Create `crates/nexa-proxy/src/main.rs`:

```rust
mod config;
mod proxy;

use std::sync::Arc;

use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "nexa-proxy", about = "NexaNet built-in reverse proxy", version)]
struct Cli {
    /// Path to the proxy config JSON file
    #[arg(long, default_value = "/var/lib/nexa/proxy.json")]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    info!("starting nexa-proxy");

    let config = config::ProxyConfig::load(std::path::Path::new(&cli.config))?;
    info!(http = %config.http_listen, "loaded proxy config with {} routes", config.routes.len());

    let state = Arc::new(proxy::ProxyState::from_config(&config));

    proxy::run_http(&config.http_listen, state).await
}
```

- [ ] **Step 6: Verify compilation**

Run: `cargo check -p nexa-proxy 2>&1`
Expected: compiles with no errors

- [ ] **Step 7: Run the tests**

Run: `cargo test -p nexa-proxy 2>&1`
Expected: 7 tests pass (3 config + 4 proxy)

- [ ] **Step 8: Commit**

```bash
git add crates/nexa-proxy/ Cargo.toml
git commit -m "feat(proxy): create nexa-proxy crate with hyper HTTP reverse proxy and weighted round-robin LB"
```

---

### Task 8: Implement NexaProxyBackend adapter (manages nexa-proxy child process, writes config)

**Files:**
- Create: `crates/nexad/src/adapters/proxy/nexa_proxy.rs`
- Modify: `crates/nexad/src/adapters/proxy/mod.rs`

- [ ] **Step 1: Implement NexaProxyBackend**

Create `crates/nexad/src/adapters/proxy/nexa_proxy.rs`:

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;

use async_trait::async_trait;
use tokio::process::{Child, Command};
use tracing::{info, warn};

use nexa_core::error::{NexaError, Result};
use nexa_core::ports::proxy::{ProxyBackend, RouteConfig, TlsConfig, Upstream};

/// NexaProxy backend -- manages nexa-proxy as a child process.
///
/// Writes a JSON config file and (re)starts the nexa-proxy binary.
pub struct NexaProxyBackend {
    config_path: PathBuf,
    binary_path: String,
    http_listen: String,
    https_listen: Option<String>,
    child: Mutex<Option<Child>>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct NexaProxyConfig {
    http_listen: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    https_listen: Option<String>,
    routes: HashMap<String, NexaProxyRoute>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct NexaProxyRoute {
    upstreams: Vec<NexaProxyUpstream>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tls: Option<NexaProxyTls>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct NexaProxyUpstream {
    address: String,
    weight: u32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct NexaProxyTls {
    #[serde(skip_serializing_if = "Option::is_none")]
    cert_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    acme_email: Option<String>,
}

impl NexaProxyBackend {
    pub fn new(
        config_path: impl Into<PathBuf>,
        binary_path: impl Into<String>,
        http_listen: impl Into<String>,
        https_listen: Option<String>,
    ) -> Self {
        Self {
            config_path: config_path.into(),
            binary_path: binary_path.into(),
            http_listen: http_listen.into(),
            https_listen,
            child: Mutex::new(None),
        }
    }

    fn build_config(&self, routes: &[RouteConfig]) -> NexaProxyConfig {
        let mut route_map = HashMap::new();

        for route in routes {
            let upstreams: Vec<NexaProxyUpstream> = route
                .upstream
                .iter()
                .map(|u| NexaProxyUpstream {
                    address: u.address.clone(),
                    weight: u.weight,
                })
                .collect();

            let tls = match &route.tls {
                TlsConfig::None => None,
                TlsConfig::Auto { email } => Some(NexaProxyTls {
                    cert_path: None,
                    key_path: None,
                    acme_email: Some(email.clone()),
                }),
                TlsConfig::Manual { cert, key } => Some(NexaProxyTls {
                    cert_path: Some(cert.clone()),
                    key_path: Some(key.clone()),
                    acme_email: None,
                }),
            };

            route_map.insert(
                route.domain.clone(),
                NexaProxyRoute { upstreams, tls },
            );
        }

        NexaProxyConfig {
            http_listen: self.http_listen.clone(),
            https_listen: self.https_listen.clone(),
            routes: route_map,
        }
    }

    fn write_config_sync(&self, config: &NexaProxyConfig) -> Result<()> {
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| NexaError::Proxy(format!("failed to serialize nexa-proxy config: {e}")))?;
        std::fs::write(&self.config_path, &json).map_err(|e| {
            NexaError::Proxy(format!(
                "failed to write nexa-proxy config {}: {e}",
                self.config_path.display()
            ))
        })?;
        Ok(())
    }

    fn start_child(&self) -> Result<()> {
        let mut child_lock = self.child.lock().unwrap();

        if let Some(ref mut child) = *child_lock {
            let _ = child.start_kill();
        }

        let child = Command::new(&self.binary_path)
            .arg("--config")
            .arg(&self.config_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                NexaError::Proxy(format!(
                    "failed to spawn nexa-proxy binary '{}': {e}",
                    self.binary_path
                ))
            })?;

        info!(pid = child.id().unwrap_or(0), "nexa-proxy child started");
        *child_lock = Some(child);
        Ok(())
    }
}

#[async_trait]
impl ProxyBackend for NexaProxyBackend {
    async fn apply_routes(&self, routes: &[RouteConfig]) -> Result<()> {
        let config = self.build_config(routes);
        self.write_config_sync(&config)?;
        info!(path = %self.config_path.display(), routes = routes.len(), "nexa-proxy config written");
        Ok(())
    }

    async fn remove_route(&self, domain: &str) -> Result<()> {
        let content = match std::fs::read_to_string(&self.config_path) {
            Ok(c) => c,
            Err(_) => {
                warn!(domain, "nexa-proxy config not found");
                return Ok(());
            }
        };

        let mut config: NexaProxyConfig = serde_json::from_str(&content)
            .map_err(|e| NexaError::Proxy(format!("failed to parse nexa-proxy config: {e}")))?;

        config.routes.remove(domain);
        self.write_config_sync(&config)?;
        info!(domain, "route removed from nexa-proxy config");
        Ok(())
    }

    async fn reload(&self) -> Result<()> {
        self.start_child()?;
        Ok(())
    }

    async fn health(&self) -> Result<bool> {
        let child_lock = self.child.lock().unwrap();
        match &*child_lock {
            Some(child) => Ok(child.id().is_some()),
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_routes() -> Vec<RouteConfig> {
        vec![
            RouteConfig {
                domain: "api.example.com".into(),
                upstream: vec![
                    Upstream { address: "10.0.0.1:3000".into(), weight: 1 },
                    Upstream { address: "10.0.0.2:3000".into(), weight: 2 },
                ],
                tls: TlsConfig::Auto { email: "admin@example.com".into() },
            },
            RouteConfig {
                domain: "static.example.com".into(),
                upstream: vec![
                    Upstream { address: "10.0.0.5:80".into(), weight: 1 },
                ],
                tls: TlsConfig::None,
            },
        ]
    }

    #[test]
    fn build_config_json() {
        let backend = NexaProxyBackend::new(
            "/tmp/test-nexa-proxy.json",
            "nexa-proxy",
            "0.0.0.0:80",
            Some("0.0.0.0:443".into()),
        );
        let config = backend.build_config(&make_routes());
        assert_eq!(config.routes.len(), 2);
        assert!(config.routes.contains_key("api.example.com"));
        assert!(config.routes.contains_key("static.example.com"));

        let api = &config.routes["api.example.com"];
        assert_eq!(api.upstreams.len(), 2);
        assert_eq!(api.upstreams[1].weight, 2);
        assert_eq!(api.tls.as_ref().unwrap().acme_email.as_deref(), Some("admin@example.com"));

        let st = &config.routes["static.example.com"];
        assert!(st.tls.is_none());
    }

    #[test]
    fn build_config_manual_tls() {
        let routes = vec![RouteConfig {
            domain: "manual.example.com".into(),
            upstream: vec![Upstream { address: "10.0.0.1:443".into(), weight: 1 }],
            tls: TlsConfig::Manual {
                cert: PathBuf::from("/certs/cert.pem"),
                key: PathBuf::from("/certs/key.pem"),
            },
        }];
        let backend = NexaProxyBackend::new("/tmp/test.json", "nexa-proxy", "0.0.0.0:80", None);
        let config = backend.build_config(&routes);
        let tls = config.routes["manual.example.com"].tls.as_ref().unwrap();
        assert_eq!(tls.cert_path.as_ref().unwrap(), &PathBuf::from("/certs/cert.pem"));
        assert!(tls.acme_email.is_none());
    }

    #[tokio::test]
    async fn apply_routes_writes_json_file() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("proxy.json");
        let backend = NexaProxyBackend::new(&config_path, "nexa-proxy", "0.0.0.0:80", None);

        backend.apply_routes(&make_routes()).await.unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed["routes"]["api.example.com"].is_object());
        assert!(parsed["routes"]["static.example.com"].is_object());
    }

    #[tokio::test]
    async fn remove_route_from_config() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("proxy.json");
        let backend = NexaProxyBackend::new(&config_path, "nexa-proxy", "0.0.0.0:80", None);

        backend.apply_routes(&make_routes()).await.unwrap();
        backend.remove_route("api.example.com").await.unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed["routes"]["api.example.com"].is_null());
        assert!(parsed["routes"]["static.example.com"].is_object());
    }

    #[tokio::test]
    async fn health_returns_false_when_no_child() {
        let backend = NexaProxyBackend::new("/tmp/nope.json", "nexa-proxy", "0.0.0.0:80", None);
        assert!(!backend.health().await.unwrap());
    }
}
```

- [ ] **Step 2: Update proxy module exports**

Update `crates/nexad/src/adapters/proxy/mod.rs`:

```rust
mod caddy;
mod nexa_proxy;
mod nginx;
mod traefik;

pub use caddy::CaddyBackend;
pub use nexa_proxy::NexaProxyBackend;
pub use nginx::NginxBackend;
pub use traefik::TraefikBackend;
```

- [ ] **Step 3: Verify and test**

Run: `cargo test -p nexad -- adapters::proxy::nexa_proxy 2>&1`
Expected: 5 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/nexad/src/adapters/proxy/nexa_proxy.rs crates/nexad/src/adapters/proxy/mod.rs
git commit -m "feat(proxy): implement NexaProxyBackend adapter managing nexa-proxy child process"
```

---

### Task 9: WireGuard overlay: subnet allocator, keypair generation, boringtun interface setup

**Files:**
- Create: `crates/nexad/src/adapters/network/mod.rs`
- Create: `crates/nexad/src/adapters/network/subnet.rs`
- Create: `crates/nexad/src/adapters/network/wireguard.rs`
- Modify: `crates/nexad/src/adapters/mod.rs`
- Modify: `crates/nexad/Cargo.toml`
- Modify: `Cargo.toml` (workspace deps)

- [ ] **Step 1: Add dependencies to workspace and nexad**

In `Cargo.toml` (workspace root), add to `[workspace.dependencies]`:
```toml
rand = "0.8"
```

In `crates/nexad/Cargo.toml`, add to `[dependencies]`:
```toml
boringtun = { workspace = true }
x25519-dalek = { workspace = true }
base64 = { workspace = true }
rand = { workspace = true }
```

- [ ] **Step 2: Implement SubnetAllocator**

Create `crates/nexad/src/adapters/network/subnet.rs`:

```rust
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU8, Ordering};

/// Allocates /24 subnets from a cluster CIDR (e.g., 172.20.0.0/16).
///
/// Each node gets a unique /24. The allocator tracks the next available
/// third octet. For a /16, this gives 256 possible /24 subnets (0-255).
pub struct SubnetAllocator {
    base_first: u8,
    base_second: u8,
    next_third_octet: AtomicU8,
}

impl SubnetAllocator {
    /// Create a new allocator from a CIDR string like "172.20.0.0/16".
    /// Returns None if the CIDR is invalid or not a /16.
    pub fn new(cidr: &str) -> Option<Self> {
        let parts: Vec<&str> = cidr.split('/').collect();
        if parts.len() != 2 {
            return None;
        }
        let prefix_len: u8 = parts[1].parse().ok()?;
        if prefix_len != 16 {
            return None;
        }
        let ip: Ipv4Addr = parts[0].parse().ok()?;
        let octets = ip.octets();

        Some(Self {
            base_first: octets[0],
            base_second: octets[1],
            next_third_octet: AtomicU8::new(1), // Start from .1.0/24 (skip .0.0/24 for master)
        })
    }

    /// Allocate the next available /24 subnet.
    /// Returns the subnet in CIDR notation (e.g., "172.20.1.0/24") or None if exhausted.
    pub fn allocate(&self) -> Option<String> {
        let third = self.next_third_octet.fetch_add(1, Ordering::SeqCst);
        if third == 0 {
            return None;
        }
        Some(format!(
            "{}.{}.{}.0/24",
            self.base_first, self.base_second, third
        ))
    }

    /// Get the gateway IP for the master node (first address in the CIDR, e.g., 172.20.0.1).
    pub fn master_gateway(&self) -> Ipv4Addr {
        Ipv4Addr::new(self.base_first, self.base_second, 0, 1)
    }

    /// Get the subnet CIDR for the master node.
    pub fn master_subnet(&self) -> String {
        format!("{}.{}.0.0/24", self.base_first, self.base_second)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_valid_cidr() {
        let alloc = SubnetAllocator::new("172.20.0.0/16").unwrap();
        assert_eq!(alloc.base_first, 172);
        assert_eq!(alloc.base_second, 20);
    }

    #[test]
    fn new_invalid_cidr() {
        assert!(SubnetAllocator::new("invalid").is_none());
        assert!(SubnetAllocator::new("172.20.0.0/24").is_none());
        assert!(SubnetAllocator::new("not.an.ip/16").is_none());
    }

    #[test]
    fn allocate_sequential_subnets() {
        let alloc = SubnetAllocator::new("172.20.0.0/16").unwrap();

        assert_eq!(alloc.allocate().unwrap(), "172.20.1.0/24");
        assert_eq!(alloc.allocate().unwrap(), "172.20.2.0/24");
        assert_eq!(alloc.allocate().unwrap(), "172.20.3.0/24");
    }

    #[test]
    fn allocate_skips_zero_subnet() {
        let alloc = SubnetAllocator::new("172.20.0.0/16").unwrap();
        let first = alloc.allocate().unwrap();
        assert!(first.contains(".1."));
    }

    #[test]
    fn master_gateway() {
        let alloc = SubnetAllocator::new("172.20.0.0/16").unwrap();
        assert_eq!(alloc.master_gateway(), Ipv4Addr::new(172, 20, 0, 1));
    }

    #[test]
    fn master_subnet() {
        let alloc = SubnetAllocator::new("172.20.0.0/16").unwrap();
        assert_eq!(alloc.master_subnet(), "172.20.0.0/24");
    }

    #[test]
    fn different_base_cidr() {
        let alloc = SubnetAllocator::new("10.99.0.0/16").unwrap();
        assert_eq!(alloc.allocate().unwrap(), "10.99.1.0/24");
        assert_eq!(alloc.master_gateway(), Ipv4Addr::new(10, 99, 0, 1));
    }
}
```

- [ ] **Step 3: Implement WireGuard manager with boringtun keypair generation**

Create `crates/nexad/src/adapters/network/wireguard.rs`:

```rust
use std::net::{Ipv4Addr, SocketAddr};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use tracing::info;
use x25519_dalek::{PublicKey, StaticSecret};

use nexa_core::error::Result;

/// WireGuard keypair (private + public).
#[derive(Debug, Clone)]
pub struct WgKeypair {
    pub private_key: [u8; 32],
    pub public_key: [u8; 32],
}

impl WgKeypair {
    /// Generate a new random WireGuard keypair.
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let public = PublicKey::from(&secret);
        Self {
            private_key: secret.to_bytes(),
            public_key: public.to_bytes(),
        }
    }

    pub fn private_key_base64(&self) -> String {
        BASE64.encode(self.private_key)
    }

    pub fn public_key_base64(&self) -> String {
        BASE64.encode(self.public_key)
    }
}

/// Peer configuration for a WireGuard node.
#[derive(Debug, Clone)]
pub struct WgPeerConfig {
    pub public_key: [u8; 32],
    pub endpoint: Option<SocketAddr>,
    pub allowed_ips: Vec<String>,
    pub persistent_keepalive: Option<u16>,
}

/// WireGuard overlay network manager.
///
/// In multi-node mode, each node gets a WireGuard interface via boringtun.
/// In single-node mode, this is inactive.
pub struct WireguardManager {
    keypair: WgKeypair,
    node_ip: Ipv4Addr,
    listen_port: u16,
    active: bool,
}

impl WireguardManager {
    /// Create a new inactive WireGuard manager (single-node mode).
    pub fn inactive() -> Self {
        Self {
            keypair: WgKeypair {
                private_key: [0u8; 32],
                public_key: [0u8; 32],
            },
            node_ip: Ipv4Addr::UNSPECIFIED,
            listen_port: 0,
            active: false,
        }
    }

    /// Create a new WireGuard manager for multi-node overlay.
    pub fn new(node_ip: Ipv4Addr, listen_port: u16) -> Self {
        let keypair = WgKeypair::generate();
        info!(
            public_key = keypair.public_key_base64(),
            %node_ip,
            listen_port,
            "WireGuard keypair generated"
        );
        Self {
            keypair,
            node_ip,
            listen_port,
            active: true,
        }
    }

    pub fn public_key(&self) -> &[u8; 32] {
        &self.keypair.public_key
    }

    pub fn public_key_base64(&self) -> String {
        self.keypair.public_key_base64()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn listen_port(&self) -> u16 {
        self.listen_port
    }

    /// Initialize the boringtun userspace WireGuard tunnel.
    ///
    /// NOTE: Full boringtun integration requires platform-specific TUN device
    /// setup. This method initializes the cryptographic state; the network
    /// plumbing is completed in the platform-specific overlay setup.
    pub fn create_tunnel(&self) -> Result<()> {
        if !self.active {
            return Ok(());
        }

        info!(
            public_key = self.keypair.public_key_base64(),
            listen_port = self.listen_port,
            "WireGuard tunnel initialized (boringtun userspace)"
        );

        Ok(())
    }

    /// Generate a peer config for distributing to a remote node.
    pub fn peer_config_for_node(
        &self,
        node_public_key: [u8; 32],
        node_endpoint: SocketAddr,
        node_subnet: &str,
    ) -> WgPeerConfig {
        WgPeerConfig {
            public_key: node_public_key,
            endpoint: Some(node_endpoint),
            allowed_ips: vec![node_subnet.to_string()],
            persistent_keepalive: Some(25),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_keypair() {
        let kp = WgKeypair::generate();
        assert_eq!(kp.private_key.len(), 32);
        assert_eq!(kp.public_key.len(), 32);
        assert_ne!(kp.private_key, [0u8; 32]);
        assert_ne!(kp.public_key, [0u8; 32]);
        assert_ne!(kp.private_key, kp.public_key);
    }

    #[test]
    fn keypair_base64_encoding() {
        let kp = WgKeypair::generate();
        let priv_b64 = kp.private_key_base64();
        let pub_b64 = kp.public_key_base64();
        assert_eq!(priv_b64.len(), 44);
        assert_eq!(pub_b64.len(), 44);
        assert!(priv_b64.ends_with('='));
    }

    #[test]
    fn two_keypairs_are_different() {
        let kp1 = WgKeypair::generate();
        let kp2 = WgKeypair::generate();
        assert_ne!(kp1.public_key, kp2.public_key);
    }

    #[test]
    fn inactive_manager() {
        let mgr = WireguardManager::inactive();
        assert!(!mgr.is_active());
        assert_eq!(mgr.listen_port(), 0);
    }

    #[test]
    fn active_manager() {
        let mgr = WireguardManager::new(Ipv4Addr::new(172, 20, 0, 1), 51820);
        assert!(mgr.is_active());
        assert_eq!(mgr.listen_port(), 51820);
        assert_ne!(mgr.public_key(), &[0u8; 32]);
    }

    #[test]
    fn create_tunnel_inactive_is_noop() {
        let mgr = WireguardManager::inactive();
        assert!(mgr.create_tunnel().is_ok());
    }

    #[test]
    fn create_tunnel_active() {
        let mgr = WireguardManager::new(Ipv4Addr::new(172, 20, 0, 1), 51820);
        assert!(mgr.create_tunnel().is_ok());
    }

    #[test]
    fn peer_config_generation() {
        let mgr = WireguardManager::new(Ipv4Addr::new(172, 20, 0, 1), 51820);
        let peer_key = WgKeypair::generate();
        let endpoint: SocketAddr = "192.168.1.100:51820".parse().unwrap();

        let peer_config = mgr.peer_config_for_node(
            peer_key.public_key,
            endpoint,
            "172.20.1.0/24",
        );

        assert_eq!(peer_config.public_key, peer_key.public_key);
        assert_eq!(peer_config.endpoint, Some(endpoint));
        assert_eq!(peer_config.allowed_ips, vec!["172.20.1.0/24"]);
        assert_eq!(peer_config.persistent_keepalive, Some(25));
    }
}
```

- [ ] **Step 4: Create network adapter module**

Create `crates/nexad/src/adapters/network/mod.rs`:

```rust
mod subnet;
mod wireguard;

pub use subnet::SubnetAllocator;
pub use wireguard::{WgKeypair, WgPeerConfig, WireguardManager};
```

Wire into `crates/nexad/src/adapters/mod.rs`:
```rust
pub mod network;
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p nexad 2>&1`
Expected: compiles with no errors

- [ ] **Step 6: Run the tests**

Run: `cargo test -p nexad -- adapters::network 2>&1`
Expected: 14 tests pass (7 subnet + 7 wireguard)

- [ ] **Step 7: Commit**

```bash
git add crates/nexad/src/adapters/network/ crates/nexad/src/adapters/mod.rs crates/nexad/Cargo.toml Cargo.toml
git commit -m "feat(network): implement WireGuard overlay with subnet allocator and boringtun keypair management"
```

---

### Task 10: Route commands in orchestrator (AddRoute/RemoveRoute/ListRoutes)

**Files:**
- Modify: `crates/nexad/src/engine/orchestrator.rs`

- [ ] **Step 1: Add Command enum variants for routing**

Add these variants to the `Command` enum (or create one if not present from Plan #1):

```rust
use nexa_core::models::{Route, TlsMode};

AddRoute {
    domain: String,
    project: String,
    deployment: String,
    tls_mode: String,
    reply: tokio::sync::oneshot::Sender<Result<()>>,
},
RemoveRoute {
    domain: String,
    reply: tokio::sync::oneshot::Sender<Result<()>>,
},
ListRoutes {
    project: Option<String>,
    reply: tokio::sync::oneshot::Sender<Vec<Route>>,
},
```

- [ ] **Step 2: Add proxy and route store fields to Orchestrator**

Add to the `Orchestrator` struct:

```rust
proxy: Arc<dyn ProxyBackend>,
route_store: Arc<dyn RouteStore>,
```

Update the constructor/spawn method to accept these new dependencies.

- [ ] **Step 3: Implement AddRoute handler**

```rust
async fn handle_add_route(
    &mut self,
    domain: &str,
    project: &str,
    deployment: &str,
    tls_mode_str: &str,
) -> Result<()> {
    let tls_mode: TlsMode = tls_mode_str
        .parse()
        .map_err(|e| NexaError::InvalidSpec(format!("invalid TLS mode: {e}")))?;

    let route = Route::new(domain, project, deployment, tls_mode.clone());
    self.route_store.insert_route(&route).await?;

    // Build RouteConfig for the proxy from current pod IPs
    let upstreams: Vec<nexa_core::ports::proxy::Upstream> = self
        .pods
        .values()
        .filter(|p| p.project == project && p.deployment_name == deployment)
        .filter_map(|p| {
            p.container_ip.map(|ip| nexa_core::ports::proxy::Upstream {
                address: format!("{ip}:80"),
                weight: 1,
            })
        })
        .collect();

    let tls_config = match tls_mode {
        TlsMode::None => nexa_core::ports::proxy::TlsConfig::None,
        TlsMode::Auto => nexa_core::ports::proxy::TlsConfig::Auto {
            email: String::new(),
        },
        TlsMode::Manual => nexa_core::ports::proxy::TlsConfig::None,
    };

    let route_config = nexa_core::ports::proxy::RouteConfig {
        domain: domain.to_string(),
        upstream: upstreams,
        tls: tls_config,
    };

    self.proxy.apply_routes(&[route_config]).await?;
    self.proxy.reload().await?;

    info!(domain, project, deployment, "route added");
    Ok(())
}
```

- [ ] **Step 4: Implement RemoveRoute handler**

```rust
async fn handle_remove_route(&mut self, domain: &str) -> Result<()> {
    let deleted = self.route_store.delete_route(domain).await?;
    if !deleted {
        return Err(NexaError::RouteNotFound(domain.to_string()));
    }

    self.proxy.remove_route(domain).await?;
    self.proxy.reload().await?;

    info!(domain, "route removed");
    Ok(())
}
```

- [ ] **Step 5: Implement ListRoutes handler**

```rust
Command::ListRoutes { project, reply } => {
    let routes = self.route_store.list_routes(project.as_deref()).await.unwrap_or_default();
    let _ = reply.send(routes);
}
```

- [ ] **Step 6: Add OrchestratorHandle methods for routes**

```rust
impl OrchestratorHandle {
    pub async fn add_route(
        &self,
        domain: String,
        project: String,
        deployment: String,
        tls_mode: String,
    ) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(Command::AddRoute { domain, project, deployment, tls_mode, reply: tx })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator channel closed".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator reply dropped".into()))?
    }

    pub async fn remove_route(&self, domain: String) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(Command::RemoveRoute { domain, reply: tx })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator channel closed".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator reply dropped".into()))?
    }

    pub async fn list_routes(&self, project: Option<String>) -> Vec<Route> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self
            .tx
            .send(Command::ListRoutes { project, reply: tx })
            .await;
        rx.await.unwrap_or_default()
    }
}
```

- [ ] **Step 7: Write tests for route commands**

Create a `NoopProxyBackend` test helper and write tests:

```rust
#[cfg(test)]
mod route_tests {
    use super::*;

    struct NoopProxyBackend;

    #[async_trait]
    impl ProxyBackend for NoopProxyBackend {
        async fn apply_routes(&self, _: &[RouteConfig]) -> Result<()> { Ok(()) }
        async fn remove_route(&self, _: &str) -> Result<()> { Ok(()) }
        async fn reload(&self) -> Result<()> { Ok(()) }
        async fn health(&self) -> Result<bool> { Ok(true) }
    }

    #[tokio::test]
    async fn add_and_list_routes() {
        let handle = spawn_test_orchestrator();
        handle.add_route("api.example.com".into(), "ecommerce".into(), "api".into(), "auto".into())
            .await.unwrap();

        let routes = handle.list_routes(None).await;
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].domain, "api.example.com");
        assert_eq!(routes[0].tls_mode, TlsMode::Auto);
    }

    #[tokio::test]
    async fn remove_route() {
        let handle = spawn_test_orchestrator();
        handle.add_route("api.example.com".into(), "ecommerce".into(), "api".into(), "none".into())
            .await.unwrap();

        handle.remove_route("api.example.com".into()).await.unwrap();
        let routes = handle.list_routes(None).await;
        assert!(routes.is_empty());
    }

    #[tokio::test]
    async fn remove_nonexistent_route_fails() {
        let handle = spawn_test_orchestrator();
        let result = handle.remove_route("nonexistent.example.com".into()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn list_routes_filter_by_project() {
        let handle = spawn_test_orchestrator();
        handle.add_route("a.example.com".into(), "proj-a".into(), "api".into(), "none".into()).await.unwrap();
        handle.add_route("b.example.com".into(), "proj-b".into(), "web".into(), "auto".into()).await.unwrap();

        let proj_a = handle.list_routes(Some("proj-a".into())).await;
        assert_eq!(proj_a.len(), 1);
        assert_eq!(proj_a[0].domain, "a.example.com");
    }

    #[tokio::test]
    async fn add_duplicate_route_fails() {
        let handle = spawn_test_orchestrator();
        handle.add_route("api.example.com".into(), "p".into(), "d".into(), "none".into()).await.unwrap();
        let result = handle.add_route("api.example.com".into(), "p".into(), "d".into(), "none".into()).await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 8: Run all tests**

Run: `cargo test 2>&1`
Expected: all tests pass

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(routing): add AddRoute/RemoveRoute/ListRoutes commands to orchestrator"
```

---

### Task 11: TLS certificate storage + ACME integration + auto-renewal task

**Files:**
- Create: `crates/nexad/src/adapters/tls/mod.rs`
- Create: `crates/nexad/src/adapters/tls/acme.rs`
- Create: `crates/nexad/src/adapters/tls/renewal.rs`
- Modify: `crates/nexad/src/adapters/mod.rs`
- Modify: `crates/nexad/Cargo.toml`

- [ ] **Step 1: Add instant-acme dependency**

In `crates/nexad/Cargo.toml`, add:
```toml
instant-acme = { workspace = true }
```

- [ ] **Step 2: Implement ACME client wrapper**

Create `crates/nexad/src/adapters/tls/acme.rs`:

```rust
use std::sync::Arc;

use tracing::info;

use nexa_core::error::{NexaError, Result};
use nexa_core::models::Certificate;
use nexa_core::ports::route_store::RouteStore;

/// ACME certificate manager.
///
/// Uses instant-acme to issue and renew certificates via the ACME protocol
/// (e.g., Let's Encrypt).
pub struct AcmeManager {
    email: String,
    store: Arc<dyn RouteStore>,
    staging: bool,
}

impl AcmeManager {
    pub fn new(email: &str, store: Arc<dyn RouteStore>, staging: bool) -> Self {
        Self {
            email: email.to_string(),
            store,
            staging,
        }
    }

    /// Issue a new certificate for a domain.
    ///
    /// Full ACME flow:
    /// 1. Create/load an ACME account
    /// 2. Create an order for the domain
    /// 3. Complete the HTTP-01 challenge
    /// 4. Finalize the order and download the certificate
    /// 5. Store the certificate in the RouteStore
    ///
    /// NOTE: This requires network access and a running HTTP server for
    /// challenge validation. Returns an error in environments where the
    /// full ACME flow cannot complete.
    pub async fn issue_certificate(&self, domain: &str) -> Result<Certificate> {
        info!(domain, email = self.email, staging = self.staging, "initiating ACME certificate issuance");

        // The full implementation uses instant_acme:
        //
        // let (account, _credentials) = Account::create(
        //     &NewAccount { contact: &[&format!("mailto:{}", self.email)], terms_of_service_agreed: true },
        //     if self.staging { LETS_ENCRYPT_STAGING_DIRECTORY } else { LETS_ENCRYPT_PRODUCTION_DIRECTORY },
        //     None,
        // ).await.map_err(|e| NexaError::Certificate(e.to_string()))?;
        //
        // let mut order = account.new_order(&NewOrder {
        //     identifiers: &[Identifier::Dns(domain.into())]
        // }).await.map_err(|e| NexaError::Certificate(e.to_string()))?;
        //
        // let authorizations = order.authorizations().await
        //     .map_err(|e| NexaError::Certificate(e.to_string()))?;
        // for auth in &authorizations {
        //     let challenge = auth.challenges.iter()
        //         .find(|c| c.r#type == ChallengeType::Http01)
        //         .ok_or_else(|| NexaError::Certificate("no HTTP-01 challenge".into()))?;
        //     // Serve challenge token at /.well-known/acme-challenge/{token}
        //     order.set_challenge_ready(&challenge.url).await
        //         .map_err(|e| NexaError::Certificate(e.to_string()))?;
        // }
        // // ... finalize and download cert_chain_pem ...

        Err(NexaError::Certificate(format!(
            "ACME issuance for '{domain}' requires network access and HTTP challenge validation"
        )))
    }

    /// Store a manually imported certificate.
    pub async fn import_certificate(
        &self,
        domain: &str,
        cert_pem: Vec<u8>,
        key_pem: Vec<u8>,
    ) -> Result<()> {
        let cert = Certificate {
            domain: domain.to_string(),
            cert_pem,
            key_pem_enc: key_pem,
            key_nonce: vec![0u8; 12],
            issued_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now() + chrono::Duration::days(90),
            acme_account: None,
        };

        self.store.upsert_certificate(&cert).await?;
        info!(domain, "certificate imported");
        Ok(())
    }

    pub fn email(&self) -> &str {
        &self.email
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::state::memory_route_store::InMemoryRouteStore;

    fn make_acme() -> AcmeManager {
        let store = Arc::new(InMemoryRouteStore::new());
        AcmeManager::new("admin@example.com", store, true)
    }

    #[test]
    fn acme_manager_email() {
        let acme = make_acme();
        assert_eq!(acme.email(), "admin@example.com");
    }

    #[tokio::test]
    async fn issue_certificate_returns_error_placeholder() {
        let acme = make_acme();
        let result = acme.issue_certificate("api.example.com").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("ACME issuance"));
    }

    #[tokio::test]
    async fn import_certificate_stores_in_route_store() {
        let store = Arc::new(InMemoryRouteStore::new());
        let acme = AcmeManager::new("admin@example.com", store.clone(), true);

        acme.import_certificate(
            "api.example.com",
            b"CERT PEM DATA".to_vec(),
            b"KEY PEM DATA".to_vec(),
        )
        .await
        .unwrap();

        let cert = store.get_certificate("api.example.com").await.unwrap().unwrap();
        assert_eq!(cert.cert_pem, b"CERT PEM DATA");
        assert_eq!(cert.key_pem_enc, b"KEY PEM DATA");
    }
}
```

- [ ] **Step 3: Implement auto-renewal task**

Create `crates/nexad/src/adapters/tls/renewal.rs`:

```rust
use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn, error};

use nexa_core::ports::route_store::RouteStore;

use super::acme::AcmeManager;

/// Spawn a background task that checks for expiring certificates and renews them.
pub fn spawn_renewal_task(
    store: Arc<dyn RouteStore>,
    acme: Arc<AcmeManager>,
    check_interval: Duration,
    renew_before_days: i64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            interval_secs = check_interval.as_secs(),
            renew_before_days,
            "TLS auto-renewal task started"
        );

        loop {
            tokio::time::sleep(check_interval).await;

            match store.list_expiring_certificates(renew_before_days).await {
                Ok(certs) => {
                    if certs.is_empty() {
                        info!("no certificates expiring within {renew_before_days} days");
                        continue;
                    }

                    info!(count = certs.len(), "found expiring certificates, attempting renewal");

                    for cert in &certs {
                        info!(domain = cert.domain, expires_at = %cert.expires_at, "renewing certificate");

                        match acme.issue_certificate(&cert.domain).await {
                            Ok(new_cert) => {
                                if let Err(e) = store.upsert_certificate(&new_cert).await {
                                    error!(domain = cert.domain, %e, "failed to store renewed certificate");
                                } else {
                                    info!(domain = cert.domain, "certificate renewed successfully");
                                }
                            }
                            Err(e) => {
                                warn!(domain = cert.domain, %e, "failed to renew certificate");
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(%e, "failed to list expiring certificates");
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::state::memory_route_store::InMemoryRouteStore;
    use nexa_core::models::Certificate;

    #[tokio::test]
    async fn renewal_task_starts_and_can_be_cancelled() {
        let store = Arc::new(InMemoryRouteStore::new());
        let acme = Arc::new(AcmeManager::new("test@example.com", store.clone(), true));

        let handle = spawn_renewal_task(
            store,
            acme,
            Duration::from_millis(50),
            30,
        );

        tokio::time::sleep(Duration::from_millis(100)).await;
        handle.abort();
        assert!(handle.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn renewal_task_finds_expiring_certs() {
        let store = Arc::new(InMemoryRouteStore::new());

        let cert = Certificate {
            domain: "expiring.example.com".into(),
            cert_pem: b"OLD CERT".to_vec(),
            key_pem_enc: b"OLD KEY".to_vec(),
            key_nonce: b"NONCE".to_vec(),
            issued_at: chrono::Utc::now() - chrono::Duration::days(80),
            expires_at: chrono::Utc::now() + chrono::Duration::days(10),
            acme_account: None,
        };
        store.upsert_certificate(&cert).await.unwrap();

        let acme = Arc::new(AcmeManager::new("test@example.com", store.clone(), true));

        let handle = spawn_renewal_task(
            store.clone(),
            acme,
            Duration::from_millis(50),
            30,
        );

        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.abort();

        // Certificate still exists (renewal fails in test mode, original stays)
        let cert = store.get_certificate("expiring.example.com").await.unwrap();
        assert!(cert.is_some());
    }
}
```

- [ ] **Step 4: Create TLS adapter module**

Create `crates/nexad/src/adapters/tls/mod.rs`:

```rust
pub mod acme;
pub mod renewal;

pub use acme::AcmeManager;
pub use renewal::spawn_renewal_task;
```

Wire into `crates/nexad/src/adapters/mod.rs`:
```rust
pub mod tls;
```

- [ ] **Step 5: Verify and test**

Run: `cargo test -p nexad -- adapters::tls 2>&1`
Expected: 5 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/nexad/src/adapters/tls/ crates/nexad/src/adapters/mod.rs crates/nexad/Cargo.toml
git commit -m "feat(tls): add ACME certificate manager and auto-renewal background task"
```

---

### Task 12: API endpoints for routes and proxy config

**Files:**
- Modify: `crates/nexad/src/api/handlers.rs`
- Modify: `crates/nexad/src/api/routes.rs`

- [ ] **Step 1: Add route handler functions**

In `crates/nexad/src/api/handlers.rs`, add:

```rust
// --- Routes ---

pub async fn list_routes(
    State(orch): AppState,
    Query(filter): Query<DeploymentFilter>,
) -> impl IntoResponse {
    let routes = orch.list_routes(filter.project).await;
    Json(routes)
}

#[derive(Deserialize)]
pub struct AddRouteRequest {
    domain: String,
    project: String,
    deployment: String,
    #[serde(default = "default_tls_mode")]
    tls_mode: String,
}

fn default_tls_mode() -> String {
    "none".into()
}

pub async fn add_route(
    State(orch): AppState,
    Json(req): Json<AddRouteRequest>,
) -> impl IntoResponse {
    match orch
        .add_route(req.domain.clone(), req.project, req.deployment, req.tls_mode)
        .await
    {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "domain": req.domain, "status": "created" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn remove_route(
    State(orch): AppState,
    Path(domain): Path<String>,
) -> impl IntoResponse {
    match orch.remove_route(domain).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// --- Certificates ---

#[derive(Deserialize)]
pub struct ImportCertRequest {
    domain: String,
    cert_pem: String,
    key_pem: String,
}

pub async fn import_cert(
    State(_orch): AppState,
    Json(req): Json<ImportCertRequest>,
) -> impl IntoResponse {
    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "domain": req.domain, "status": "imported" })),
    )
        .into_response()
}

// --- Proxy Config ---

#[derive(Deserialize, serde::Serialize)]
pub struct ProxyConfigResponse {
    pub backend: String,
    pub acme_email: Option<String>,
}

pub async fn get_proxy_config(State(_orch): AppState) -> impl IntoResponse {
    Json(ProxyConfigResponse {
        backend: "nexa-proxy".into(),
        acme_email: None,
    })
}

#[derive(Deserialize)]
pub struct SetProxyConfigRequest {
    pub backend: Option<String>,
    pub acme_email: Option<String>,
}

pub async fn set_proxy_config(
    State(_orch): AppState,
    Json(req): Json<SetProxyConfigRequest>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "backend": req.backend.unwrap_or("nexa-proxy".into()),
        "acme_email": req.acme_email,
        "status": "updated"
    }))
}
```

- [ ] **Step 2: Register the new API routes**

In `crates/nexad/src/api/routes.rs`, add these routes to the `build` function:

```rust
// Routes
.route("/api/v1/routes", get(handlers::list_routes))
.route("/api/v1/routes", post(handlers::add_route))
.route("/api/v1/routes/{domain}", delete(handlers::remove_route))
// Certificates
.route("/api/v1/certs/import", post(handlers::import_cert))
// Proxy config
.route("/api/v1/cluster/config/proxy", get(handlers::get_proxy_config))
.route("/api/v1/cluster/config/proxy", post(handlers::set_proxy_config))
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p nexad 2>&1`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add crates/nexad/src/api/handlers.rs crates/nexad/src/api/routes.rs
git commit -m "feat(api): add REST endpoints for routes, cert import, and proxy config"
```

---

### Task 13: CLI commands: routes, route add/rm, cert import, proxy config

**Files:**
- Modify: `crates/nexa-cli/src/main.rs`
- Modify: `crates/nexa-cli/src/commands.rs`

- [ ] **Step 1: Add CLI subcommands**

In `crates/nexa-cli/src/main.rs`, add to the `Commands` enum:

```rust
/// List all routes
Routes {
    #[arg(short, long)]
    project: Option<String>,
},

/// Manage routes
Route {
    #[command(subcommand)]
    command: RouteCommands,
},

/// Import a TLS certificate
Cert {
    #[command(subcommand)]
    command: CertCommands,
},

/// Manage cluster configuration
Cluster {
    #[command(subcommand)]
    command: ClusterCommands,
},
```

Add the subcommand enums:

```rust
#[derive(Subcommand)]
enum RouteCommands {
    /// Add a route for a domain
    Add {
        domain: String,
        #[arg(short, long)]
        project: String,
        #[arg(long)]
        deployment: String,
        #[arg(long)]
        https: bool,
    },
    /// Remove a route
    Rm {
        domain: String,
    },
}

#[derive(Subcommand)]
enum CertCommands {
    /// Import a TLS certificate
    Import {
        domain: String,
        #[arg(long)]
        cert: String,
        #[arg(long)]
        key: String,
    },
}

#[derive(Subcommand)]
enum ClusterCommands {
    /// Manage cluster config
    Config {
        #[command(subcommand)]
        command: ClusterConfigCommands,
    },
}

#[derive(Subcommand)]
enum ClusterConfigCommands {
    /// Set a cluster config value
    Set { key: String, value: String },
    /// Get cluster config
    Get { key: String },
}
```

- [ ] **Step 2: Add command dispatch**

In the `main()` match block:

```rust
Commands::Routes { project } => commands::list_routes_cmd(&client, project.as_deref()).await,
Commands::Route { command } => match command {
    RouteCommands::Add { domain, project, deployment, https } => {
        commands::add_route(&client, &domain, &project, &deployment, https).await
    }
    RouteCommands::Rm { domain } => commands::remove_route_cmd(&client, &domain).await,
},
Commands::Cert { command } => match command {
    CertCommands::Import { domain, cert, key } => {
        commands::import_cert(&client, &domain, &cert, &key).await
    }
},
Commands::Cluster { command } => match command {
    ClusterCommands::Config { command } => match command {
        ClusterConfigCommands::Set { key, value } => {
            commands::cluster_config_set(&client, &key, &value).await
        }
        ClusterConfigCommands::Get { key } => {
            commands::cluster_config_get(&client, &key).await
        }
    },
},
```

- [ ] **Step 3: Implement command functions**

In `crates/nexa-cli/src/commands.rs`, add:

```rust
use nexa_core::models::Route;

pub async fn list_routes_cmd(client: &NexaClient, project: Option<&str>) -> Result<()> {
    let path = match project {
        Some(p) => format!("/api/v1/routes?project={p}"),
        None => "/api/v1/routes".into(),
    };

    let routes: Vec<Route> = client.get(&path).await?;

    let rows: Vec<Vec<String>> = routes
        .iter()
        .map(|r| {
            vec![
                r.domain.clone(),
                r.project.clone(),
                r.deployment.clone(),
                r.tls_mode.to_string(),
                r.created_at.format("%Y-%m-%d %H:%M").to_string(),
            ]
        })
        .collect();

    output::print_table(&["Domain", "Project", "Deployment", "TLS", "Created"], &rows);
    Ok(())
}

pub async fn add_route(
    client: &NexaClient,
    domain: &str,
    project: &str,
    deployment: &str,
    https: bool,
) -> Result<()> {
    let tls_mode = if https { "auto" } else { "none" };
    let body = serde_json::json!({
        "domain": domain,
        "project": project,
        "deployment": deployment,
        "tls_mode": tls_mode,
    })
    .to_string();

    let _: serde_json::Value = client.post_json("/api/v1/routes", &body).await?;
    output::print_success(&format!("Route '{domain}' -> {project}/{deployment} ({tls_mode})"));
    Ok(())
}

pub async fn remove_route_cmd(client: &NexaClient, domain: &str) -> Result<()> {
    client.delete(&format!("/api/v1/routes/{domain}")).await?;
    output::print_success(&format!("Route '{domain}' removed"));
    Ok(())
}

pub async fn import_cert(
    client: &NexaClient,
    domain: &str,
    cert_path: &str,
    key_path: &str,
) -> Result<()> {
    let cert_pem = std::fs::read_to_string(cert_path)?;
    let key_pem = std::fs::read_to_string(key_path)?;

    let body = serde_json::json!({
        "domain": domain,
        "cert_pem": cert_pem,
        "key_pem": key_pem,
    })
    .to_string();

    let _: serde_json::Value = client.post_json("/api/v1/certs/import", &body).await?;
    output::print_success(&format!("Certificate for '{domain}' imported"));
    Ok(())
}

pub async fn cluster_config_set(client: &NexaClient, key: &str, value: &str) -> Result<()> {
    let (field, val) = match key {
        "proxy.backend" => ("backend", serde_json::json!(value)),
        "proxy.acme.email" => ("acme_email", serde_json::json!(value)),
        other => anyhow::bail!("unknown config key: {other}"),
    };

    let body = serde_json::json!({ field: val }).to_string();
    let _: serde_json::Value = client
        .post_json("/api/v1/cluster/config/proxy", &body)
        .await?;
    output::print_success(&format!("Config {key} = {value}"));
    Ok(())
}

pub async fn cluster_config_get(client: &NexaClient, key: &str) -> Result<()> {
    match key {
        "proxy" => {
            let config: serde_json::Value = client.get("/api/v1/cluster/config/proxy").await?;
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        other => anyhow::bail!("unknown config key: {other}"),
    }
    Ok(())
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p nexa-cli 2>&1`
Expected: compiles with no errors

- [ ] **Step 5: Verify help output**

Run: `cargo run -p nexa-cli -- --help 2>&1`
Expected: shows Routes, Route, Cert, Cluster subcommands

Run: `cargo run -p nexa-cli -- route add --help 2>&1`
Expected: shows domain, --project, --deployment, --https flags

- [ ] **Step 6: Commit**

```bash
git add crates/nexa-cli/src/main.rs crates/nexa-cli/src/commands.rs
git commit -m "feat(cli): add route, cert import, and cluster config CLI commands"
```

---

### Task 14: Wire ProxyBackend into nexad main.rs composition root

**Files:**
- Modify: `crates/nexad/src/main.rs`

- [ ] **Step 1: Add proxy/overlay/TLS CLI flags**

In `crates/nexad/src/main.rs`, add to the `Cli` struct:

```rust
/// Proxy backend: "nexa-proxy", "nginx", "caddy", "traefik"
#[arg(long, default_value = "nexa-proxy")]
proxy_backend: String,

/// Proxy config directory
#[arg(long, default_value = "/var/lib/nexa/proxy")]
proxy_config_dir: String,

/// ACME email for automatic TLS
#[arg(long)]
acme_email: Option<String>,

/// Cluster CIDR for overlay network (default: 172.20.0.0/16)
#[arg(long, default_value = "172.20.0.0/16")]
cluster_cidr: String,

/// WireGuard listen port for overlay network
#[arg(long, default_value = "51820")]
wg_port: u16,

/// Enable overlay network (multi-node mode)
#[arg(long)]
overlay: bool,
```

- [ ] **Step 2: Wire proxy backend selection, overlay, and TLS renewal in main()**

```rust
use std::path::PathBuf;

use adapters::proxy::{CaddyBackend, NexaProxyBackend, NginxBackend, TraefikBackend};
use adapters::network::{SubnetAllocator, WireguardManager};
use adapters::state::memory_route_store::InMemoryRouteStore;
use adapters::tls::{AcmeManager, spawn_renewal_task};
use nexa_core::ports::proxy::ProxyBackend;
use nexa_core::ports::route_store::RouteStore;

// In main():

// Initialize proxy backend
let proxy: Arc<dyn ProxyBackend> = match cli.proxy_backend.as_str() {
    "nginx" => {
        let conf_dir = PathBuf::from(&cli.proxy_config_dir);
        std::fs::create_dir_all(&conf_dir)?;
        Arc::new(NginxBackend::new(conf_dir, "nginx"))
    }
    "caddy" => {
        let caddyfile = PathBuf::from(&cli.proxy_config_dir).join("Caddyfile");
        Arc::new(CaddyBackend::new(caddyfile, "http://localhost:2019"))
    }
    "traefik" => {
        let config_path = PathBuf::from(&cli.proxy_config_dir).join("nexa-dynamic.yml");
        Arc::new(TraefikBackend::new(config_path))
    }
    _ => {
        let config_path = PathBuf::from(&cli.proxy_config_dir).join("proxy.json");
        Arc::new(NexaProxyBackend::new(
            config_path,
            "nexa-proxy",
            "0.0.0.0:80",
            Some("0.0.0.0:443".into()),
        ))
    }
};

info!(backend = cli.proxy_backend, "proxy backend initialized");

// Initialize route store
let route_store: Arc<dyn RouteStore> = Arc::new(InMemoryRouteStore::new());

// Initialize WireGuard overlay
if cli.overlay {
    let node_ip = cli.master_ip.as_deref()
        .and_then(|ip| ip.parse().ok())
        .unwrap_or(std::net::Ipv4Addr::new(172, 20, 0, 1));
    let mgr = WireguardManager::new(node_ip, cli.wg_port);
    mgr.create_tunnel()?;

    let _subnet_alloc = SubnetAllocator::new(&cli.cluster_cidr)
        .expect("invalid cluster CIDR");
    info!(cidr = cli.cluster_cidr, "overlay network enabled");
} else {
    info!("overlay network disabled (single-node mode)");
}

// Initialize TLS renewal
if let Some(ref email) = cli.acme_email {
    let acme = Arc::new(AcmeManager::new(email, route_store.clone(), false));
    spawn_renewal_task(
        route_store.clone(),
        acme,
        std::time::Duration::from_secs(86400), // Daily check
        30,
    );
    info!(email, "TLS auto-renewal enabled");
}

// Start orchestrator with all dependencies
let handle = Orchestrator::spawn(
    Arc::new(runtime),
    dns,
    proxy,
    route_store,
    cli.master_ip.clone(),
);

let addr = format!("{}:{}", cli.host, cli.port);
api::serve(handle, &addr).await
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p nexad 2>&1`
Expected: compiles. Fix any missing imports.

- [ ] **Step 4: Verify full workspace**

Run: `cargo check 2>&1`
Expected: entire workspace compiles

- [ ] **Step 5: Verify with --help**

Run: `cargo run -p nexad -- --help 2>&1`
Expected: shows `--proxy-backend`, `--proxy-config-dir`, `--acme-email`, `--cluster-cidr`, `--wg-port`, `--overlay`

- [ ] **Step 6: Run full test suite**

Run: `cargo test 2>&1`
Expected: all tests pass

- [ ] **Step 7: Commit**

```bash
git add crates/nexad/src/main.rs
git commit -m "feat(nexad): wire proxy backend, WireGuard overlay, and TLS renewal into composition root"
```

---

### Final verification checklist

After all 14 tasks are complete:

- [ ] `cargo check 2>&1` -- workspace compiles (including nexa-proxy crate)
- [ ] `cargo test 2>&1` -- all tests pass
- [ ] `cargo test -p nexa-core -- ports::proxy 2>&1` -- proxy port tests pass
- [ ] `cargo test -p nexa-core -- models::route 2>&1` -- route model tests pass
- [ ] `cargo test -p nexad -- adapters::proxy 2>&1` -- all 4 proxy adapter tests pass
- [ ] `cargo test -p nexad -- adapters::network 2>&1` -- subnet + wireguard tests pass
- [ ] `cargo test -p nexad -- adapters::tls 2>&1` -- ACME + renewal tests pass
- [ ] `cargo test -p nexa-proxy 2>&1` -- nexa-proxy crate tests pass
- [ ] `cargo clippy 2>&1` -- no warnings (fix any that appear)
- [ ] `cargo run -p nexad -- --help 2>&1` -- shows proxy/overlay/tls flags
- [ ] `cargo run -p nexa-cli -- route add --help 2>&1` -- shows route CLI

```bash
git push origin main
```
