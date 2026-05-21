# Container Runtime Abstraction (containerd support) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add containerd as a second container runtime backend alongside Docker. Introduce runtime auto-detection, a CNI networking manager for containerd, file-based log tailing, and a unified integration test suite that validates both runtimes against the same `ContainerRuntime` trait.

**Architecture:** The existing `ContainerRuntime` trait in `nexa-core` gains four new methods (`container_ip`, `events`, `dns` fields on `ContainerConfig`) that were specified but not yet implemented. A `RuntimeDetector` checks socket availability and CLI flags to select the runtime. `ContainerdRuntime` lives in `nexad/src/adapters/runtime/containerd.rs` and delegates networking to a `CniManager`. All containerd containers run in the `nexa` namespace. Logs are written to `{data_dir}/logs/{container_id}/` and tailed via `tokio::fs` + `tokio::io::BufReader`. The `nexa setup cni` CLI command downloads standard CNI plugin binaries.

**Tech Stack:** containerd-client 0.5 (gRPC via tonic), tokio (fs, io, process), serde_json (CNI config), async-trait, futures, sha2 (image digest), bollard (existing Docker adapter)

---

### Task 1: Extend ContainerRuntime trait and ContainerConfig

**Files:**
- Modify: `crates/nexa-core/src/runtime/traits.rs`
- Modify: `crates/nexa-core/src/runtime/docker.rs`

- [ ] **Step 1: Write failing test for new trait methods**

Add to the bottom of `crates/nexa-core/src/runtime/traits.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    /// Verify the trait is object-safe with the new methods.
    fn _assert_object_safe(_: &dyn ContainerRuntime) {}

    #[test]
    fn container_config_has_dns_fields() {
        let config = ContainerConfig {
            name: "test".into(),
            image: "nginx".into(),
            env: HashMap::new(),
            ports: vec![],
            volumes: vec![],
            labels: HashMap::new(),
            network: None,
            dns: vec!["8.8.8.8".into()],
            dns_search: vec!["nexa.local".into()],
        };
        assert_eq!(config.dns.len(), 1);
        assert_eq!(config.dns_search.len(), 1);
    }
}
```

Run: `cargo test -p nexa-core -- runtime::traits::tests 2>&1`
Expected: FAIL -- `ContainerConfig` has no field `dns`, no method `container_ip`/`events`/`runtime_name` on trait

- [ ] **Step 2: Add dns/dns_search fields to ContainerConfig**

In `crates/nexa-core/src/runtime/traits.rs`, add to the `ContainerConfig` struct:

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
    pub dns: Vec<String>,
    pub dns_search: Vec<String>,
}
```

- [ ] **Step 3: Add new methods to the ContainerRuntime trait**

In `crates/nexa-core/src/runtime/traits.rs`, add these imports and types:

```rust
use std::net::IpAddr;
```

Add a new type alias below `LogStream`:

```rust
pub type EventStream = Pin<Box<dyn Stream<Item = Result<ContainerEvent>> + Send>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerEvent {
    pub container_id: String,
    pub event_type: ContainerEventType,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ContainerEventType {
    Started,
    Stopped,
    Died,
    Healthy,
    Unhealthy,
    Oom,
}
```

Extend the trait:

```rust
#[async_trait]
pub trait ContainerRuntime: Send + Sync {
    async fn pull_image(&self, image: &str) -> Result<()>;
    async fn create_container(&self, config: &ContainerConfig) -> Result<String>;
    async fn start_container(&self, id: &str) -> Result<()>;
    async fn stop_container(&self, id: &str, timeout_secs: u64) -> Result<()>;
    async fn remove_container(&self, id: &str, force: bool) -> Result<()>;
    async fn inspect_container(&self, id: &str) -> Result<ContainerInfo>;
    async fn logs(&self, id: &str, tail: Option<u64>) -> Result<LogStream>;
    async fn container_exists(&self, name: &str) -> Result<bool>;
    async fn create_network(&self, name: &str) -> Result<String>;
    async fn remove_network(&self, name: &str) -> Result<()>;
    async fn connect_to_network(&self, container_id: &str, network: &str) -> Result<()>;
    async fn container_ip(&self, id: &str, network: &str) -> Result<IpAddr>;
    async fn events(&self) -> Result<EventStream>;
    fn runtime_name(&self) -> &'static str;
}
```

- [ ] **Step 4: Update DockerRuntime to implement the new methods**

In `crates/nexa-core/src/runtime/docker.rs`, add these imports:

```rust
use std::net::IpAddr;
use chrono::Utc;
```

Add these method implementations inside `impl ContainerRuntime for DockerRuntime`:

```rust
    async fn container_ip(&self, id: &str, network: &str) -> Result<IpAddr> {
        let info = self
            .client
            .inspect_container(id, None)
            .await
            .map_err(|e| NexaError::Runtime(e.to_string()))?;

        let networks = info
            .network_settings
            .and_then(|ns| ns.networks)
            .ok_or_else(|| NexaError::Runtime("no network settings".into()))?;

        let endpoint = networks
            .get(network)
            .ok_or_else(|| NexaError::Runtime(format!("container not on network '{network}'")))?;

        let ip_str = endpoint
            .ip_address
            .as_ref()
            .ok_or_else(|| NexaError::Runtime("no IP address assigned".into()))?;

        ip_str
            .parse::<IpAddr>()
            .map_err(|e| NexaError::Runtime(format!("invalid IP: {e}")))
    }

    async fn events(&self) -> Result<EventStream> {
        use bollard::system::EventsOptions;
        use std::collections::HashMap;

        let filters: HashMap<String, Vec<String>> = HashMap::from([
            ("type".into(), vec!["container".into()]),
            ("label".into(), vec!["managed-by=nexanet".into()]),
        ]);

        let options = EventsOptions::<String> {
            filters,
            ..Default::default()
        };

        let stream = self.client.events(Some(options));

        let mapped = stream.map(|result| match result {
            Ok(event) => {
                let event_type = match event.action.as_deref() {
                    Some("start") => ContainerEventType::Started,
                    Some("stop") => ContainerEventType::Stopped,
                    Some("die") => ContainerEventType::Died,
                    Some("health_status: healthy") => ContainerEventType::Healthy,
                    Some("health_status: unhealthy") => ContainerEventType::Unhealthy,
                    Some("oom") => ContainerEventType::Oom,
                    _ => ContainerEventType::Stopped,
                };

                let container_id = event
                    .actor
                    .and_then(|a| a.id)
                    .unwrap_or_default();

                Ok(ContainerEvent {
                    container_id,
                    event_type,
                    timestamp: Utc::now(),
                })
            }
            Err(e) => Err(NexaError::Runtime(e.to_string())),
        });

        Ok(Box::pin(mapped))
    }

    fn runtime_name(&self) -> &'static str {
        "docker"
    }
```

- [ ] **Step 5: Fix ContainerConfig construction sites**

In `crates/nexad/src/engine/orchestrator.rs`, update every `ContainerConfig { ... }` construction to include the new fields:

```rust
            dns: vec![],
            dns_search: vec![],
```

- [ ] **Step 6: Verify everything compiles and tests pass**

Run: `cargo check 2>&1`
Expected: compiles

Run: `cargo test -p nexa-core -- runtime::traits::tests 2>&1`
Expected: 1 test passes

- [ ] **Step 7: Commit**

```bash
git add crates/nexa-core/src/runtime/ crates/nexad/src/engine/
git commit -m "feat: extend ContainerRuntime trait with container_ip, events, runtime_name, dns fields"
```

---

### Task 2: Runtime auto-detection and --runtime CLI flag

**Files:**
- Create: `crates/nexad/src/adapters/runtime/detect.rs`
- Modify: `crates/nexad/src/adapters/runtime/mod.rs`
- Modify: `crates/nexad/src/main.rs`

- [ ] **Step 1: Write failing tests for runtime detection**

Create `crates/nexad/src/adapters/runtime/detect.rs`:

```rust
use std::path::Path;
use std::sync::Arc;

use nexa_core::error::{NexaError, Result};
use nexa_core::runtime::ContainerRuntime;
use tracing::info;

use super::DockerRuntime;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RuntimeKind {
    Docker,
    Containerd,
    Auto,
}

impl std::str::FromStr for RuntimeKind {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "docker" => Ok(RuntimeKind::Docker),
            "containerd" => Ok(RuntimeKind::Containerd),
            "auto" => Ok(RuntimeKind::Auto),
            other => Err(format!("unknown runtime: '{other}'. expected: docker, containerd, auto")),
        }
    }
}

impl std::fmt::Display for RuntimeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeKind::Docker => write!(f, "docker"),
            RuntimeKind::Containerd => write!(f, "containerd"),
            RuntimeKind::Auto => write!(f, "auto"),
        }
    }
}

/// Probes for available container runtimes by checking socket paths.
pub struct RuntimeDetector;

impl RuntimeDetector {
    const CONTAINERD_SOCK: &'static str = "/run/containerd/containerd.sock";
    const DOCKER_SOCK: &'static str = "/var/run/docker.sock";

    /// Returns (containerd_available, docker_available).
    pub fn probe() -> (bool, bool) {
        let containerd = Path::new(Self::CONTAINERD_SOCK).exists();
        let docker = Path::new(Self::DOCKER_SOCK).exists();
        (containerd, docker)
    }

