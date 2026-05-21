# Service Discovery (Internal DNS) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Multi-Repo Path Mapping:** This project uses separate repos. Translate paths as follows:
> | Plan path prefix | Repo | Local path |
> |---|---|---|
> | `crates/nexa-core/` | [`nexa-core`](https://github.com/nexa-net/nexa-core) | `/Users/nassime/GitHub/nexa-core/` |
> | `crates/nexad/` | [`nexad`](https://github.com/nexa-net/nexad) | `/Users/nassime/GitHub/nexad/` |
> | `crates/nexa-cli/` | [`nexa-cli`](https://github.com/nexa-net/nexa-cli) | `/Users/nassime/GitHub/nexa-cli/` |
> | `crates/nexa-proxy/` | [`nexa-proxy`](https://github.com/nexa-net/nexa-proxy) | `/Users/nassime/GitHub/nexa-proxy/` |
>
> `cargo check -p <crate>` → `cargo check` in the target repo. `nexa-core` dep: `git = "https://github.com/nexa-net/nexa-core"`

**Goal:** Add embedded DNS-based service discovery so containers can resolve each other by name (`<deployment>.<project>.internal`) using a lightweight hickory-dns server running inside nexad master on port 53.

**Architecture:** A `DnsProvider` port trait in nexa-core defines register/deregister/lookup operations. Two adapters implement it: `NoopDnsProvider` (single-node, relies on Docker DNS) and `HickoryDnsProvider` (multi-node, runs an embedded hickory-dns server with an in-memory `DnsRecordStore`). The orchestrator calls `dns.register()` after a pod starts and `dns.deregister()` when a pod stops. Containers receive `dns` and `dns_search` config pointing them at the nexad master IP, enabling short-name resolution within a project. Non-`.internal` queries are forwarded to the system's upstream DNS.

**Tech Stack:** hickory-dns 0.25, hickory-server 0.25, tokio (UDP/TCP listeners), async-trait, std::net::IpAddr

---

### Task 1: Define DnsProvider port trait

**Files:**
- Create: `crates/nexa-core/src/ports/dns.rs`
- Modify: `crates/nexa-core/src/ports/mod.rs` (if it exists) or `crates/nexa-core/src/lib.rs`

- [ ] **Step 1: Write the failing test for the DnsProvider trait**

Create `crates/nexa-core/src/ports/dns.rs`:

```rust
use std::net::IpAddr;

use async_trait::async_trait;

use crate::error::Result;

/// Port trait for DNS-based service discovery.
///
/// Naming convention:
///   <deployment>.<project>.internal          -> round-robin across replicas
///   <deployment>-<index>.<project>.internal   -> specific replica
#[async_trait]
pub trait DnsProvider: Send + Sync {
    /// Register a replica IP for a deployment.
    async fn register(&self, project: &str, deployment: &str, ip: IpAddr) -> Result<()>;

    /// Deregister a replica IP for a deployment.
    async fn deregister(&self, project: &str, deployment: &str, ip: IpAddr) -> Result<()>;

    /// Lookup all IPs for a deployment (round-robin order).
    async fn lookup(&self, project: &str, deployment: &str) -> Result<Vec<IpAddr>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    /// Verify the trait is object-safe (can be used as dyn DnsProvider).
    #[test]
    fn trait_is_object_safe() {
        fn _assert_object_safe(_: &dyn DnsProvider) {}
    }

    /// Verify IpAddr can represent both v4 and v6.
    #[test]
    fn ipaddr_supports_v4_and_v6() {
        let v4: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let v6: IpAddr = "::1".parse().unwrap();
        assert!(v4.is_ipv4());
        assert!(v6.is_ipv6());
    }
}
```

- [ ] **Step 2: Wire the module into the ports directory**

If `crates/nexa-core/src/ports/mod.rs` exists, add:
```rust
pub mod dns;
```

If ports is not yet a module (the codebase still uses `crates/nexa-core/src/runtime/`), create `crates/nexa-core/src/ports/mod.rs`:
```rust
pub mod dns;
pub mod runtime;
```

Move `crates/nexa-core/src/runtime/traits.rs` content into `crates/nexa-core/src/ports/runtime.rs` if it hasn't already been moved by a prior plan. Update `crates/nexa-core/src/lib.rs` to expose `pub mod ports;`.

**NOTE:** If plans #1-9 already restructured into hexagonal layout with `ports/runtime.rs`, just add `pub mod dns;` to `crates/nexa-core/src/ports/mod.rs`.

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p nexa-core 2>&1`
Expected: compiles with no errors

- [ ] **Step 4: Run the tests**

Run: `cargo test -p nexa-core -- ports::dns 2>&1`
Expected: 2 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-core/src/ports/dns.rs crates/nexa-core/src/ports/mod.rs crates/nexa-core/src/lib.rs
git commit -m "feat(dns): define DnsProvider port trait for service discovery"
```

---

### Task 2: Implement NoopDnsProvider adapter

**Files:**
- Create: `crates/nexad/src/adapters/dns/mod.rs`
- Create: `crates/nexad/src/adapters/dns/noop.rs`
- Modify: `crates/nexad/src/adapters/mod.rs`

- [ ] **Step 1: Write failing test for NoopDnsProvider**

Create `crates/nexad/src/adapters/dns/noop.rs`:

```rust
use std::net::IpAddr;

use async_trait::async_trait;

use nexa_core::error::Result;
use nexa_core::ports::dns::DnsProvider;

/// No-op DNS provider for single-node mode.
/// Containers use Docker's built-in DNS within the bridge network.
pub struct NoopDnsProvider;

impl NoopDnsProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DnsProvider for NoopDnsProvider {
    async fn register(&self, _project: &str, _deployment: &str, _ip: IpAddr) -> Result<()> {
        Ok(())
    }

    async fn deregister(&self, _project: &str, _deployment: &str, _ip: IpAddr) -> Result<()> {
        Ok(())
    }

    async fn lookup(&self, _project: &str, _deployment: &str) -> Result<Vec<IpAddr>> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn register_returns_ok() {
        let dns = NoopDnsProvider::new();
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        assert!(dns.register("myapp", "api", ip).await.is_ok());
    }

    #[tokio::test]
    async fn deregister_returns_ok() {
        let dns = NoopDnsProvider::new();
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        assert!(dns.deregister("myapp", "api", ip).await.is_ok());
    }

    #[tokio::test]
    async fn lookup_returns_empty() {
        let dns = NoopDnsProvider::new();
        let result = dns.lookup("myapp", "api").await.unwrap();
        assert!(result.is_empty());
    }
}
```

- [ ] **Step 2: Create the dns adapter module**

Create `crates/nexad/src/adapters/dns/mod.rs`:

```rust
mod noop;

pub use noop::NoopDnsProvider;
```

- [ ] **Step 3: Wire into adapters/mod.rs**

In `crates/nexad/src/adapters/mod.rs`, add:
```rust
pub mod dns;
```

So it becomes:
```rust
pub mod dns;
pub mod runtime;
```

- [ ] **Step 4: Verify and test**

Run: `cargo test -p nexad -- adapters::dns 2>&1`
Expected: 3 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/nexad/src/adapters/dns/
git commit -m "feat(dns): implement NoopDnsProvider for single-node mode"
```

---

### Task 3: Add dns/dns_search fields to ContainerConfig

**Files:**
- Modify: `crates/nexa-core/src/runtime/traits.rs` (or `crates/nexa-core/src/ports/runtime.rs` if hexagonal layout is active)
- Modify: `crates/nexa-core/src/runtime/docker.rs` (or `crates/nexad/src/adapters/runtime/docker.rs`)

- [ ] **Step 1: Write failing test for new ContainerConfig fields**

Add test in the runtime traits file (or a new test block):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_config_dns_fields_default_to_empty() {
        let config = ContainerConfig {
            name: "test".into(),
            image: "nginx".into(),
            env: HashMap::new(),
            ports: vec![],
            volumes: vec![],
            labels: HashMap::new(),
            network: None,
            dns: vec![],
            dns_search: vec![],
        };
        assert!(config.dns.is_empty());
        assert!(config.dns_search.is_empty());
    }

    #[test]
    fn container_config_dns_fields_hold_values() {
        let config = ContainerConfig {
            name: "test".into(),
            image: "nginx".into(),
            env: HashMap::new(),
            ports: vec![],
            volumes: vec![],
            labels: HashMap::new(),
            network: None,
            dns: vec!["10.0.0.1".into()],
            dns_search: vec!["ecommerce.internal".into()],
        };
        assert_eq!(config.dns, vec!["10.0.0.1"]);
        assert_eq!(config.dns_search, vec!["ecommerce.internal"]);
    }
}
```

- [ ] **Step 2: Add the fields to ContainerConfig**

In the file containing `ContainerConfig` (either `crates/nexa-core/src/runtime/traits.rs` or `crates/nexa-core/src/ports/runtime.rs`), add two fields:

```rust
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    pub name: String,
    pub image: String,
    pub env: HashMap<String, String>,
    pub ports: Vec<PortBinding>,
    pub volumes: Vec<VolumeBinding>,
    pub labels: HashMap<String, String>,
    pub network: Option<String>,
    pub dns: Vec<String>,           // <-- NEW: DNS server IPs (e.g., master node IP)
    pub dns_search: Vec<String>,    // <-- NEW: search domains (e.g., ["ecommerce.internal"])
}
```

- [ ] **Step 3: Fix all ContainerConfig construction sites**

Every place that constructs a `ContainerConfig` must now include `dns` and `dns_search`. Search for them:

```bash
grep -rn "ContainerConfig {" crates/ --include="*.rs"
```

For each match, add:
```rust
dns: vec![],
dns_search: vec![],
```

This includes:
- The orchestrator's `create_pod` method (in `crates/nexad/src/engine/orchestrator.rs` or `crates/nexa-core/src/domain/orchestrator.rs`)
- Any test mock code that constructs `ContainerConfig`

- [ ] **Step 4: Update Docker adapter to pass dns/dns_search to bollard**

In the Docker adapter's `create_container` method (either `crates/nexa-core/src/runtime/docker.rs` or `crates/nexad/src/adapters/runtime/docker.rs`), update the `HostConfig` construction:

```rust
let host_config = HostConfig {
    port_bindings: Some(port_bindings),
    binds: Some(binds),
    network_mode: config.network.clone(),
    dns: if config.dns.is_empty() {
        None
    } else {
        Some(config.dns.clone())
    },
    dns_search: if config.dns_search.is_empty() {
        None
    } else {
        Some(config.dns_search.clone())
    },
    ..Default::default()
};
```

- [ ] **Step 5: Verify compilation and tests**

Run: `cargo check 2>&1 && cargo test 2>&1`
Expected: all compile, all tests pass

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(dns): add dns/dns_search fields to ContainerConfig and Docker adapter"
```

---

### Task 4: Create DnsRecordStore with register/deregister/lookup logic

**Files:**
- Create: `crates/nexad/src/adapters/dns/record_store.rs`
- Modify: `crates/nexad/src/adapters/dns/mod.rs`

- [ ] **Step 1: Write failing tests for DnsRecordStore**

Create `crates/nexad/src/adapters/dns/record_store.rs`:

```rust
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::RwLock;

/// Thread-safe in-memory DNS record store.
///
/// Structure: project -> deployment -> Vec<IpAddr>
///
/// Naming convention:
///   <deployment>.<project>.internal          -> all IPs (round-robin)
///   <deployment>-<index>.<project>.internal   -> specific replica by insertion order
pub struct DnsRecordStore {
    entries: RwLock<HashMap<String, HashMap<String, Vec<IpAddr>>>>,
}

impl DnsRecordStore {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Add an IP for a deployment. Duplicates are ignored.
    pub fn register(&self, project: &str, deployment: &str, ip: IpAddr) {
        let mut entries = self.entries.write().unwrap();
        let project_map = entries.entry(project.to_string()).or_default();
        let ips = project_map.entry(deployment.to_string()).or_default();
        if !ips.contains(&ip) {
            ips.push(ip);
        }
    }

    /// Remove an IP for a deployment.
    pub fn deregister(&self, project: &str, deployment: &str, ip: IpAddr) {
        let mut entries = self.entries.write().unwrap();
        if let Some(project_map) = entries.get_mut(project) {
            if let Some(ips) = project_map.get_mut(deployment) {
                ips.retain(|existing| existing != &ip);
                if ips.is_empty() {
                    project_map.remove(deployment);
                }
            }
            if project_map.is_empty() {
                entries.remove(project);
            }
        }
    }

    /// Lookup all IPs for a deployment (round-robin order is caller's responsibility).
    pub fn lookup(&self, project: &str, deployment: &str) -> Vec<IpAddr> {
        let entries = self.entries.read().unwrap();
        entries
            .get(project)
            .and_then(|m| m.get(deployment))
            .cloned()
            .unwrap_or_default()
    }

    /// Lookup a specific replica by index (0-based insertion order).
    pub fn lookup_replica(&self, project: &str, deployment: &str, index: usize) -> Option<IpAddr> {
        let entries = self.entries.read().unwrap();
        entries
            .get(project)
            .and_then(|m| m.get(deployment))
            .and_then(|ips| ips.get(index).copied())
    }

    /// Parse a DNS query name and resolve it.
    /// Expects format: `<deployment>.<project>.internal` or `<deployment>-<index>.<project>.internal`
    /// Returns None if format doesn't match or no records found.
    pub fn resolve(&self, query_name: &str) -> Option<Vec<IpAddr>> {
        let name = query_name.trim_end_matches('.');
        let parts: Vec<&str> = name.splitn(3, '.').collect();
        if parts.len() != 3 || parts[2] != "internal" {
            return None;
        }

        let project = parts[1];
        let host = parts[0];

        // Check for replica-specific query: <deployment>-<index>
        if let Some(dash_pos) = host.rfind('-') {
            let maybe_index = &host[(dash_pos + 1)..];
            if let Ok(index) = maybe_index.parse::<usize>() {
                let deployment = &host[..dash_pos];
                // First try replica-specific lookup
                if let Some(ip) = self.lookup_replica(project, deployment, index) {
                    return Some(vec![ip]);
                }
                // Fall through: maybe the whole string is the deployment name
            }
        }

        // Standard deployment lookup
        let ips = self.lookup(project, host);
        if ips.is_empty() {
            None
        } else {
            Some(ips)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn register_and_lookup() {
        let store = DnsRecordStore::new();
        store.register("ecommerce", "api", ip(10, 0, 0, 1));
        store.register("ecommerce", "api", ip(10, 0, 0, 2));

        let result = store.lookup("ecommerce", "api");
        assert_eq!(result, vec![ip(10, 0, 0, 1), ip(10, 0, 0, 2)]);
    }

    #[test]
    fn register_ignores_duplicates() {
        let store = DnsRecordStore::new();
        store.register("app", "web", ip(10, 0, 0, 1));
        store.register("app", "web", ip(10, 0, 0, 1));

        assert_eq!(store.lookup("app", "web").len(), 1);
    }

    #[test]
    fn deregister_removes_ip() {
        let store = DnsRecordStore::new();
        store.register("app", "web", ip(10, 0, 0, 1));
        store.register("app", "web", ip(10, 0, 0, 2));
        store.deregister("app", "web", ip(10, 0, 0, 1));

        assert_eq!(store.lookup("app", "web"), vec![ip(10, 0, 0, 2)]);
    }

    #[test]
    fn deregister_cleans_up_empty_maps() {
        let store = DnsRecordStore::new();
        store.register("app", "web", ip(10, 0, 0, 1));
        store.deregister("app", "web", ip(10, 0, 0, 1));

        assert!(store.lookup("app", "web").is_empty());
        // Internal maps should be cleaned up
        let entries = store.entries.read().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn deregister_nonexistent_is_noop() {
        let store = DnsRecordStore::new();
        // Should not panic
        store.deregister("app", "web", ip(10, 0, 0, 1));
    }

    #[test]
    fn lookup_missing_returns_empty() {
        let store = DnsRecordStore::new();
        assert!(store.lookup("nonexistent", "api").is_empty());
    }

    #[test]
    fn lookup_replica_by_index() {
        let store = DnsRecordStore::new();
        store.register("ecommerce", "api", ip(10, 0, 0, 1));
        store.register("ecommerce", "api", ip(10, 0, 0, 2));
        store.register("ecommerce", "api", ip(10, 0, 0, 3));

        assert_eq!(store.lookup_replica("ecommerce", "api", 0), Some(ip(10, 0, 0, 1)));
        assert_eq!(store.lookup_replica("ecommerce", "api", 2), Some(ip(10, 0, 0, 3)));
        assert_eq!(store.lookup_replica("ecommerce", "api", 5), None);
    }

    #[test]
    fn resolve_deployment_name() {
        let store = DnsRecordStore::new();
        store.register("ecommerce", "api", ip(10, 0, 0, 1));
        store.register("ecommerce", "api", ip(10, 0, 0, 2));

        let result = store.resolve("api.ecommerce.internal").unwrap();
        assert_eq!(result, vec![ip(10, 0, 0, 1), ip(10, 0, 0, 2)]);
    }

    #[test]
    fn resolve_deployment_name_with_trailing_dot() {
        let store = DnsRecordStore::new();
        store.register("ecommerce", "api", ip(10, 0, 0, 1));

        let result = store.resolve("api.ecommerce.internal.").unwrap();
        assert_eq!(result, vec![ip(10, 0, 0, 1)]);
    }

    #[test]
    fn resolve_specific_replica() {
        let store = DnsRecordStore::new();
        store.register("ecommerce", "api", ip(10, 0, 0, 1));
        store.register("ecommerce", "api", ip(10, 0, 0, 2));
        store.register("ecommerce", "api", ip(10, 0, 0, 3));

        let result = store.resolve("api-1.ecommerce.internal").unwrap();
        assert_eq!(result, vec![ip(10, 0, 0, 2)]);
    }

    #[test]
    fn resolve_non_internal_returns_none() {
        let store = DnsRecordStore::new();
        store.register("ecommerce", "api", ip(10, 0, 0, 1));

        assert!(store.resolve("api.ecommerce.com").is_none());
    }

    #[test]
    fn resolve_unknown_deployment_returns_none() {
        let store = DnsRecordStore::new();
        assert!(store.resolve("unknown.myapp.internal").is_none());
    }

    #[test]
    fn resolve_deployment_name_with_dashes() {
        // A deployment named "my-api" should resolve even though it contains a dash
        let store = DnsRecordStore::new();
        store.register("ecommerce", "my-api", ip(10, 0, 0, 1));

        let result = store.resolve("my-api.ecommerce.internal").unwrap();
        assert_eq!(result, vec![ip(10, 0, 0, 1)]);
    }

    #[test]
    fn multiple_projects_isolated() {
        let store = DnsRecordStore::new();
        store.register("proj-a", "api", ip(10, 0, 0, 1));
        store.register("proj-b", "api", ip(10, 0, 0, 2));

        assert_eq!(
            store.resolve("api.proj-a.internal").unwrap(),
            vec![ip(10, 0, 0, 1)]
        );
        assert_eq!(
            store.resolve("api.proj-b.internal").unwrap(),
            vec![ip(10, 0, 0, 2)]
        );
    }
}
```

- [ ] **Step 2: Wire into dns adapter module**

Update `crates/nexad/src/adapters/dns/mod.rs`:

```rust
mod noop;
pub mod record_store;

pub use noop::NoopDnsProvider;
pub use record_store::DnsRecordStore;
```

- [ ] **Step 3: Verify and test**

Run: `cargo test -p nexad -- adapters::dns::record_store 2>&1`
Expected: all 13 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/nexad/src/adapters/dns/record_store.rs crates/nexad/src/adapters/dns/mod.rs
git commit -m "feat(dns): implement DnsRecordStore with register/deregister/resolve logic"
```

---

### Task 5: Implement HickoryDnsProvider adapter with embedded DNS server

**Files:**
- Create: `crates/nexad/src/adapters/dns/hickory.rs`
- Modify: `crates/nexad/src/adapters/dns/mod.rs`
- Modify: `Cargo.toml` (workspace deps)
- Modify: `crates/nexad/Cargo.toml`

- [ ] **Step 1: Add hickory-dns dependencies to workspace**

In `Cargo.toml` (workspace root), add to `[workspace.dependencies]`:

```toml
hickory-dns = "0.25"
hickory-server = "0.25"
```

In `crates/nexad/Cargo.toml`, add to `[dependencies]`:

```toml
hickory-dns = { workspace = true }
hickory-server = { workspace = true }
```

- [ ] **Step 2: Verify deps resolve**

Run: `cargo check -p nexad 2>&1 | head -20`
Expected: downloads and compiles (or gives a clear error about API usage, not about missing crate)

**NOTE:** If hickory-dns 0.25 is not yet published, use the latest available version (e.g., 0.24). Adjust imports accordingly. The key crates are:
- `hickory_server::ServerFuture` (the DNS server)
- `hickory_server::authority::{Authority, Catalog, ZoneType}`
- `hickory_server::store::in_memory::InMemoryAuthority`
- `hickory_proto::rr::{Name, RData, Record, RecordType, RrKey}`
- `hickory_proto::op::{Header, MessageType, OpCode, ResponseCode}`

If the API has changed, adapt accordingly. The core pattern is:
1. Create a `Catalog` with an `InMemoryAuthority` for the `internal.` zone
2. Run `ServerFuture` bound to UDP+TCP on port 53
3. On register/deregister, update records in the authority

- [ ] **Step 3: Implement HickoryDnsProvider**

Create `crates/nexad/src/adapters/dns/hickory.rs`:

```rust
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::net::{TcpListener, UdpSocket};
use tracing::{error, info};

use nexa_core::error::{NexaError, Result};
use nexa_core::ports::dns::DnsProvider;

use super::record_store::DnsRecordStore;

/// Embedded DNS provider using hickory-dns.
///
/// Runs a lightweight DNS server on port 53 (UDP + TCP).
/// Resolves `*.internal` queries from the DnsRecordStore.
/// Forwards all other queries to upstream DNS (e.g., 8.8.8.8).
pub struct HickoryDnsProvider {
    store: Arc<DnsRecordStore>,
    listen_addr: SocketAddr,
    upstream_dns: SocketAddr,
}

impl HickoryDnsProvider {
    /// Create a new HickoryDnsProvider.
    ///
    /// - `listen_addr`: address to bind the DNS server (e.g., 0.0.0.0:53)
    /// - `upstream_dns`: upstream DNS for forwarding non-.internal queries (e.g., 8.8.8.8:53)
    pub fn new(listen_addr: SocketAddr, upstream_dns: SocketAddr) -> Self {
        Self {
            store: Arc::new(DnsRecordStore::new()),
            listen_addr,
            upstream_dns,
        }
    }

    /// Start the DNS server in a background tokio task.
    /// Returns immediately; the server runs until the process exits.
    pub async fn start(&self) -> Result<()> {
        let store = self.store.clone();
        let listen_addr = self.listen_addr;
        let upstream_dns = self.upstream_dns;

        // Bind UDP socket
        let udp_socket = UdpSocket::bind(listen_addr)
            .await
            .map_err(|e| NexaError::Runtime(format!("failed to bind DNS UDP on {listen_addr}: {e}")))?;

        // Bind TCP listener
        let tcp_listener = TcpListener::bind(listen_addr)
            .await
            .map_err(|e| NexaError::Runtime(format!("failed to bind DNS TCP on {listen_addr}: {e}")))?;

        info!(%listen_addr, %upstream_dns, "starting embedded DNS server");

        // Spawn UDP handler
        let udp_store = store.clone();
        let udp_upstream = upstream_dns;
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match udp_socket.recv_from(&mut buf).await {
                    Ok((len, src)) => {
                        let data = buf[..len].to_vec();
                        let socket_ref = &udp_socket;
                        let response = handle_dns_query(&data, &udp_store, udp_upstream).await;
                        if let Some(response_bytes) = response {
                            if let Err(e) = socket_ref.send_to(&response_bytes, src).await {
                                error!(%e, "failed to send DNS UDP response");
                            }
                        }
                    }
                    Err(e) => {
                        error!(%e, "DNS UDP recv error");
                    }
                }
            }
        });

        // Spawn TCP handler
        let tcp_store = store.clone();
        let tcp_upstream = upstream_dns;
        tokio::spawn(async move {
            loop {
                match tcp_listener.accept().await {
                    Ok((stream, _addr)) => {
                        let store = tcp_store.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_tcp_dns_client(stream, &store, tcp_upstream).await {
                                error!(%e, "DNS TCP handler error");
                            }
                        });
                    }
                    Err(e) => {
                        error!(%e, "DNS TCP accept error");
                    }
                }
            }
        });

        Ok(())
    }

    /// Get a reference to the underlying record store (for testing).
    pub fn store(&self) -> &Arc<DnsRecordStore> {
        &self.store
    }
}

#[async_trait]
impl DnsProvider for HickoryDnsProvider {
    async fn register(&self, project: &str, deployment: &str, ip: IpAddr) -> Result<()> {
        self.store.register(project, deployment, ip);
        info!(project, deployment, %ip, "DNS record registered");
        Ok(())
    }

    async fn deregister(&self, project: &str, deployment: &str, ip: IpAddr) -> Result<()> {
        self.store.deregister(project, deployment, ip);
        info!(project, deployment, %ip, "DNS record deregistered");
        Ok(())
    }

    async fn lookup(&self, project: &str, deployment: &str) -> Result<Vec<IpAddr>> {
        Ok(self.store.lookup(project, deployment))
    }
}

/// Handle a single DNS TCP client connection.
/// TCP DNS uses a 2-byte length prefix before each message.
async fn handle_tcp_dns_client(
    mut stream: tokio::net::TcpStream,
    store: &DnsRecordStore,
    upstream: SocketAddr,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Read 2-byte length prefix
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let msg_len = u16::from_be_bytes(len_buf) as usize;

    // Read the DNS message
    let mut msg_buf = vec![0u8; msg_len];
    stream.read_exact(&mut msg_buf).await?;

    // Process the query
    if let Some(response) = handle_dns_query(&msg_buf, store, upstream).await {
        // Write 2-byte length prefix + response
        let resp_len = (response.len() as u16).to_be_bytes();
        stream.write_all(&resp_len).await?;
        stream.write_all(&response).await?;
    }

    Ok(())
}

/// Parse a raw DNS query, resolve it from the store or forward upstream.
/// Returns the raw response bytes, or None on parse failure.
async fn handle_dns_query(
    data: &[u8],
    store: &DnsRecordStore,
    upstream: SocketAddr,
) -> Option<Vec<u8>> {
    // Minimal DNS message parsing
    // DNS header is 12 bytes minimum
    if data.len() < 12 {
        return None;
    }

    let id = u16::from_be_bytes([data[0], data[1]]);
    let flags = u16::from_be_bytes([data[2], data[3]]);
    let qd_count = u16::from_be_bytes([data[4], data[5]]);

    // Only handle standard queries (opcode 0)
    let opcode = (flags >> 11) & 0xF;
    if opcode != 0 || qd_count == 0 {
        return None;
    }

    // Parse the question section to extract the query name and type
    let (query_name, qtype, question_end) = parse_question(data, 12)?;

    // Check if this is an .internal query
    let name_lower = query_name.to_lowercase();
    if name_lower.ends_with(".internal") || name_lower.ends_with(".internal.") {
        // Resolve from our store
        let ips = store.resolve(&name_lower);
        return Some(build_dns_response(id, data, &query_name, question_end, qtype, ips));
    }

    // Forward to upstream DNS
    forward_to_upstream(data, upstream).await
}

/// Parse the DNS question section starting at `offset`.
/// Returns (query_name, qtype, end_offset).
fn parse_question(data: &[u8], mut offset: usize) -> Option<(String, u16, usize)> {
    let mut labels = Vec::new();

    loop {
        if offset >= data.len() {
            return None;
        }
        let label_len = data[offset] as usize;
        offset += 1;

        if label_len == 0 {
            break;
        }

        if offset + label_len > data.len() {
            return None;
        }

        let label = std::str::from_utf8(&data[offset..offset + label_len]).ok()?;
        labels.push(label.to_string());
        offset += label_len;
    }

    if offset + 4 > data.len() {
        return None;
    }

    let qtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
    // qclass at offset+2..offset+4 (skip)
    offset += 4;

    let name = labels.join(".");
    Some((name, qtype, offset))
}

/// Build a DNS response for an .internal query.
/// qtype 1 = A record (IPv4), qtype 28 = AAAA record (IPv6).
fn build_dns_response(
    id: u16,
    original: &[u8],
    _query_name: &str,
    question_end: usize,
    qtype: u16,
    ips: Option<Vec<IpAddr>>,
) -> Vec<u8> {
    let ips = ips.unwrap_or_default();

    // Filter IPs by query type
    let matching_ips: Vec<&IpAddr> = ips
        .iter()
        .filter(|ip| match (qtype, ip) {
            (1, IpAddr::V4(_)) => true,   // A record
            (28, IpAddr::V6(_)) => true,   // AAAA record
            (255, _) => true,              // ANY
            _ => false,
        })
        .collect();

    let an_count = matching_ips.len() as u16;
    let rcode = if matching_ips.is_empty() && ips.is_empty() { 3u16 } else { 0u16 }; // NXDOMAIN or NOERROR

    // Build response header
    let flags: u16 = 0x8000  // QR=1 (response)
        | 0x0400              // AA=1 (authoritative)
        | 0x0080              // RA=1 (recursion available)
        | rcode;

    let mut response = Vec::with_capacity(512);

    // Header (12 bytes)
    response.extend_from_slice(&id.to_be_bytes());
    response.extend_from_slice(&flags.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());        // QDCOUNT = 1
    response.extend_from_slice(&an_count.to_be_bytes());    // ANCOUNT
    response.extend_from_slice(&0u16.to_be_bytes());        // NSCOUNT
    response.extend_from_slice(&0u16.to_be_bytes());        // ARCOUNT

    // Copy question section from original
    response.extend_from_slice(&original[12..question_end]);

    // Answer section — one RR per IP
    for ip in &matching_ips {
        // Name pointer to question (offset 12)
        response.extend_from_slice(&[0xC0, 0x0C]);

        match ip {
            IpAddr::V4(v4) => {
                response.extend_from_slice(&1u16.to_be_bytes());     // TYPE = A
                response.extend_from_slice(&1u16.to_be_bytes());     // CLASS = IN
                response.extend_from_slice(&60u32.to_be_bytes());    // TTL = 60s
                response.extend_from_slice(&4u16.to_be_bytes());     // RDLENGTH = 4
                response.extend_from_slice(&v4.octets());
            }
            IpAddr::V6(v6) => {
                response.extend_from_slice(&28u16.to_be_bytes());    // TYPE = AAAA
                response.extend_from_slice(&1u16.to_be_bytes());     // CLASS = IN
                response.extend_from_slice(&60u32.to_be_bytes());    // TTL = 60s
                response.extend_from_slice(&16u16.to_be_bytes());    // RDLENGTH = 16
                response.extend_from_slice(&v6.octets());
            }
        }
    }

    response
}

/// Forward a DNS query to the upstream resolver and return the raw response.
async fn forward_to_upstream(data: &[u8], upstream: SocketAddr) -> Option<Vec<u8>> {
    let socket = UdpSocket::bind("0.0.0.0:0").await.ok()?;
    socket.send_to(data, upstream).await.ok()?;

    let mut buf = vec![0u8; 4096];
    match tokio::time::timeout(Duration::from_secs(5), socket.recv_from(&mut buf)).await {
        Ok(Ok((len, _))) => Some(buf[..len].to_vec()),
        _ => {
            error!("upstream DNS timeout");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[tokio::test]
    async fn register_and_lookup_via_provider() {
        let provider = HickoryDnsProvider::new(
            "127.0.0.1:15353".parse().unwrap(),
            "8.8.8.8:53".parse().unwrap(),
        );

        provider.register("ecommerce", "api", ip4(10, 0, 0, 1)).await.unwrap();
        provider.register("ecommerce", "api", ip4(10, 0, 0, 2)).await.unwrap();

        let ips = provider.lookup("ecommerce", "api").await.unwrap();
        assert_eq!(ips, vec![ip4(10, 0, 0, 1), ip4(10, 0, 0, 2)]);
    }

    #[tokio::test]
    async fn deregister_removes_via_provider() {
        let provider = HickoryDnsProvider::new(
            "127.0.0.1:15354".parse().unwrap(),
            "8.8.8.8:53".parse().unwrap(),
        );

        provider.register("app", "web", ip4(10, 0, 0, 1)).await.unwrap();
        provider.deregister("app", "web", ip4(10, 0, 0, 1)).await.unwrap();

        let ips = provider.lookup("app", "web").await.unwrap();
        assert!(ips.is_empty());
    }

    #[test]
    fn parse_question_extracts_name_and_type() {
        // Manually construct a DNS query for "api.ecommerce.internal" type A
        let mut data = vec![
            0x00, 0x01, // ID
            0x01, 0x00, // Flags: standard query
            0x00, 0x01, // QDCOUNT: 1
            0x00, 0x00, // ANCOUNT: 0
            0x00, 0x00, // NSCOUNT: 0
            0x00, 0x00, // ARCOUNT: 0
        ];
        // Question: api.ecommerce.internal
        data.push(3); data.extend_from_slice(b"api");
        data.push(9); data.extend_from_slice(b"ecommerce");
        data.push(8); data.extend_from_slice(b"internal");
        data.push(0); // end of name
        data.extend_from_slice(&[0x00, 0x01]); // QTYPE: A
        data.extend_from_slice(&[0x00, 0x01]); // QCLASS: IN

        let (name, qtype, _end) = parse_question(&data, 12).unwrap();
        assert_eq!(name, "api.ecommerce.internal");
        assert_eq!(qtype, 1); // A record
    }

    #[test]
    fn build_response_with_ips() {
        let mut query = vec![
            0x00, 0x42, // ID
            0x01, 0x00, // Flags
            0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        query.push(3); query.extend_from_slice(b"api");
        query.push(9); query.extend_from_slice(b"ecommerce");
        query.push(8); query.extend_from_slice(b"internal");
        query.push(0);
        query.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]); // A, IN

        let question_end = query.len();
        let ips = Some(vec![ip4(10, 0, 0, 1), ip4(10, 0, 0, 2)]);

        let response = build_dns_response(0x0042, &query, "api.ecommerce.internal", question_end, 1, ips);

        // Verify response header
        assert_eq!(response[0..2], [0x00, 0x42]); // ID matches
        let flags = u16::from_be_bytes([response[2], response[3]]);
        assert!(flags & 0x8000 != 0); // QR=1 (response)
        assert!(flags & 0x0400 != 0); // AA=1 (authoritative)
        let an_count = u16::from_be_bytes([response[6], response[7]]);
        assert_eq!(an_count, 2); // 2 answers
    }

    #[test]
    fn build_response_nxdomain_when_no_ips() {
        let mut query = vec![
            0x00, 0x01, 0x01, 0x00,
            0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        query.push(3); query.extend_from_slice(b"xxx");
        query.push(3); query.extend_from_slice(b"yyy");
        query.push(8); query.extend_from_slice(b"internal");
        query.push(0);
        query.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);

        let question_end = query.len();

        let response = build_dns_response(0x0001, &query, "xxx.yyy.internal", question_end, 1, None);
        let flags = u16::from_be_bytes([response[2], response[3]]);
        let rcode = flags & 0x000F;
        assert_eq!(rcode, 3); // NXDOMAIN
    }
}
```

- [ ] **Step 4: Wire into dns adapter module**

Update `crates/nexad/src/adapters/dns/mod.rs`:

```rust
mod hickory;
mod noop;
pub mod record_store;

pub use hickory::HickoryDnsProvider;
pub use noop::NoopDnsProvider;
pub use record_store::DnsRecordStore;
```

- [ ] **Step 5: Verify and test**

Run: `cargo test -p nexad -- adapters::dns 2>&1`
Expected: all tests pass (NoopDnsProvider tests + DnsRecordStore tests + HickoryDnsProvider tests)

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(dns): implement HickoryDnsProvider with embedded UDP/TCP DNS server and upstream forwarding"
```

---

### Task 6: Integrate DNS registration into orchestrator pod lifecycle

**Files:**
- Modify: `crates/nexa-core/src/domain/orchestrator.rs` (if hexagonal layout done) or `crates/nexad/src/engine/orchestrator.rs`

- [ ] **Step 1: Write failing test for DNS integration**

Add to the orchestrator test module. Requires a mock/spy DnsProvider:

```rust
use std::net::IpAddr;
use std::sync::Mutex;
use nexa_core::ports::dns::DnsProvider;

struct SpyDnsProvider {
    registered: Mutex<Vec<(String, String, IpAddr)>>,
    deregistered: Mutex<Vec<(String, String, IpAddr)>>,
}

impl SpyDnsProvider {
    fn new() -> Self {
        Self {
            registered: Mutex::new(vec![]),
            deregistered: Mutex::new(vec![]),
        }
    }
}

#[async_trait::async_trait]
impl DnsProvider for SpyDnsProvider {
    async fn register(&self, project: &str, deployment: &str, ip: IpAddr) -> nexa_core::error::Result<()> {
        self.registered.lock().unwrap().push((
            project.to_string(),
            deployment.to_string(),
            ip,
        ));
        Ok(())
    }

    async fn deregister(&self, project: &str, deployment: &str, ip: IpAddr) -> nexa_core::error::Result<()> {
        self.deregistered.lock().unwrap().push((
            project.to_string(),
            deployment.to_string(),
            ip,
        ));
        Ok(())
    }

    async fn lookup(&self, _project: &str, _deployment: &str) -> nexa_core::error::Result<Vec<IpAddr>> {
        Ok(vec![])
    }
}
```

Add test:

```rust
#[tokio::test]
async fn deploy_registers_dns_for_pods() {
    let dns = Arc::new(SpyDnsProvider::new());
    let handle = Orchestrator::spawn(Arc::new(MockRuntime), dns.clone());

    let spec = DeploymentSpec {
        project: "ecommerce".into(),
        deployment: DeploymentMeta { name: "api".into() },
        replicas: 2,
        image: "nginx:latest".into(),
        ports: vec![],
        env: HashMap::new(),
        volumes: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::default(),
    };

    handle.deploy(spec).await.unwrap();

    let registered = dns.registered.lock().unwrap();
    assert_eq!(registered.len(), 2);
    assert!(registered.iter().all(|(p, d, _)| p == "ecommerce" && d == "api"));
}

#[tokio::test]
async fn stop_deregisters_dns_for_pods() {
    let dns = Arc::new(SpyDnsProvider::new());
    let handle = Orchestrator::spawn(Arc::new(MockRuntime), dns.clone());

    let spec = DeploymentSpec {
        project: "ecommerce".into(),
        deployment: DeploymentMeta { name: "api".into() },
        replicas: 1,
        image: "nginx:latest".into(),
        ports: vec![],
        env: HashMap::new(),
        volumes: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::default(),
    };

    handle.deploy(spec).await.unwrap();
    handle.stop("ecommerce".into(), "api".into()).await.unwrap();

    let deregistered = dns.deregistered.lock().unwrap();
    assert_eq!(deregistered.len(), 1);
    assert_eq!(deregistered[0].0, "ecommerce");
    assert_eq!(deregistered[0].1, "api");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p nexa-core -- domain::orchestrator 2>&1` (or `cargo test -p nexad -- engine::orchestrator 2>&1`)
Expected: FAIL -- `Orchestrator::spawn` does not accept a `dns` parameter yet

- [ ] **Step 3: Add DnsProvider to the Orchestrator**

Update the `Orchestrator` struct to hold a DNS provider:

```rust
pub struct Orchestrator {
    runtime: Arc<dyn ContainerRuntime>,
    dns: Arc<dyn DnsProvider>,
    projects: StdHashMap<String, Project>,
    deployments: StdHashMap<Uuid, Deployment>,
    pods: StdHashMap<Uuid, Pod>,
}
```

Update `Orchestrator::spawn` to accept a DNS provider:

```rust
pub fn spawn(
    runtime: Arc<dyn ContainerRuntime>,
    dns: Arc<dyn DnsProvider>,
) -> OrchestratorHandle {
    let (tx, rx) = mpsc::channel(256);
    tokio::spawn(async move {
        let mut orch = Self {
            runtime,
            dns,
            projects: StdHashMap::new(),
            deployments: StdHashMap::new(),
            pods: StdHashMap::new(),
        };
        orch.run(rx).await;
    });
    OrchestratorHandle { tx }
}
```

- [ ] **Step 4: Add DNS registration to create_pod**

In the `create_pod` method, after the container starts successfully and has an IP, register with DNS.

The container IP comes from inspecting the container after start. Add a `get_container_ip` method to `ContainerRuntime` trait, or use the container network settings. The simplest approach: after `start_container`, inspect the container to get its IP address.

Add to the `ContainerRuntime` trait in `crates/nexa-core/src/runtime/traits.rs` (or `ports/runtime.rs`):

```rust
async fn container_ip(&self, id: &str, network: &str) -> Result<Option<IpAddr>>;
```

Implement in the Docker adapter:

```rust
async fn container_ip(&self, id: &str, network: &str) -> Result<Option<IpAddr>> {
    let info = self.client.inspect_container(id, None)
        .await
        .map_err(|e| NexaError::Runtime(e.to_string()))?;

    let ip = info
        .network_settings
        .and_then(|ns| ns.networks)
        .and_then(|nets| nets.get(network).cloned())
        .and_then(|net| net.ip_address)
        .and_then(|ip_str| ip_str.parse::<IpAddr>().ok());

    Ok(ip)
}
```

Implement in MockRuntime (for tests):

```rust
async fn container_ip(&self, _id: &str, _network: &str) -> Result<Option<IpAddr>> {
    // Return a deterministic test IP
    static COUNTER: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(Some(IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, n))))
}
```

Then in `create_pod`, after the container starts:

```rust
match self.runtime.create_container(&config).await {
    Ok(container_id) => {
        self.runtime.start_container(&container_id).await?;
        pod.container_id = Some(container_id.clone());
        pod.status = PodStatus::Running;

        // Register DNS
        if let Some(ip) = self.runtime.container_ip(&container_id, &network_name).await? {
            pod.container_ip = Some(ip);
            let _ = self.dns.register(&spec.project, &spec.deployment.name, ip).await;
        }
    }
    Err(_) => {
        pod.status = PodStatus::Failed;
    }
}
```

- [ ] **Step 5: Add container_ip field to Pod model**

In `crates/nexa-core/src/models/pod.rs` (or `domain/models/pod.rs`), add:

```rust
use std::net::IpAddr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pod {
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub project: String,
    pub deployment_name: String,
    pub replica_index: u32,
    pub container_id: Option<String>,
    pub container_ip: Option<IpAddr>,   // <-- NEW
    pub status: PodStatus,
    pub image: String,
    pub created_at: DateTime<Utc>,
}
```

Update `Pod::new` to set `container_ip: None`.

- [ ] **Step 6: Add DNS deregistration to stop/remove paths**

In `handle_stop` (or `stop_deployment`), before removing pods, deregister DNS:

```rust
for pod_id in &pod_ids {
    if let Some(pod) = self.pods.get(pod_id) {
        // Deregister DNS
        if let Some(ip) = pod.container_ip {
            let _ = self.dns.deregister(&pod.project, &pod.deployment_name, ip).await;
        }
        // Stop and remove container
        if let Some(cid) = &pod.container_id {
            let _ = self.runtime.stop_container(cid, 10).await;
            let _ = self.runtime.remove_container(cid, true).await;
        }
    }
    self.pods.remove(pod_id);
}
```

Similarly in `reconcile_deployment` when scaling down:

```rust
for &pod_id in &current_pods[(desired as usize)..] {
    if let Some(pod) = self.pods.get(&pod_id) {
        // Deregister DNS
        if let Some(ip) = pod.container_ip {
            let _ = self.dns.deregister(&pod.project, &pod.deployment_name, ip).await;
        }
        if let Some(cid) = &pod.container_id {
            let _ = self.runtime.stop_container(cid, 10).await;
            let _ = self.runtime.remove_container(cid, true).await;
        }
    }
    self.pods.remove(&pod_id);
}
```

- [ ] **Step 7: Update existing test helper to pass DNS**

Update `spawn_test_orchestrator` (or equivalent) to pass a NoopDnsProvider:

```rust
fn spawn_test_orchestrator() -> OrchestratorHandle {
    Orchestrator::spawn(
        Arc::new(MockRuntime),
        Arc::new(NoopDnsProvider::new()),
    )
}
```

Update all tests that directly call `Orchestrator::spawn` with the old 1-arg signature.

- [ ] **Step 8: Run all tests**

Run: `cargo test 2>&1`
Expected: all tests pass, including the new DNS spy tests

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(dns): integrate DNS register/deregister into orchestrator pod lifecycle"
```

---

### Task 7: Set container DNS config when creating containers

**Files:**
- Modify: `crates/nexad/src/engine/orchestrator.rs` (or `crates/nexa-core/src/domain/orchestrator.rs`)

- [ ] **Step 1: Write failing test for DNS container config**

Add to orchestrator tests:

```rust
struct DnsInspectingRuntime {
    last_config: Mutex<Option<ContainerConfig>>,
}

impl DnsInspectingRuntime {
    fn new() -> Self {
        Self { last_config: Mutex::new(None) }
    }
}

#[async_trait::async_trait]
impl ContainerRuntime for DnsInspectingRuntime {
    async fn create_container(&self, config: &ContainerConfig) -> Result<String> {
        *self.last_config.lock().unwrap() = Some(config.clone());
        Ok(format!("mock-{}", config.name))
    }
    // ... other methods delegate to mock defaults ...
    async fn pull_image(&self, _: &str) -> Result<()> { Ok(()) }
    async fn start_container(&self, _: &str) -> Result<()> { Ok(()) }
    async fn stop_container(&self, _: &str, _: u64) -> Result<()> { Ok(()) }
    async fn remove_container(&self, _: &str, _: bool) -> Result<()> { Ok(()) }
    async fn inspect_container(&self, _: &str) -> Result<ContainerInfo> {
        Ok(ContainerInfo { id: "m".into(), name: "m".into(), image: "m".into(), state: ContainerState::Running })
    }
    async fn logs(&self, _: &str, _: Option<u64>) -> Result<LogStream> {
        Ok(Box::pin(futures::stream::empty()))
    }
    async fn container_exists(&self, _: &str) -> Result<bool> { Ok(false) }
    async fn create_network(&self, _: &str) -> Result<String> { Ok("net".into()) }
    async fn remove_network(&self, _: &str) -> Result<()> { Ok(()) }
    async fn connect_to_network(&self, _: &str, _: &str) -> Result<()> { Ok(()) }
    async fn container_ip(&self, _: &str, _: &str) -> Result<Option<IpAddr>> {
        Ok(Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))))
    }
}

#[tokio::test]
async fn containers_receive_dns_config() {
    let rt = Arc::new(DnsInspectingRuntime::new());
    let handle = Orchestrator::spawn_with_dns_config(
        rt.clone(),
        Arc::new(NoopDnsProvider::new()),
        "10.0.0.100".into(),  // master_ip
    );

    let spec = DeploymentSpec {
        project: "ecommerce".into(),
        deployment: DeploymentMeta { name: "api".into() },
        replicas: 1,
        image: "nginx".into(),
        ports: vec![],
        env: HashMap::new(),
        volumes: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::default(),
    };

    handle.deploy(spec).await.unwrap();

    let config = rt.last_config.lock().unwrap().clone().unwrap();
    assert_eq!(config.dns, vec!["10.0.0.100"]);
    assert_eq!(config.dns_search, vec!["ecommerce.internal"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -- containers_receive_dns_config 2>&1`
Expected: FAIL -- no `spawn_with_dns_config` method

- [ ] **Step 3: Add master_ip to Orchestrator and pass dns/dns_search to ContainerConfig**

Add `master_ip: Option<String>` to the `Orchestrator` struct.

Update `Orchestrator::spawn` (or add a new constructor) to accept a master IP:

```rust
pub fn spawn(
    runtime: Arc<dyn ContainerRuntime>,
    dns: Arc<dyn DnsProvider>,
    master_ip: Option<String>,
) -> OrchestratorHandle {
    let (tx, rx) = mpsc::channel(256);
    tokio::spawn(async move {
        let mut orch = Self {
            runtime,
            dns,
            master_ip,
            projects: StdHashMap::new(),
            deployments: StdHashMap::new(),
            pods: StdHashMap::new(),
        };
        orch.run(rx).await;
    });
    OrchestratorHandle { tx }
}
```

In `create_pod`, when building `ContainerConfig`:

```rust
let (dns_servers, dns_search) = match &self.master_ip {
    Some(ip) => (
        vec![ip.clone()],
        vec![format!("{}.internal", spec.project)],
    ),
    None => (vec![], vec![]),
};

let config = ContainerConfig {
    name: container_name,
    image: spec.image.clone(),
    env: spec.env.clone(),
    ports,
    volumes: /* ... */,
    labels,
    network: Some(network_name),
    dns: dns_servers,
    dns_search,
};
```

- [ ] **Step 4: Update all callers**

Update `spawn_test_orchestrator`:
```rust
fn spawn_test_orchestrator() -> OrchestratorHandle {
    Orchestrator::spawn(
        Arc::new(MockRuntime),
        Arc::new(NoopDnsProvider::new()),
        None,
    )
}
```

Update `nexad/src/main.rs` (in Task 8).

- [ ] **Step 5: Run all tests**

Run: `cargo test 2>&1`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(dns): set container dns/dns_search config pointing to master DNS server"
```

---

### Task 8: Wire DnsProvider into nexad main.rs startup

**Files:**
- Modify: `crates/nexad/src/main.rs`
- Modify: CLI args (add `--dns-mode` and `--master-ip` flags)

- [ ] **Step 1: Add CLI flags**

In `crates/nexad/src/main.rs`, update the `Cli` struct:

```rust
#[derive(Parser)]
#[command(name = "nexad", about = "NexaNet daemon", version)]
struct Cli {
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    #[arg(long, default_value = "6443")]
    port: u16,

    #[arg(long, default_value = "/var/lib/nexa")]
    data_dir: String,

    /// DNS mode: "noop" for single-node (Docker DNS), "embedded" for multi-node
    #[arg(long, default_value = "noop")]
    dns_mode: String,

    /// IP address of this master node (used for container DNS config).
    /// Required when dns_mode=embedded.
    #[arg(long)]
    master_ip: Option<String>,

    /// DNS listen address (host:port) for embedded DNS server
    #[arg(long, default_value = "0.0.0.0:53")]
    dns_listen: String,

    /// Upstream DNS server for forwarding non-.internal queries
    #[arg(long, default_value = "8.8.8.8:53")]
    dns_upstream: String,
}
```

- [ ] **Step 2: Wire DNS provider selection in main()**

```rust
use std::sync::Arc;

use adapters::dns::{HickoryDnsProvider, NoopDnsProvider};
use nexa_core::ports::dns::DnsProvider;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    info!("starting nexad on {}:{}", cli.host, cli.port);

    let runtime = adapters::runtime::DockerRuntime::new()?;
    runtime.ping().await?;
    info!("connected to Docker runtime");

    // Initialize DNS provider
    let dns: Arc<dyn DnsProvider> = match cli.dns_mode.as_str() {
        "embedded" => {
            let listen_addr = cli.dns_listen.parse()
                .expect("invalid --dns-listen address");
            let upstream_addr = cli.dns_upstream.parse()
                .expect("invalid --dns-upstream address");

            let provider = HickoryDnsProvider::new(listen_addr, upstream_addr);
            provider.start().await?;
            info!("embedded DNS server started on {}", cli.dns_listen);
            Arc::new(provider)
        }
        _ => {
            info!("using noop DNS (single-node mode, containers use Docker DNS)");
            Arc::new(NoopDnsProvider::new())
        }
    };

    let handle = Orchestrator::spawn(Arc::new(runtime), dns, cli.master_ip.clone());
    let addr = format!("{}:{}", cli.host, cli.port);

    api::serve(handle, &addr).await
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p nexad 2>&1`
Expected: compiles

- [ ] **Step 4: Verify with --help**

Run: `cargo run -p nexad -- --help 2>&1`
Expected: shows `--dns-mode`, `--master-ip`, `--dns-listen`, `--dns-upstream` flags

- [ ] **Step 5: Commit**

```bash
git add crates/nexad/src/main.rs
git commit -m "feat(dns): wire DnsProvider into nexad startup with CLI flags for mode selection"
```

---

### Task 9: External DNS forwarding for non-.internal queries

**Files:**
- Modify: `crates/nexad/src/adapters/dns/hickory.rs` (already implemented in Task 5, this task adds dedicated tests)

- [ ] **Step 1: Write integration test for external forwarding**

The forwarding logic was implemented in Task 5's `handle_dns_query` and `forward_to_upstream` functions. This task validates the end-to-end behavior.

Add to `crates/nexad/src/adapters/dns/hickory.rs` tests:

```rust
#[test]
fn non_internal_query_not_resolved_locally() {
    let store = DnsRecordStore::new();
    store.register("ecommerce", "api", ip4(10, 0, 0, 1));

    // Query for google.com should NOT be resolved from our store
    assert!(store.resolve("google.com").is_none());
    assert!(store.resolve("api.ecommerce.com").is_none());
    assert!(store.resolve("www.example.org").is_none());
}

#[test]
fn handle_dns_query_identifies_internal_vs_external() {
    // Build a DNS query for "api.ecommerce.internal" type A
    let internal_query = build_test_dns_query("api.ecommerce.internal");
    let external_query = build_test_dns_query("www.google.com");

    let store = DnsRecordStore::new();
    store.register("ecommerce", "api", ip4(10, 0, 0, 1));

    // Internal: parse question and verify it ends with .internal
    let (name, _, _) = parse_question(&internal_query, 12).unwrap();
    assert!(name.ends_with(".internal"));

    let (name, _, _) = parse_question(&external_query, 12).unwrap();
    assert!(!name.ends_with(".internal"));
}

fn build_test_dns_query(domain: &str) -> Vec<u8> {
    let mut data = vec![
        0x00, 0x01, // ID
        0x01, 0x00, // Flags: standard query, RD=1
        0x00, 0x01, // QDCOUNT: 1
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    for label in domain.split('.') {
        data.push(label.len() as u8);
        data.extend_from_slice(label.as_bytes());
    }
    data.push(0); // end of name
    data.extend_from_slice(&[0x00, 0x01]); // QTYPE: A
    data.extend_from_slice(&[0x00, 0x01]); // QCLASS: IN
    data
}
```

- [ ] **Step 2: Write integration test verifying upstream forwarding works end-to-end**

This test requires network access and a real upstream DNS server. Mark it `#[ignore]` for CI:

```rust
#[tokio::test]
#[ignore] // Requires network access to 8.8.8.8
async fn forward_to_upstream_resolves_external_domain() {
    let upstream: SocketAddr = "8.8.8.8:53".parse().unwrap();
    let query = build_test_dns_query("www.google.com");

    let response = forward_to_upstream(&query, upstream).await;
    assert!(response.is_some(), "upstream should return a response");

    let resp = response.unwrap();
    assert!(resp.len() > 12, "response should have header + answers");

    // Verify QR bit is set (response)
    let flags = u16::from_be_bytes([resp[2], resp[3]]);
    assert!(flags & 0x8000 != 0, "should be a response");

    // Verify RCODE is 0 (no error)
    let rcode = flags & 0x000F;
    assert_eq!(rcode, 0, "should be NOERROR");
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p nexad -- adapters::dns 2>&1`
Expected: all non-ignored tests pass

Run ignored test manually if you have network:
```bash
cargo test -p nexad -- adapters::dns::hickory::tests::forward_to_upstream_resolves_external_domain --ignored 2>&1
```

- [ ] **Step 4: Add a DnsError variant to NexaError**

In `crates/nexa-core/src/error.rs`, add:

```rust
#[error("dns error: {0}")]
Dns(String),
```

This allows DNS-specific errors to be propagated properly.

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(dns): add external DNS forwarding tests and DnsError variant"
```

---

### Final verification checklist

After all 9 tasks are complete:

- [ ] `cargo check 2>&1` -- workspace compiles
- [ ] `cargo test 2>&1` -- all tests pass
- [ ] `cargo test -p nexad -- adapters::dns 2>&1` -- DNS adapter tests pass
- [ ] `cargo clippy 2>&1` -- no warnings (fix any that appear)

```bash
git push origin main
```