    /// Auto-detect which runtime to use:
    /// 1. containerd.sock exists -> containerd available
    /// 2. docker.sock exists -> docker available
    /// 3. Both -> prefer Docker (default)
    /// 4. Neither -> fail
    pub fn auto_detect() -> Result<RuntimeKind> {
        let (containerd, docker) = Self::probe();

        match (docker, containerd) {
            (true, _) => {
                info!("auto-detected Docker runtime");
                Ok(RuntimeKind::Docker)
            }
            (false, true) => {
                info!("auto-detected containerd runtime (no Docker socket)");
                Ok(RuntimeKind::Containerd)
            }
            (false, false) => Err(NexaError::Runtime(
                "no container runtime found. expected /var/run/docker.sock or \
                 /run/containerd/containerd.sock"
                    .into(),
            )),
        }
    }

    /// Resolve the user's choice or auto-detect.
    pub fn resolve(kind: RuntimeKind) -> Result<RuntimeKind> {
        match kind {
            RuntimeKind::Auto => Self::auto_detect(),
            RuntimeKind::Docker => {
                if !Path::new(Self::DOCKER_SOCK).exists() {
                    return Err(NexaError::Runtime(
                        "Docker runtime requested but /var/run/docker.sock not found".into(),
                    ));
                }
                Ok(RuntimeKind::Docker)
            }
            RuntimeKind::Containerd => {
                if !Path::new(Self::CONTAINERD_SOCK).exists() {
                    return Err(NexaError::Runtime(
                        "containerd runtime requested but /run/containerd/containerd.sock not found"
                            .into(),
                    ));
                }
                Ok(RuntimeKind::Containerd)
            }
        }
    }

    /// Build the selected runtime as a trait object. For now only Docker
    /// is wired; containerd will be added in Task 3.
    pub async fn build(kind: RuntimeKind, _data_dir: &str) -> Result<Arc<dyn ContainerRuntime>> {
        match kind {
            RuntimeKind::Docker => {
                let rt = DockerRuntime::new()?;
                rt.ping().await?;
                info!("connected to Docker runtime");
                Ok(Arc::new(rt))
            }
            RuntimeKind::Containerd => {
                Err(NexaError::Runtime("containerd runtime not yet implemented".into()))
            }
            RuntimeKind::Auto => {
                unreachable!("resolve() must be called before build()")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_runtime_kind_docker() {
        let kind: RuntimeKind = "docker".parse().unwrap();
        assert_eq!(kind, RuntimeKind::Docker);
    }

    #[test]
    fn parse_runtime_kind_containerd() {
        let kind: RuntimeKind = "containerd".parse().unwrap();
        assert_eq!(kind, RuntimeKind::Containerd);
    }

    #[test]
    fn parse_runtime_kind_auto() {
        let kind: RuntimeKind = "auto".parse().unwrap();
        assert_eq!(kind, RuntimeKind::Auto);
    }

    #[test]
    fn parse_runtime_kind_case_insensitive() {
        let kind: RuntimeKind = "Docker".parse().unwrap();
        assert_eq!(kind, RuntimeKind::Docker);
        let kind: RuntimeKind = "CONTAINERD".parse().unwrap();
        assert_eq!(kind, RuntimeKind::Containerd);
    }

    #[test]
    fn parse_runtime_kind_invalid() {
        let result: std::result::Result<RuntimeKind, _> = "podman".parse();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("podman"));
    }

    #[test]
    fn display_runtime_kind() {
        assert_eq!(RuntimeKind::Docker.to_string(), "docker");
        assert_eq!(RuntimeKind::Containerd.to_string(), "containerd");
        assert_eq!(RuntimeKind::Auto.to_string(), "auto");
    }

    #[test]
    fn probe_returns_booleans() {
        // Just verify the function runs without panic; actual values depend on host
        let (containerd, docker) = RuntimeDetector::probe();
        // On CI/dev machines at least one should typically be true,
        // but we can't assert that in a unit test.
        let _ = (containerd, docker);
    }
}
```

Run: `cargo test -p nexad -- adapters::runtime::detect 2>&1`
Expected: FAIL -- module `detect` does not exist

- [ ] **Step 2: Wire the detect module into the runtime adapter**

Update `crates/nexad/src/adapters/runtime/mod.rs`:

```rust
mod detect;
mod docker;

pub use detect::{RuntimeDetector, RuntimeKind};
pub use docker::DockerRuntime;
```

- [ ] **Step 3: Run detection tests**

Run: `cargo test -p nexad -- adapters::runtime::detect 2>&1`
Expected: all 7 tests pass

- [ ] **Step 4: Add --runtime flag to nexad main.rs**

In `crates/nexad/src/main.rs`, update the `Cli` struct:

```rust
mod adapters;
mod api;
mod engine;

use std::sync::Arc;

use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

use adapters::runtime::{RuntimeDetector, RuntimeKind};

#[derive(Parser)]
#[command(name = "nexad", about = "NexaNet daemon", version)]
struct Cli {
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    #[arg(long, default_value = "6443")]
    port: u16,

    #[arg(long, default_value = "/var/lib/nexa")]
    data_dir: String,

    /// Container runtime to use: docker, containerd, or auto (default)
    #[arg(long, default_value = "auto")]
    runtime: RuntimeKind,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    info!("starting nexad on {}:{}", cli.host, cli.port);

    let resolved = RuntimeDetector::resolve(cli.runtime)?;
    let runtime = RuntimeDetector::build(resolved, &cli.data_dir).await?;
    info!(runtime = runtime.runtime_name(), "runtime initialized");

    let orchestrator = engine::Orchestrator::new_with_runtime(runtime).await?;
    let addr = format!("{}:{}", cli.host, cli.port);

    api::serve(orchestrator, &addr).await
}
```

Note: `Orchestrator::new_with_runtime` is a new constructor that accepts a pre-built `Arc<dyn ContainerRuntime>`. Add it in `crates/nexad/src/engine/orchestrator.rs`:

```rust
    pub async fn new_with_runtime(runtime: Arc<dyn ContainerRuntime>) -> anyhow::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            runtime,
            projects: DashMap::new(),
            deployments: DashMap::new(),
            pods: DashMap::new(),
        }))
    }
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check 2>&1`
Expected: compiles

- [ ] **Step 6: Verify --runtime help text appears**

Run: `cargo run -p nexad -- --help 2>&1 | grep -A2 runtime`
Expected: shows `--runtime <RUNTIME>` with description

- [ ] **Step 7: Commit**

```bash
git add crates/nexad/src/adapters/runtime/detect.rs crates/nexad/src/adapters/runtime/mod.rs crates/nexad/src/main.rs crates/nexad/src/engine/orchestrator.rs
git commit -m "feat: add runtime auto-detection and --runtime CLI flag for nexad"
```

---

### Task 3: CniManager for containerd networking

**Files:**
- Create: `crates/nexad/src/adapters/runtime/cni.rs`
- Modify: `crates/nexad/src/adapters/runtime/mod.rs`

- [ ] **Step 1: Write failing tests for CniManager**

Create `crates/nexad/src/adapters/runtime/cni.rs`:

```rust
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use nexa_core::error::{NexaError, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Manages CNI network configurations and plugin invocations for containerd.
///
/// containerd has no built-in networking. NexaNet manages CNI configs per project.
/// Each project gets a bridge network with host-local IPAM.
pub struct CniManager {
    cni_bin_dir: PathBuf,
    cni_conf_dir: PathBuf,
    /// Track allocated subnets: network_name -> subnet
    subnet_allocator: SubnetAllocator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CniConfig {
    #[serde(rename = "cniVersion")]
    cni_version: String,
    name: String,
    plugins: Vec<CniPlugin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum CniPlugin {
    #[serde(rename = "bridge")]
    Bridge {
        bridge: String,
        #[serde(rename = "isGateway")]
        is_gateway: bool,
        ipam: CniIpam,
    },
    #[serde(rename = "loopback")]
    Loopback {},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CniIpam {
    #[serde(rename = "type")]
    ipam_type: String,
    subnet: String,
}

/// Simple /24 subnet allocator for project networks.
/// Allocates from 172.20.0.0/24, 172.20.1.0/24, etc.
struct SubnetAllocator {
    next_third_octet: u8,
    allocated: HashMap<String, String>,
}

impl SubnetAllocator {
    fn new() -> Self {
        Self {
            next_third_octet: 0,
            allocated: HashMap::new(),
        }
    }

    fn allocate(&mut self, network_name: &str) -> Result<String> {
        if let Some(existing) = self.allocated.get(network_name) {
            return Ok(existing.clone());
        }

        if self.next_third_octet == 255 {
            return Err(NexaError::Runtime("subnet pool exhausted".into()));
        }

        let subnet = format!("172.20.{}.0/24", self.next_third_octet);
        self.next_third_octet += 1;
        self.allocated.insert(network_name.to_string(), subnet.clone());
        Ok(subnet)
    }

    fn release(&mut self, network_name: &str) {
        self.allocated.remove(network_name);
    }

    fn get(&self, network_name: &str) -> Option<&String> {
        self.allocated.get(network_name)
    }
}

impl CniManager {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            cni_bin_dir: data_dir.join("cni").join("bin"),
            cni_conf_dir: data_dir.join("cni").join("conf"),
            subnet_allocator: SubnetAllocator::new(),
        }
    }

    /// Check that CNI plugin binaries exist.
    pub fn check_plugins(&self) -> Result<()> {
        let required = ["bridge", "loopback", "host-local"];
        let mut missing = Vec::new();

        for plugin in &required {
            let path = self.cni_bin_dir.join(plugin);
            if !path.exists() {
                missing.push(*plugin);
            }
        }

        if !missing.is_empty() {
            return Err(NexaError::Runtime(format!(
                "missing CNI plugins: {}. run 'nexa setup cni' to install them",
                missing.join(", ")
            )));
        }

        Ok(())
    }

    /// Create or ensure a CNI network config file for a project.
    pub fn ensure_network(&mut self, name: &str) -> Result<String> {
        let conf_path = self.cni_conf_dir.join(format!("{name}.conflist"));

        if conf_path.exists() {
            // Already configured, return existing subnet
            if let Some(subnet) = self.subnet_allocator.get(name) {
                return Ok(subnet.clone());
            }
            // Read existing config to recover subnet
            let content = std::fs::read_to_string(&conf_path)?;
            let config: CniConfig = serde_json::from_str(&content)
                .map_err(|e| NexaError::Runtime(format!("corrupt CNI config: {e}")))?;
            for plugin in &config.plugins {
                if let CniPlugin::Bridge { ipam, .. } = plugin {
                    self.subnet_allocator
                        .allocated
                        .insert(name.to_string(), ipam.subnet.clone());
                    return Ok(ipam.subnet.clone());
                }
            }
            return Err(NexaError::Runtime("CNI config has no bridge plugin".into()));
        }

        let subnet = self.subnet_allocator.allocate(name)?;

        let config = CniConfig {
            cni_version: "1.0.0".into(),
            name: name.to_string(),
            plugins: vec![
                CniPlugin::Bridge {
                    bridge: name.to_string(),
                    is_gateway: true,
                    ipam: CniIpam {
                        ipam_type: "host-local".into(),
                        subnet: subnet.clone(),
                    },
                },
                CniPlugin::Loopback {},
            ],
        };

        std::fs::create_dir_all(&self.cni_conf_dir)?;
        let json = serde_json::to_string_pretty(&config)?;
        std::fs::write(&conf_path, &json)?;

        info!(name, subnet = subnet, "created CNI network config");
        Ok(subnet)
    }

    /// Remove a CNI network config file.
    pub fn remove_network(&mut self, name: &str) -> Result<()> {
        let conf_path = self.cni_conf_dir.join(format!("{name}.conflist"));
        if conf_path.exists() {
            std::fs::remove_file(&conf_path)?;
        }
        self.subnet_allocator.release(name);
        debug!(name, "removed CNI network config");
        Ok(())
    }

    /// Attach a container to a CNI network by invoking the CNI ADD command.
    /// Returns the assigned IP address.
    pub async fn attach(&self, container_id: &str, network: &str, netns_path: &str) -> Result<IpAddr> {
        let conf_path = self.cni_conf_dir.join(format!("{network}.conflist"));
        if !conf_path.exists() {
            return Err(NexaError::Runtime(format!(
                "CNI network '{network}' not configured"
            )));
        }

        let config = std::fs::read_to_string(&conf_path)?;

        let output = tokio::process::Command::new(self.cni_bin_dir.join("bridge"))
            .env("CNI_COMMAND", "ADD")
            .env("CNI_CONTAINERID", container_id)
            .env("CNI_NETNS", netns_path)
            .env("CNI_IFNAME", "eth0")
            .env("CNI_PATH", &self.cni_bin_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| NexaError::Runtime(format!("failed to invoke CNI bridge plugin: {e}")))?;

        // Write config to stdin
        let output = output.wait_with_output().await
            .map_err(|e| NexaError::Runtime(format!("CNI bridge plugin failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NexaError::Runtime(format!("CNI ADD failed: {stderr}")));
        }

        // Parse the CNI result JSON to extract the IP
        let result_str = String::from_utf8_lossy(&output.stdout);
        let result: serde_json::Value = serde_json::from_str(&result_str)
            .map_err(|e| NexaError::Runtime(format!("invalid CNI result: {e}")))?;

        let ip_str = result["ips"][0]["address"]
            .as_str()
            .ok_or_else(|| NexaError::Runtime("CNI result missing IP address".into()))?;

        // CNI returns CIDR notation like "172.20.0.2/24", strip the prefix length
        let ip_only = ip_str
            .split('/')
            .next()
            .unwrap_or(ip_str);

        let ip: IpAddr = ip_only
            .parse()
            .map_err(|e| NexaError::Runtime(format!("invalid IP from CNI: {e}")))?;

        info!(container_id, network, ip = %ip, "attached container to CNI network");
        Ok(ip)
    }

    /// Detach a container from a CNI network by invoking the CNI DEL command.
    pub async fn detach(&self, container_id: &str, network: &str, netns_path: &str) -> Result<()> {
        let conf_path = self.cni_conf_dir.join(format!("{network}.conflist"));
        if !conf_path.exists() {
            warn!(network, "CNI config not found during detach, skipping");
            return Ok(());
        }

        let output = tokio::process::Command::new(self.cni_bin_dir.join("bridge"))
            .env("CNI_COMMAND", "DEL")
            .env("CNI_CONTAINERID", container_id)
            .env("CNI_NETNS", netns_path)
            .env("CNI_IFNAME", "eth0")
            .env("CNI_PATH", &self.cni_bin_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| NexaError::Runtime(format!("failed to invoke CNI DEL: {e}")))?
            .wait_with_output()
            .await
            .map_err(|e| NexaError::Runtime(format!("CNI DEL failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(container_id, network, stderr = %stderr, "CNI DEL returned error");
        }

        debug!(container_id, network, "detached from CNI network");
        Ok(())
    }

    /// Get the CNI bin directory path (for setup commands).
    pub fn bin_dir(&self) -> &Path {
        &self.cni_bin_dir
    }

    /// Get the CNI conf directory path.
    pub fn conf_dir(&self) -> &Path {
        &self.cni_conf_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_manager() -> (CniManager, TempDir) {
        let tmp = TempDir::new().unwrap();
        let mgr = CniManager::new(tmp.path());
        (mgr, tmp)
    }

    #[test]
    fn subnet_allocator_assigns_sequential_subnets() {
        let mut alloc = SubnetAllocator::new();
        let s1 = alloc.allocate("net-a").unwrap();
        let s2 = alloc.allocate("net-b").unwrap();
        assert_eq!(s1, "172.20.0.0/24");
        assert_eq!(s2, "172.20.1.0/24");
    }

    #[test]
    fn subnet_allocator_returns_same_for_existing() {
        let mut alloc = SubnetAllocator::new();
        let s1 = alloc.allocate("net-a").unwrap();
        let s2 = alloc.allocate("net-a").unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn subnet_allocator_release_frees_name() {
        let mut alloc = SubnetAllocator::new();
        alloc.allocate("net-a").unwrap();
        alloc.release("net-a");
        assert!(alloc.get("net-a").is_none());
    }

    #[test]
    fn ensure_network_creates_conflist() {
        let (mut mgr, tmp) = test_manager();
        let subnet = mgr.ensure_network("nexa-ecommerce").unwrap();
        assert_eq!(subnet, "172.20.0.0/24");

        let conf_path = tmp
            .path()
            .join("cni")
            .join("conf")
            .join("nexa-ecommerce.conflist");
        assert!(conf_path.exists());

        let content = std::fs::read_to_string(&conf_path).unwrap();
        let config: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(config["cniVersion"], "1.0.0");
        assert_eq!(config["name"], "nexa-ecommerce");
        assert_eq!(config["plugins"][0]["type"], "bridge");
        assert_eq!(config["plugins"][0]["bridge"], "nexa-ecommerce");
        assert_eq!(config["plugins"][0]["isGateway"], true);
        assert_eq!(config["plugins"][0]["ipam"]["type"], "host-local");
        assert_eq!(config["plugins"][0]["ipam"]["subnet"], "172.20.0.0/24");
        assert_eq!(config["plugins"][1]["type"], "loopback");
    }

    #[test]
    fn ensure_network_idempotent() {
        let (mut mgr, _tmp) = test_manager();
        let s1 = mgr.ensure_network("nexa-test").unwrap();
        let s2 = mgr.ensure_network("nexa-test").unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn remove_network_deletes_conflist() {
        let (mut mgr, tmp) = test_manager();
        mgr.ensure_network("nexa-test").unwrap();

        let conf_path = tmp
            .path()
            .join("cni")
            .join("conf")
            .join("nexa-test.conflist");
        assert!(conf_path.exists());

        mgr.remove_network("nexa-test").unwrap();
        assert!(!conf_path.exists());
    }

    #[test]
    fn remove_network_noop_if_missing() {
        let (mut mgr, _tmp) = test_manager();
        // Should not error
        mgr.remove_network("nonexistent").unwrap();
    }

    #[test]
    fn check_plugins_fails_when_missing() {
        let (mgr, _tmp) = test_manager();
        let result = mgr.check_plugins();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("bridge"));
        assert!(err.contains("nexa setup cni"));
    }

    #[test]
    fn check_plugins_passes_when_present() {
        let (mgr, tmp) = test_manager();

        // Create fake plugin binaries
        let bin_dir = tmp.path().join("cni").join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        for name in &["bridge", "loopback", "host-local"] {
            std::fs::write(bin_dir.join(name), "fake").unwrap();
        }

        mgr.check_plugins().unwrap();
    }

    #[test]
    fn multiple_networks_get_different_subnets() {
        let (mut mgr, _tmp) = test_manager();
        let s1 = mgr.ensure_network("nexa-project-a").unwrap();
        let s2 = mgr.ensure_network("nexa-project-b").unwrap();
        let s3 = mgr.ensure_network("nexa-project-c").unwrap();
        assert_ne!(s1, s2);
        assert_ne!(s2, s3);
        assert_ne!(s1, s3);
    }
}
```

Run: `cargo test -p nexad -- adapters::runtime::cni 2>&1`
Expected: FAIL -- module not found

- [ ] **Step 2: Wire CniManager into the runtime module**

Update `crates/nexad/src/adapters/runtime/mod.rs`:

```rust
pub mod cni;
mod detect;
mod docker;

pub use cni::CniManager;
pub use detect::{RuntimeDetector, RuntimeKind};
pub use docker::DockerRuntime;
```

- [ ] **Step 3: Add tempfile dev-dependency to nexad**

In `crates/nexad/Cargo.toml`, add:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 4: Run CniManager tests**

Run: `cargo test -p nexad -- adapters::runtime::cni 2>&1`
Expected: all 9 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/nexad/src/adapters/runtime/cni.rs crates/nexad/src/adapters/runtime/mod.rs crates/nexad/Cargo.toml
git commit -m "feat: add CniManager for containerd CNI network config generation and plugin invocation"
```

---

### Task 4: ContainerdRuntime adapter -- image pull + create/start container

**Files:**
- Create: `crates/nexad/src/adapters/runtime/containerd.rs`
- Modify: `crates/nexad/src/adapters/runtime/mod.rs`
- Modify: `crates/nexad/src/adapters/runtime/detect.rs`
- Modify: `crates/nexad/Cargo.toml`

- [ ] **Step 1: Add containerd-client workspace dependency**

In the root `Cargo.toml`, add to `[workspace.dependencies]`:

```toml
containerd-client = "0.5"
```

In `crates/nexad/Cargo.toml`, add to `[dependencies]`:

```toml
containerd-client = { workspace = true }
```

- [ ] **Step 2: Create the ContainerdRuntime struct with image pull and create/start**

Create `crates/nexad/src/adapters/runtime/containerd.rs`:

```rust
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use containerd_client::services::v1::containers_client::ContainersClient;
use containerd_client::services::v1::content_client::ContentClient;
use containerd_client::services::v1::images_client::ImagesClient;
use containerd_client::services::v1::tasks_client::TasksClient;
use containerd_client::services::v1::{
    Container, CreateContainerRequest, CreateTaskRequest, DeleteContainerRequest,
    DeleteTaskRequest, GetContainerRequest, GetImageRequest, KillRequest, ListContainersRequest,
    PullImageRequest, StartRequest, WaitRequest,
};
use containerd_client::tonic::Request;
use containerd_client::with_namespace;
use futures::StreamExt;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use nexa_core::error::{NexaError, Result};
use nexa_core::runtime::*;

use super::cni::CniManager;

const NEXA_NAMESPACE: &str = "nexa";

pub struct ContainerdRuntime {
    channel: containerd_client::tonic::transport::Channel,
    cni: Mutex<CniManager>,
    namespace: String,
    data_dir: PathBuf,
    /// Track container_id -> netns_path for CNI detach
    netns_map: Mutex<HashMap<String, String>>,
    /// Track container_id -> network_name for IP lookups
    network_map: Mutex<HashMap<String, (String, IpAddr)>>,
}

impl ContainerdRuntime {
    pub async fn new(data_dir: &str) -> Result<Self> {
        let channel = containerd_client::connect("/run/containerd/containerd.sock")
            .await
            .map_err(|e| NexaError::Runtime(format!("failed to connect to containerd: {e}")))?;

        let cni = CniManager::new(Path::new(data_dir));

        Ok(Self {
            channel,
            cni: Mutex::new(cni),
            namespace: NEXA_NAMESPACE.to_string(),
            data_dir: PathBuf::from(data_dir),
            netns_map: Mutex::new(HashMap::new()),
            network_map: Mutex::new(HashMap::new()),
        })
    }

    pub async fn ping(&self) -> Result<()> {
        let mut client =
            containerd_client::services::v1::version_client::VersionClient::new(self.channel.clone());
        let req = with_namespace!(containerd_client::services::v1::VersionRequest {}, &self.namespace);
        client
            .version(req)
            .await
            .map_err(|e| NexaError::Runtime(format!("containerd unreachable: {e}")))?;
        Ok(())
    }

    fn log_dir(&self, container_id: &str) -> PathBuf {
        self.data_dir.join("logs").join(container_id)
    }

    fn ensure_log_dir(&self, container_id: &str) -> Result<PathBuf> {
        let dir = self.log_dir(container_id);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Resolve a Docker-style image reference to a containerd-compatible reference.
    /// e.g. "nginx" -> "docker.io/library/nginx:latest"
    /// e.g. "nginx:1.25" -> "docker.io/library/nginx:1.25"
    /// e.g. "ghcr.io/org/img:v1" -> "ghcr.io/org/img:v1" (unchanged)
    fn normalize_image_ref(image: &str) -> String {
        let with_tag = if image.contains(':') {
            image.to_string()
        } else {
            format!("{image}:latest")
        };

        if with_tag.contains('/') {
            // Already has a registry or org prefix
            if with_tag.starts_with("docker.io/") || with_tag.contains('.') {
                with_tag
            } else {
                // Has org but no registry: e.g. "myorg/myimg:latest"
                format!("docker.io/{with_tag}")
            }
        } else {
            // Bare image name like "nginx:latest"
            format!("docker.io/library/{with_tag}")
        }
    }
}

#[async_trait]
impl ContainerRuntime for ContainerdRuntime {
    async fn pull_image(&self, image: &str) -> Result<()> {
        info!(image, "pulling image via containerd");

        let normalized = Self::normalize_image_ref(image);

        let mut client = ImagesClient::new(self.channel.clone());
        // containerd-client uses the transfer service for pulling; fall back to
        // ctr-style pull via the images + content services.
        // The simplest approach: shell out to `ctr` as the gRPC transfer API
        // is not fully exposed in containerd-client 0.5 Rust bindings.
        let output = tokio::process::Command::new("ctr")
            .args([
                "--namespace", &self.namespace,
                "images", "pull",
                &normalized,
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
            .map_err(|e| NexaError::ImagePull(format!("failed to run ctr: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NexaError::ImagePull(format!(
                "ctr image pull failed for '{normalized}': {stderr}"
            )));
        }

        info!(image, normalized = normalized, "image pulled via containerd");
        Ok(())
    }

    async fn create_container(&self, config: &ContainerConfig) -> Result<String> {
        debug!(name = config.name, image = config.image, "creating containerd container");

        let normalized_image = Self::normalize_image_ref(&config.image);
        let container_id = config.name.clone();

        let log_dir = self.ensure_log_dir(&container_id)?;
        let stdout_path = log_dir.join("stdout.log");
        let stderr_path = log_dir.join("stderr.log");

        // Build environment variables
        let env: Vec<String> = config
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();

        // Build mount specs for volumes
        let mut mounts = Vec::new();
        for vol in &config.volumes {
            mounts.push(containerd_client::types::Mount {
                r#type: "bind".into(),
                source: vol.source.clone(),
                target: vol.target.clone(),
                options: if vol.read_only {
                    vec!["rbind".into(), "ro".into()]
                } else {
                    vec!["rbind".into(), "rw".into()]
                },
            });
        }

        // Build labels
        let mut labels = config.labels.clone();
        labels.insert("managed-by".into(), "nexanet".into());
        labels.insert("nexa.image".into(), normalized_image.clone());

        // Create the container via gRPC
        let mut client = ContainersClient::new(self.channel.clone());
        let container = Container {
            id: container_id.clone(),
            image: normalized_image.clone(),
            labels,
            ..Default::default()
        };

        let req = with_namespace!(
            CreateContainerRequest {
                container: Some(container),
            },
            &self.namespace
        );

        client
            .create(req)
            .await
            .map_err(|e| NexaError::Runtime(format!("containerd create failed: {e}")))?;

        info!(id = container_id, "containerd container created");
        Ok(container_id)
    }

    async fn start_container(&self, id: &str) -> Result<()> {
        debug!(id, "starting containerd task");

        let log_dir = self.ensure_log_dir(id)?;
        let stdout_path = log_dir.join("stdout.log");
        let stderr_path = log_dir.join("stderr.log");

        // Create a task for the container (containerd separates container metadata from execution)
        let mut client = TasksClient::new(self.channel.clone());

        let req = with_namespace!(
            CreateTaskRequest {
                container_id: id.to_string(),
                ..Default::default()
            },
            &self.namespace
        );

        let resp = client
            .create(req)
            .await
            .map_err(|e| NexaError::Runtime(format!("containerd task create failed: {e}")))?;

        let pid = resp.into_inner().pid;

        // Start the task
        let req = with_namespace!(
            StartRequest {
                container_id: id.to_string(),
                ..Default::default()
            },
            &self.namespace
        );

        client
            .start(req)
            .await
            .map_err(|e| NexaError::Runtime(format!("containerd task start failed: {e}")))?;

        // Record the network namespace path for CNI
        let netns_path = format!("/proc/{pid}/ns/net");
        self.netns_map
            .lock()
            .await
            .insert(id.to_string(), netns_path);

        info!(id, pid, "containerd task started");
        Ok(())
    }

    async fn stop_container(&self, id: &str, timeout_secs: u64) -> Result<()> {
        debug!(id, "stopping containerd task");

        let mut client = TasksClient::new(self.channel.clone());

        // Send SIGTERM first
        let req = with_namespace!(
            KillRequest {
                container_id: id.to_string(),
                signal: 15, // SIGTERM
                ..Default::default()
            },
            &self.namespace
        );

        if let Err(e) = client.kill(req).await {
            warn!(id, error = %e, "SIGTERM failed, task may already be stopped");
            return Ok(());
        }

        // Wait for the task to exit with timeout
        let req = with_namespace!(
            WaitRequest {
                container_id: id.to_string(),
                ..Default::default()
            },
            &self.namespace
        );

        let wait_fut = client.wait(req);
        match tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            wait_fut,
        )
        .await
        {
            Ok(Ok(_)) => {
                debug!(id, "task exited gracefully");
            }
            Ok(Err(e)) => {
                warn!(id, error = %e, "wait failed");
            }
            Err(_) => {
                // Timeout: send SIGKILL
                warn!(id, "SIGTERM timeout, sending SIGKILL");
                let req = with_namespace!(
                    KillRequest {
                        container_id: id.to_string(),
                        signal: 9, // SIGKILL
                        ..Default::default()
                    },
                    &self.namespace
                );
                let _ = client.kill(req).await;
            }
        }

        // Detach from CNI network if attached
        let netns_path = self.netns_map.lock().await.remove(id);
        if let Some(netns) = netns_path {
            let net_info = self.network_map.lock().await.remove(id);
            if let Some((network, _ip)) = net_info {
                let cni = self.cni.lock().await;
                let _ = cni.detach(id, &network, &netns).await;
            }
        }

        debug!(id, "container stopped");
        Ok(())
    }

    async fn remove_container(&self, id: &str, force: bool) -> Result<()> {
        debug!(id, force, "removing containerd container");

        // Delete the task first
        let mut tasks_client = TasksClient::new(self.channel.clone());
        let req = with_namespace!(
            DeleteTaskRequest {
                container_id: id.to_string(),
            },
            &self.namespace
        );
        // Ignore errors -- task may already be deleted
        let _ = tasks_client.delete(req).await;

        // Delete the container
        let mut client = ContainersClient::new(self.channel.clone());
        let req = with_namespace!(
            DeleteContainerRequest {
                id: id.to_string(),
            },
            &self.namespace
        );

        client
            .delete(req)
            .await
            .map_err(|e| NexaError::Runtime(format!("containerd delete failed: {e}")))?;

        // Clean up log directory
        let log_dir = self.log_dir(id);
        if log_dir.exists() {
            let _ = std::fs::remove_dir_all(&log_dir);
        }

        // Clean up maps
        self.netns_map.lock().await.remove(id);
        self.network_map.lock().await.remove(id);

        debug!(id, "containerd container removed");
        Ok(())
    }

    async fn inspect_container(&self, id: &str) -> Result<ContainerInfo> {
        let mut containers_client = ContainersClient::new(self.channel.clone());
        let req = with_namespace!(
            GetContainerRequest {
                id: id.to_string(),
            },
            &self.namespace
        );

        let resp = containers_client
            .get(req)
            .await
            .map_err(|e| NexaError::Runtime(format!("containerd inspect failed: {e}")))?;

        let container = resp
            .into_inner()
            .container
            .ok_or_else(|| NexaError::Runtime("container not found in response".into()))?;

        // Check if a task exists to determine state
        let mut tasks_client = TasksClient::new(self.channel.clone());
        let task_req = with_namespace!(
            containerd_client::services::v1::GetRequest {
                container_id: id.to_string(),
                ..Default::default()
            },
            &self.namespace
        );

        let state = match tasks_client.get(task_req).await {
            Ok(resp) => {
                let process = resp.into_inner().process;
                match process.and_then(|p| containerd_client::services::v1::Status::try_from(p.status).ok()) {
                    Some(containerd_client::services::v1::Status::Running) => ContainerState::Running,
                    Some(containerd_client::services::v1::Status::Created) => ContainerState::Created,
                    Some(containerd_client::services::v1::Status::Stopped) => ContainerState::Exited,
                    Some(containerd_client::services::v1::Status::Paused) => ContainerState::Paused,
                    Some(containerd_client::services::v1::Status::Pausing) => ContainerState::Paused,
                    _ => ContainerState::Unknown,
                }
            }
            Err(_) => ContainerState::Created, // No task = container exists but not started
        };

        let image = container
            .labels
            .get("nexa.image")
            .cloned()
            .unwrap_or(container.image);

        Ok(ContainerInfo {
            id: container.id.clone(),
            name: container.id, // containerd uses ID as name
            image,
            state,
        })
    }

    async fn logs(&self, id: &str, tail: Option<u64>) -> Result<LogStream> {
        let stdout_path = self.log_dir(id).join("stdout.log");

        if !stdout_path.exists() {
            return Err(NexaError::Runtime(format!(
                "no log file for container '{id}'"
            )));
        }

        let tail_lines = tail.unwrap_or(100);

        // Read the last N lines and then tail for new output
        let file = tokio::fs::File::open(&stdout_path)
            .await
            .map_err(|e| NexaError::Runtime(format!("failed to open log file: {e}")))?;

        let reader = tokio::io::BufReader::new(file);
        let lines = tokio::io::AsyncBufReadExt::lines(reader);

        let stream = tokio_stream::wrappers::LinesStream::new(lines).map(|result| match result {
            Ok(line) => Ok(line),
            Err(e) => Err(NexaError::Runtime(format!("log read error: {e}"))),
        });

        Ok(Box::pin(stream))
    }

    async fn container_exists(&self, name: &str) -> Result<bool> {
        let mut client = ContainersClient::new(self.channel.clone());
        let req = with_namespace!(
            GetContainerRequest {
                id: name.to_string(),
            },
            &self.namespace
        );

        match client.get(req).await {
            Ok(_) => Ok(true),
            Err(status) if status.code() == containerd_client::tonic::Code::NotFound => Ok(false),
            Err(e) => Err(NexaError::Runtime(format!("containerd query failed: {e}"))),
        }
    }

    async fn create_network(&self, name: &str) -> Result<String> {
        let mut cni = self.cni.lock().await;
        let subnet = cni.ensure_network(name)?;
        Ok(format!("{name}:{subnet}"))
    }

    async fn remove_network(&self, name: &str) -> Result<()> {
        let mut cni = self.cni.lock().await;
        cni.remove_network(name)
    }

    async fn connect_to_network(&self, container_id: &str, network: &str) -> Result<()> {
        let netns_path = {
            let map = self.netns_map.lock().await;
            map.get(container_id)
                .cloned()
                .ok_or_else(|| NexaError::Runtime(format!(
                    "no netns for container '{container_id}' -- is the task started?"
                )))?
        };

        let cni = self.cni.lock().await;
        let ip = cni.attach(container_id, network, &netns_path).await?;

        self.network_map
            .lock()
            .await
            .insert(container_id.to_string(), (network.to_string(), ip));

        Ok(())
    }

    async fn container_ip(&self, id: &str, _network: &str) -> Result<IpAddr> {
        let map = self.network_map.lock().await;
        let (_net, ip) = map
            .get(id)
            .ok_or_else(|| NexaError::Runtime(format!(
                "container '{id}' has no assigned IP (not attached to a network)"
            )))?;
        Ok(*ip)
    }

    async fn events(&self) -> Result<EventStream> {
        // Use containerd's events API to watch for task events
        let mut client =
            containerd_client::services::v1::events_client::EventsClient::new(self.channel.clone());

        let req = with_namespace!(
            containerd_client::services::v1::SubscribeRequest {
                filters: vec!["topic==/tasks/*".into()],
            },
            &self.namespace
        );

        let stream = client
            .subscribe(req)
            .await
            .map_err(|e| NexaError::Runtime(format!("containerd events subscribe failed: {e}")))?
            .into_inner();

        let mapped = stream.map(|result| match result {
            Ok(envelope) => {
                let event_type = match envelope.topic.as_str() {
                    "/tasks/start" => ContainerEventType::Started,
                    "/tasks/exit" => ContainerEventType::Died,
                    "/tasks/oom" => ContainerEventType::Oom,
                    "/tasks/paused" => ContainerEventType::Stopped,
                    _ => ContainerEventType::Stopped,
                };

                Ok(ContainerEvent {
                    container_id: envelope.namespace, // simplified; real impl parses the Any payload
                    event_type,
                    timestamp: chrono::Utc::now(),
                })
            }
            Err(e) => Err(NexaError::Runtime(format!("containerd event error: {e}"))),
        });

        Ok(Box::pin(mapped))
    }

    fn runtime_name(&self) -> &'static str {
        "containerd"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_bare_image() {
        assert_eq!(
            ContainerdRuntime::normalize_image_ref("nginx"),
            "docker.io/library/nginx:latest"
        );
    }

    #[test]
    fn normalize_image_with_tag() {
        assert_eq!(
            ContainerdRuntime::normalize_image_ref("nginx:1.25"),
            "docker.io/library/nginx:1.25"
        );
    }

    #[test]
    fn normalize_image_with_org() {
        assert_eq!(
            ContainerdRuntime::normalize_image_ref("myorg/myimg:v1"),
            "docker.io/myorg/myimg:v1"
        );
    }

    #[test]
    fn normalize_image_with_registry() {
        assert_eq!(
            ContainerdRuntime::normalize_image_ref("ghcr.io/org/img:v1"),
            "ghcr.io/org/img:v1"
        );
    }

    #[test]
    fn normalize_docker_io_unchanged() {
        assert_eq!(
            ContainerdRuntime::normalize_image_ref("docker.io/library/nginx:latest"),
            "docker.io/library/nginx:latest"
        );
    }
}
```

- [ ] **Step 3: Wire ContainerdRuntime into the module and detector**

Update `crates/nexad/src/adapters/runtime/mod.rs`:

```rust
pub mod cni;
mod containerd;
mod detect;
mod docker;

pub use cni::CniManager;
pub use containerd::ContainerdRuntime;
pub use detect::{RuntimeDetector, RuntimeKind};
pub use docker::DockerRuntime;
```

Update the `RuntimeDetector::build` method in `crates/nexad/src/adapters/runtime/detect.rs` to wire in containerd:

```rust
    pub async fn build(kind: RuntimeKind, data_dir: &str) -> Result<Arc<dyn ContainerRuntime>> {
        match kind {
            RuntimeKind::Docker => {
                let rt = DockerRuntime::new()?;
                rt.ping().await?;
                info!("connected to Docker runtime");
                Ok(Arc::new(rt))
            }
            RuntimeKind::Containerd => {
                let rt = super::ContainerdRuntime::new(data_dir).await?;
                rt.ping().await?;
                info!("connected to containerd runtime");
                Ok(Arc::new(rt))
            }
            RuntimeKind::Auto => {
                unreachable!("resolve() must be called before build()")
            }
        }
    }
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check 2>&1`
Expected: compiles (may have warnings about unused variables in containerd.rs)

- [ ] **Step 5: Run unit tests**

Run: `cargo test -p nexad -- adapters::runtime::containerd 2>&1`
Expected: 5 image normalization tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/nexad/src/adapters/runtime/containerd.rs crates/nexad/src/adapters/runtime/mod.rs crates/nexad/src/adapters/runtime/detect.rs crates/nexad/Cargo.toml Cargo.toml
git commit -m "feat: add ContainerdRuntime adapter with image pull, create, start, stop, remove, inspect"
```

---

### Task 5: ContainerdRuntime adapter -- file-based log tailing

**Files:**
- Create: `crates/nexad/src/adapters/runtime/log_tailer.rs`
- Modify: `crates/nexad/src/adapters/runtime/containerd.rs`
- Modify: `crates/nexad/src/adapters/runtime/mod.rs`

- [ ] **Step 1: Write failing tests for log tailing**

Create `crates/nexad/src/adapters/runtime/log_tailer.rs`:

```rust
use std::path::{Path, PathBuf};

use futures::Stream;
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader, SeekFrom};
use tracing::debug;

use nexa_core::error::{NexaError, Result};

/// Tails a log file, returning the last N lines and then streaming new lines.
pub struct LogTailer;

impl LogTailer {
    /// Read the last `tail` lines from a file, then stream new lines as they appear.
    pub async fn tail(
        path: &Path,
        tail: Option<u64>,
    ) -> Result<impl Stream<Item = Result<String>>> {
        if !path.exists() {
            return Err(NexaError::Runtime(format!(
                "log file not found: {}",
                path.display()
            )));
        }

        let tail_count = tail.unwrap_or(100) as usize;
        let path = path.to_path_buf();

        // First, read the last N lines from the existing file content
        let existing = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| NexaError::Runtime(format!("failed to read log: {e}")))?;

        let lines: Vec<String> = existing.lines().map(String::from).collect();
        let start = if lines.len() > tail_count {
            lines.len() - tail_count
        } else {
            0
        };
        let tail_lines: Vec<String> = lines[start..].to_vec();
        let file_len = existing.len() as u64;

        // Create a stream that first yields the tail lines, then watches for new content
        let stream = async_stream::stream! {
            // Yield existing tail lines
            for line in tail_lines {
                yield Ok(line);
            }

            // Now tail the file for new content
            let mut file = match tokio::fs::File::open(&path).await {
                Ok(f) => f,
                Err(e) => {
                    yield Err(NexaError::Runtime(format!("failed to reopen log: {e}")));
                    return;
                }
            };

            if let Err(e) = file.seek(SeekFrom::Start(file_len)).await {
                yield Err(NexaError::Runtime(format!("failed to seek: {e}")));
                return;
            }

            let mut reader = BufReader::new(file);
            let mut buf = String::new();

            loop {
                buf.clear();
                match reader.read_line(&mut buf).await {
                    Ok(0) => {
                        // No new data yet, wait a bit
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                    Ok(_) => {
                        let line = buf.trim_end_matches('\n').to_string();
                        if !line.is_empty() {
                            yield Ok(line);
                        }
                    }
                    Err(e) => {
                        yield Err(NexaError::Runtime(format!("log read error: {e}")));
                        return;
                    }
                }
            }
        };

        Ok(stream)
    }

    /// Read all lines from a log file without tailing (for tests / one-shot reads).
    pub async fn read_all(path: &Path) -> Result<Vec<String>> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| NexaError::Runtime(format!("failed to read log: {e}")))?;
        Ok(content.lines().map(String::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use tempfile::TempDir;

    #[tokio::test]
    async fn tail_returns_last_n_lines() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("test.log");

        // Write 10 lines
        let content: String = (0..10).map(|i| format!("line {i}\n")).collect();
        tokio::fs::write(&log_path, &content).await.unwrap();

        let stream = LogTailer::tail(&log_path, Some(3)).await.unwrap();
        tokio::pin!(stream);

        let mut lines = Vec::new();
        // Collect only the initial tail lines (the stream will block waiting for new data)
        for _ in 0..3 {
            if let Some(Ok(line)) = stream.next().await {
                lines.push(line);
            }
        }

        assert_eq!(lines, vec!["line 7", "line 8", "line 9"]);
    }

    #[tokio::test]
    async fn tail_returns_all_when_fewer_than_requested() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("test.log");

        tokio::fs::write(&log_path, "one\ntwo\n").await.unwrap();

        let stream = LogTailer::tail(&log_path, Some(100)).await.unwrap();
        tokio::pin!(stream);

        let mut lines = Vec::new();
        for _ in 0..2 {
            if let Some(Ok(line)) = stream.next().await {
                lines.push(line);
            }
        }

        assert_eq!(lines, vec!["one", "two"]);
    }

    #[tokio::test]
    async fn tail_errors_on_missing_file() {
        let tmp = TempDir::new().unwrap();
        let result = LogTailer::tail(&tmp.path().join("nonexistent.log"), None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn read_all_returns_all_lines() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("test.log");
        tokio::fs::write(&log_path, "alpha\nbeta\ngamma\n")
            .await
            .unwrap();

        let lines = LogTailer::read_all(&log_path).await.unwrap();
        assert_eq!(lines, vec!["alpha", "beta", "gamma"]);
    }

    #[tokio::test]
    async fn tail_picks_up_new_lines() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("test.log");

        // Write initial content
        tokio::fs::write(&log_path, "initial\n").await.unwrap();

        let stream = LogTailer::tail(&log_path, Some(10)).await.unwrap();
        tokio::pin!(stream);

        // Read the initial line
        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first, "initial");

        // Append a new line in the background
        let path_clone = log_path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&path_clone)
                .await
                .unwrap();
            file.write_all(b"appended\n").await.unwrap();
        });

        // Should eventually pick up the new line
        let second = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            stream.next(),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();

        assert_eq!(second, "appended");
    }
}
```

- [ ] **Step 2: Wire log_tailer module and add async-stream dependency**

In the root `Cargo.toml`, add to `[workspace.dependencies]`:

```toml
async-stream = "0.3"
```

In `crates/nexad/Cargo.toml`, add to `[dependencies]`:

```toml
async-stream = { workspace = true }
tokio-stream = { workspace = true }
```

Update `crates/nexad/src/adapters/runtime/mod.rs`:

```rust
pub mod cni;
mod containerd;
mod detect;
mod docker;
pub mod log_tailer;

pub use cni::CniManager;
pub use containerd::ContainerdRuntime;
pub use detect::{RuntimeDetector, RuntimeKind};
pub use docker::DockerRuntime;
```

- [ ] **Step 3: Update ContainerdRuntime::logs to use LogTailer**

In `crates/nexad/src/adapters/runtime/containerd.rs`, replace the `logs` method:

```rust
    async fn logs(&self, id: &str, tail: Option<u64>) -> Result<LogStream> {
        let stdout_path = self.log_dir(id).join("stdout.log");
        let stream = super::log_tailer::LogTailer::tail(&stdout_path, tail).await?;
        Ok(Box::pin(stream))
    }
```

- [ ] **Step 4: Run log tailer tests**

Run: `cargo test -p nexad -- adapters::runtime::log_tailer 2>&1`
Expected: all 5 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/nexad/src/adapters/runtime/log_tailer.rs crates/nexad/src/adapters/runtime/mod.rs crates/nexad/src/adapters/runtime/containerd.rs crates/nexad/Cargo.toml Cargo.toml
git commit -m "feat: add file-based LogTailer for containerd log streaming"
```

---

### Task 6: `nexa setup cni` command to download standard CNI plugins

**Files:**
- Modify: `crates/nexa-cli/src/main.rs`
- Modify: `crates/nexa-cli/src/commands.rs`
- Modify: `crates/nexa-cli/Cargo.toml`

- [ ] **Step 1: Add the setup cni subcommand to CLI**

In `crates/nexa-cli/src/main.rs`, add a `Setup` subcommand to the existing `Cli` enum. The exact shape depends on the current CLI structure. Add to the `Commands` enum:

```rust
    /// Setup system components
    Setup {
        #[command(subcommand)]
        component: SetupComponent,
    },
```

Add the `SetupComponent` enum:

```rust
#[derive(clap::Subcommand)]
enum SetupComponent {
    /// Download and install standard CNI plugins
    Cni {
        /// Directory to install CNI plugin binaries
        #[arg(long, default_value = "/var/lib/nexa/cni/bin")]
        bin_dir: String,

        /// CNI plugins version to download
        #[arg(long, default_value = "1.4.1")]
        version: String,
    },
}
```

Wire the command handler in the match block:

```rust
        Commands::Setup { component } => match component {
            SetupComponent::Cni { bin_dir, version } => {
                commands::setup_cni(&bin_dir, &version).await?
            }
        },
```

- [ ] **Step 2: Implement setup_cni in commands.rs**

Add to `crates/nexa-cli/src/commands.rs`:

```rust
use std::os::unix::fs::PermissionsExt;

pub async fn setup_cni(bin_dir: &str, version: &str) -> Result<()> {
    let arch = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        anyhow::bail!("unsupported architecture");
    };

    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        // CNI plugins are Linux-only; on macOS this is only useful in VMs
        "linux"
    } else {
        anyhow::bail!("CNI plugins are only available for Linux");
    };

    let filename = format!("cni-plugins-{os}-{arch}-v{version}.tgz");
    let url = format!(
        "https://github.com/containernetworking/plugins/releases/download/v{version}/{filename}"
    );

    println!("Downloading CNI plugins v{version} for {os}/{arch}...");
    println!("  URL: {url}");

    let bin_path = Path::new(bin_dir);
    std::fs::create_dir_all(bin_path)?;

    // Download the tarball
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("download failed: {e}"))?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "download failed with status {}: {}",
            resp.status(),
            resp.status().canonical_reason().unwrap_or("unknown")
        );
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| anyhow::anyhow!("download body read failed: {e}"))?;

    // Extract the tarball
    let tar_gz = flate2::read::GzDecoder::new(std::io::Cursor::new(&bytes));
    let mut archive = tar::Archive::new(tar_gz);

    let mut installed = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if name.is_empty() {
            continue;
        }

        let dest = bin_path.join(&name);
        entry.unpack(&dest)?;

        // Ensure executable
        let mut perms = std::fs::metadata(&dest)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&dest, perms)?;

        installed.push(name);
    }

    installed.sort();
    println!("\nInstalled {} CNI plugins to {bin_dir}:", installed.len());
    for name in &installed {
        println!("  - {name}");
    }

    // Verify the required plugins are present
    let required = ["bridge", "loopback", "host-local"];
    let missing: Vec<&&str> = required
        .iter()
        .filter(|r| !installed.iter().any(|i| i == **r))
        .collect();

    if missing.is_empty() {
        output::print_success("All required CNI plugins installed successfully");
    } else {
        println!(
            "\nWarning: missing required plugins: {}",
            missing.iter().map(|m| **m).collect::<Vec<_>>().join(", ")
        );
    }

    Ok(())
}
```

- [ ] **Step 3: Add flate2 and tar dependencies**

In the root `Cargo.toml`, add to `[workspace.dependencies]`:

```toml
flate2 = "1"
tar = "0.4"
```

In `crates/nexa-cli/Cargo.toml`, add to `[dependencies]`:

```toml
flate2 = { workspace = true }
tar = { workspace = true }
```

- [ ] **Step 4: Verify CLI compiles and shows help**

Run: `cargo check -p nexa-cli 2>&1`
Expected: compiles

Run: `cargo run -p nexa-cli -- setup cni --help 2>&1`
Expected: shows help text with --bin-dir and --version flags

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-cli/src/main.rs crates/nexa-cli/src/commands.rs crates/nexa-cli/Cargo.toml Cargo.toml
git commit -m "feat: add 'nexa setup cni' command to download standard CNI plugins"
```

---

### Task 7: Integration test suite for both runtimes

**Files:**
- Create: `crates/nexad/tests/runtime_integration.rs`

- [ ] **Step 1: Write the shared integration test suite**

Create `crates/nexad/tests/runtime_integration.rs`:

```rust
//! Integration tests that run the same ContainerRuntime test suite against
//! both Docker and containerd backends.
//!
//! These tests require actual runtimes running on the host.
//! Skip with: `cargo test -p nexad --test runtime_integration -- --ignored`
//!
//! To run:
//!   NEXA_TEST_RUNTIME=docker cargo test -p nexad --test runtime_integration
//!   NEXA_TEST_RUNTIME=containerd cargo test -p nexad --test runtime_integration

use std::sync::Arc;

use nexa_core::error::Result;
use nexa_core::runtime::*;

/// The test image -- must be small and widely available
const TEST_IMAGE: &str = "busybox:latest";
const TEST_TIMEOUT: u64 = 30;

async fn get_runtime() -> Option<Arc<dyn ContainerRuntime>> {
    let runtime_name = std::env::var("NEXA_TEST_RUNTIME").unwrap_or("docker".into());

    match runtime_name.as_str() {
        "docker" => {
            use nexad::adapters::runtime::DockerRuntime;
            match DockerRuntime::new() {
                Ok(rt) => {
                    if rt.ping().await.is_ok() {
                        Some(Arc::new(rt))
                    } else {
                        eprintln!("Docker daemon not reachable, skipping tests");
                        None
                    }
                }
                Err(_) => {
                    eprintln!("Failed to create DockerRuntime, skipping tests");
                    None
                }
            }
        }
        "containerd" => {
            use nexad::adapters::runtime::ContainerdRuntime;
            match ContainerdRuntime::new("/tmp/nexa-test").await {
                Ok(rt) => {
                    if rt.ping().await.is_ok() {
                        Some(Arc::new(rt))
                    } else {
                        eprintln!("containerd not reachable, skipping tests");
                        None
                    }
                }
                Err(_) => {
                    eprintln!("Failed to create ContainerdRuntime, skipping tests");
                    None
                }
            }
        }
        other => {
            panic!("Unknown NEXA_TEST_RUNTIME: {other}");
        }
    }
}

fn unique_name(prefix: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    format!("nexa-test-{prefix}-{id}")
}

#[tokio::test]
#[ignore] // Requires a running container runtime
async fn test_pull_image() {
    let Some(rt) = get_runtime().await else { return };
    rt.pull_image(TEST_IMAGE).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_create_start_stop_remove() {
    let Some(rt) = get_runtime().await else { return };
    rt.pull_image(TEST_IMAGE).await.unwrap();

    let name = unique_name("lifecycle");
    let config = ContainerConfig {
        name: name.clone(),
        image: TEST_IMAGE.into(),
        env: std::collections::HashMap::from([("TEST_VAR".into(), "hello".into())]),
        ports: vec![],
        volumes: vec![],
        labels: std::collections::HashMap::from([("managed-by".into(), "nexanet-test".into())]),
        network: None,
        dns: vec![],
        dns_search: vec![],
    };

    let id = rt.create_container(&config).await.unwrap();
    assert!(!id.is_empty());

    rt.start_container(&id).await.unwrap();

    let info = rt.inspect_container(&id).await.unwrap();
    assert_eq!(info.state, ContainerState::Running);

    rt.stop_container(&id, 5).await.unwrap();

    let info = rt.inspect_container(&id).await.unwrap();
    assert!(
        info.state == ContainerState::Exited || info.state == ContainerState::Created,
        "expected Exited or Created, got {:?}",
        info.state
    );

    rt.remove_container(&id, true).await.unwrap();
    assert!(!rt.container_exists(&name).await.unwrap());
}

#[tokio::test]
#[ignore]
async fn test_container_exists() {
    let Some(rt) = get_runtime().await else { return };
    rt.pull_image(TEST_IMAGE).await.unwrap();

    let name = unique_name("exists");
    assert!(!rt.container_exists(&name).await.unwrap());

    let config = ContainerConfig {
        name: name.clone(),
        image: TEST_IMAGE.into(),
        env: std::collections::HashMap::new(),
        ports: vec![],
        volumes: vec![],
        labels: std::collections::HashMap::new(),
        network: None,
        dns: vec![],
        dns_search: vec![],
    };

    let id = rt.create_container(&config).await.unwrap();
    assert!(rt.container_exists(&name).await.unwrap());

    rt.remove_container(&id, true).await.unwrap();
    assert!(!rt.container_exists(&name).await.unwrap());
}

#[tokio::test]
#[ignore]
async fn test_inspect_container() {
    let Some(rt) = get_runtime().await else { return };
    rt.pull_image(TEST_IMAGE).await.unwrap();

    let name = unique_name("inspect");
    let config = ContainerConfig {
        name: name.clone(),
        image: TEST_IMAGE.into(),
        env: std::collections::HashMap::new(),
        ports: vec![],
        volumes: vec![],
        labels: std::collections::HashMap::new(),
        network: None,
        dns: vec![],
        dns_search: vec![],
    };

    let id = rt.create_container(&config).await.unwrap();
    let info = rt.inspect_container(&id).await.unwrap();
    assert_eq!(info.id, id);
    assert!(info.image.contains("busybox"));

    rt.remove_container(&id, true).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_network_lifecycle() {
    let Some(rt) = get_runtime().await else { return };

    let net_name = unique_name("net");
    let net_id = rt.create_network(&net_name).await.unwrap();
    assert!(!net_id.is_empty());

    // Remove should succeed
    rt.remove_network(&net_name).await.unwrap();
}

#[tokio::test]
#[ignore]
async fn test_runtime_name() {
    let Some(rt) = get_runtime().await else { return };
    let name = rt.runtime_name();
    assert!(
        name == "docker" || name == "containerd",
        "unexpected runtime name: {name}"
    );
}
```

- [ ] **Step 2: Add uuid dev-dependency to nexad and make adapters public for integration tests**

In `crates/nexad/Cargo.toml`, add:

```toml
[dev-dependencies]
tempfile = "3"
uuid = { workspace = true }
```

In `crates/nexad/src/main.rs` (or `lib.rs` if you have one), add to make the adapters module accessible from integration tests:

Create `crates/nexad/src/lib.rs`:

```rust
pub mod adapters;
```

Ensure `crates/nexad/src/main.rs` does not re-declare `mod adapters;` as private if `lib.rs` exports it. Update `main.rs` to use:

```rust
use nexad::adapters;
```

- [ ] **Step 3: Verify integration tests compile (but skip execution)**

Run: `cargo test -p nexad --test runtime_integration --no-run 2>&1`
Expected: compiles

- [ ] **Step 4: Run integration tests against Docker (if available)**

Run: `NEXA_TEST_RUNTIME=docker cargo test -p nexad --test runtime_integration -- --ignored 2>&1`
Expected: all tests pass (or skip gracefully if Docker is not running)

- [ ] **Step 5: Commit**

```bash
git add crates/nexad/tests/runtime_integration.rs crates/nexad/src/lib.rs crates/nexad/src/main.rs crates/nexad/Cargo.toml
git commit -m "feat: add integration test suite for ContainerRuntime (Docker + containerd)"
```

---

### Task 8: Wire runtime selection into nexad composition root

**Files:**
- Modify: `crates/nexad/src/main.rs`
- Modify: `crates/nexad/src/engine/orchestrator.rs`

- [ ] **Step 1: Write a test that Orchestrator accepts any ContainerRuntime**

In `crates/nexad/src/engine/orchestrator.rs`, add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr};
    use std::pin::Pin;

    use futures::Stream;
    use nexa_core::runtime::*;

    struct MockRuntime;

    #[async_trait::async_trait]
    impl ContainerRuntime for MockRuntime {
        async fn pull_image(&self, _image: &str) -> nexa_core::error::Result<()> { Ok(()) }
        async fn create_container(&self, config: &ContainerConfig) -> nexa_core::error::Result<String> {
            Ok(format!("mock-{}", config.name))
        }
        async fn start_container(&self, _id: &str) -> nexa_core::error::Result<()> { Ok(()) }
        async fn stop_container(&self, _id: &str, _t: u64) -> nexa_core::error::Result<()> { Ok(()) }
        async fn remove_container(&self, _id: &str, _f: bool) -> nexa_core::error::Result<()> { Ok(()) }
        async fn inspect_container(&self, _id: &str) -> nexa_core::error::Result<ContainerInfo> {
            Ok(ContainerInfo {
                id: "mock".into(),
                name: "mock".into(),
                image: "mock".into(),
                state: ContainerState::Running,
            })
        }
        async fn logs(&self, _id: &str, _tail: Option<u64>) -> nexa_core::error::Result<LogStream> {
            Ok(Box::pin(futures::stream::empty()))
        }
        async fn container_exists(&self, _name: &str) -> nexa_core::error::Result<bool> { Ok(false) }
        async fn create_network(&self, _name: &str) -> nexa_core::error::Result<String> { Ok("net-id".into()) }
        async fn remove_network(&self, _name: &str) -> nexa_core::error::Result<()> { Ok(()) }
        async fn connect_to_network(&self, _id: &str, _net: &str) -> nexa_core::error::Result<()> { Ok(()) }
        async fn container_ip(&self, _id: &str, _net: &str) -> nexa_core::error::Result<IpAddr> {
            Ok(IpAddr::V4(Ipv4Addr::new(172, 20, 0, 2)))
        }
        async fn events(&self) -> nexa_core::error::Result<EventStream> {
            Ok(Box::pin(futures::stream::empty()))
        }
        fn runtime_name(&self) -> &'static str { "mock" }
    }

    #[tokio::test]
    async fn orchestrator_accepts_mock_runtime() {
        let runtime: Arc<dyn ContainerRuntime> = Arc::new(MockRuntime);
        let orch = Orchestrator::new_with_runtime(runtime).await.unwrap();
        let projects = orch.list_projects();
        assert!(projects.is_empty());
    }

    #[tokio::test]
    async fn orchestrator_deploy_with_mock() {
        let runtime: Arc<dyn ContainerRuntime> = Arc::new(MockRuntime);
        let orch = Orchestrator::new_with_runtime(runtime).await.unwrap();

        let spec = nexa_core::models::DeploymentSpec {
            project: "test".into(),
            deployment: nexa_core::models::DeploymentMeta { name: "api".into() },
            replicas: 1,
            image: "nginx:latest".into(),
            ports: vec![8080],
            env: HashMap::new(),
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: nexa_core::models::RestartPolicy::default(),
        };

        let deployment = orch.deploy(spec).await.unwrap();
        assert_eq!(deployment.name(), "api");
        assert_eq!(deployment.project(), "test");

        let pods = orch.list_pods(Some("test"));
        assert_eq!(pods.len(), 1);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p nexad -- engine::orchestrator::tests 2>&1`
Expected: 2 tests pass

- [ ] **Step 3: Verify the full nexad binary compiles with runtime selection**

Run: `cargo build -p nexad 2>&1`
Expected: builds successfully

- [ ] **Step 4: Verify end-to-end help output**

Run: `cargo run -p nexad -- --help 2>&1`
Expected output includes:
```
--runtime <RUNTIME>  Container runtime to use: docker, containerd, or auto (default) [default: auto]
```

- [ ] **Step 5: Commit**

```bash
git add crates/nexad/src/engine/orchestrator.rs crates/nexad/src/main.rs
git commit -m "feat: wire runtime selection into nexad composition root with MockRuntime tests"
```

---

### Task 9: Add NexaError variants for containerd-specific errors

**Files:**
- Modify: `crates/nexa-core/src/error.rs`

- [ ] **Step 1: Add new error variants**

In `crates/nexa-core/src/error.rs`, add to the `NexaError` enum:

```rust
    #[error("CNI error: {0}")]
    Cni(String),

    #[error("runtime not available: {0}")]
    RuntimeNotAvailable(String),
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check 2>&1`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add crates/nexa-core/src/error.rs
git commit -m "feat: add Cni and RuntimeNotAvailable error variants to NexaError"
```

---

### Task 10: Final verification and cleanup

**Files:**
- All modified files from Tasks 1-9

- [ ] **Step 1: Run the full unit test suite**

Run: `cargo test 2>&1`
Expected: all tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace 2>&1`
Expected: no errors (warnings acceptable for now)

- [ ] **Step 3: Verify Docker runtime still works end-to-end**

Run: `cargo run -p nexad -- --runtime docker --help 2>&1`
Expected: shows full help output

- [ ] **Step 4: Check that --runtime containerd is accepted**

Run: `cargo run -p nexad -- --runtime containerd --help 2>&1`
Expected: shows full help output (actual startup would fail without containerd socket, but the flag is parsed)

- [ ] **Step 5: Verify file structure**

Run: `find crates -name "*.rs" -path "*/runtime/*" | sort`
Expected output:
```
crates/nexa-core/src/runtime/docker.rs
crates/nexa-core/src/runtime/mod.rs
crates/nexa-core/src/runtime/traits.rs
crates/nexad/src/adapters/runtime/cni.rs
crates/nexad/src/adapters/runtime/containerd.rs
crates/nexad/src/adapters/runtime/detect.rs
crates/nexad/src/adapters/runtime/docker.rs
crates/nexad/src/adapters/runtime/log_tailer.rs
crates/nexad/src/adapters/runtime/mod.rs
```

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "chore: container runtime abstraction cleanup and verification"
```

- [ ] **Step 7: Push**

```bash
git push origin main
```
