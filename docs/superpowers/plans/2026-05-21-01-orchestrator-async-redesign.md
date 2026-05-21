# Orchestrator Async Redesign — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the DashMap+RwLock orchestrator with an actor-model design using mpsc/oneshot channels, and restructure the codebase into hexagonal architecture (ports & adapters).

**Architecture:** The orchestrator becomes a single tokio task owning all state. External callers (API handlers) communicate via a `Command` enum sent over `mpsc::Sender`, receiving responses via `oneshot`. Domain logic moves to `nexa-core/src/domain/`, port traits to `nexa-core/src/ports/`, and infrastructure adapters to `nexad/src/adapters/`.

**Tech Stack:** tokio (mpsc, oneshot, spawn), async-trait, serde, uuid, chrono, bollard (Docker adapter only)

---

### Task 1: Restructure nexa-core into hexagonal layout

**Files:**
- Create: `crates/nexa-core/src/domain/mod.rs`
- Create: `crates/nexa-core/src/domain/models/mod.rs`
- Create: `crates/nexa-core/src/domain/models/deployment.rs`
- Create: `crates/nexa-core/src/domain/models/pod.rs`
- Create: `crates/nexa-core/src/domain/models/project.rs`
- Create: `crates/nexa-core/src/ports/mod.rs`
- Create: `crates/nexa-core/src/ports/runtime.rs`
- Modify: `crates/nexa-core/src/lib.rs`
- Delete: `crates/nexa-core/src/models/mod.rs`
- Delete: `crates/nexa-core/src/models/deployment.rs`
- Delete: `crates/nexa-core/src/models/pod.rs`
- Delete: `crates/nexa-core/src/models/project.rs`
- Delete: `crates/nexa-core/src/runtime/mod.rs`
- Delete: `crates/nexa-core/src/runtime/traits.rs`
- Delete: `crates/nexa-core/src/runtime/docker.rs`

- [ ] **Step 1: Create domain/models/ directory and move model files**

```bash
mkdir -p crates/nexa-core/src/domain/models
cp crates/nexa-core/src/models/deployment.rs crates/nexa-core/src/domain/models/deployment.rs
cp crates/nexa-core/src/models/pod.rs crates/nexa-core/src/domain/models/pod.rs
cp crates/nexa-core/src/models/project.rs crates/nexa-core/src/domain/models/project.rs
```

Create `crates/nexa-core/src/domain/models/mod.rs`:
```rust
mod deployment;
mod pod;
mod project;

pub use deployment::*;
pub use pod::*;
pub use project::*;
```

Create `crates/nexa-core/src/domain/mod.rs`:
```rust
pub mod models;
```

- [ ] **Step 2: Create ports/ directory and move runtime trait**

Create `crates/nexa-core/src/ports/runtime.rs` — copy the trait definitions from `crates/nexa-core/src/runtime/traits.rs` but update the import path for `Result`:

```rust
use std::collections::HashMap;
use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct ContainerConfig {
    pub name: String,
    pub image: String,
    pub env: HashMap<String, String>,
    pub ports: Vec<PortBinding>,
    pub volumes: Vec<VolumeBinding>,
    pub labels: HashMap<String, String>,
    pub network: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PortBinding {
    pub container_port: u16,
    pub host_port: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct VolumeBinding {
    pub source: String,
    pub target: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: ContainerState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ContainerState {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Exited,
    Dead,
    Unknown,
}

pub type LogStream = Pin<Box<dyn Stream<Item = Result<String>> + Send>>;

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
}
```

Create `crates/nexa-core/src/ports/mod.rs`:
```rust
pub mod runtime;
```

- [ ] **Step 3: Update lib.rs to new structure and delete old directories**

Replace `crates/nexa-core/src/lib.rs`:
```rust
pub mod config;
pub mod domain;
pub mod error;
pub mod ports;
```

Delete old files:
```bash
rm -rf crates/nexa-core/src/models
rm -rf crates/nexa-core/src/runtime
```

- [ ] **Step 4: Update config.rs imports**

In `crates/nexa-core/src/config.rs`, change:
```rust
use crate::models::DeploymentSpec;
```
to:
```rust
use crate::domain::models::DeploymentSpec;
```

- [ ] **Step 5: Verify nexa-core compiles**

Run: `cargo check -p nexa-core 2>&1`
Expected: compiles with no errors (warnings OK)

- [ ] **Step 6: Commit**

```bash
git add -A crates/nexa-core/
git commit -m "refactor: restructure nexa-core into hexagonal layout (domain + ports)"
```

---

### Task 2: Move Docker adapter to nexad

**Files:**
- Create: `crates/nexad/src/adapters/mod.rs`
- Create: `crates/nexad/src/adapters/runtime/mod.rs`
- Create: `crates/nexad/src/adapters/runtime/docker.rs`

- [ ] **Step 1: Create adapters directory structure**

```bash
mkdir -p crates/nexad/src/adapters/runtime
```

- [ ] **Step 2: Move docker.rs to nexad adapters**

Copy `crates/nexa-core/src/runtime/docker.rs` (which was deleted in Task 1 — use git to recover content) to `crates/nexad/src/adapters/runtime/docker.rs`. Update all imports to use the new paths:

```rust
use std::collections::HashMap;

use async_trait::async_trait;
use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, LogsOptions, RemoveContainerOptions,
    StopContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::network::{ConnectNetworkOptions, CreateNetworkOptions};
use bollard::Docker;
use bollard::models::{EndpointSettings, HostConfig, PortBinding as BollardPortBinding};
use futures::StreamExt;
use tracing::{debug, info};

use nexa_core::ports::runtime::*;
use nexa_core::error::{NexaError, Result};

pub struct DockerRuntime {
    client: Docker,
}

impl DockerRuntime {
    pub fn new() -> Result<Self> {
        let client =
            Docker::connect_with_local_defaults().map_err(|e| NexaError::Runtime(e.to_string()))?;
        Ok(Self { client })
    }

    pub async fn ping(&self) -> Result<()> {
        self.client
            .ping()
            .await
            .map_err(|e| NexaError::Runtime(format!("Docker daemon unreachable: {e}")))?;
        Ok(())
    }
}

// ... rest of the impl ContainerRuntime for DockerRuntime unchanged,
// but every `use super::traits::*` becomes `use nexa_core::ports::runtime::*`
```

Create `crates/nexad/src/adapters/runtime/mod.rs`:
```rust
mod docker;

pub use docker::DockerRuntime;
```

Create `crates/nexad/src/adapters/mod.rs`:
```rust
pub mod runtime;
```

- [ ] **Step 3: Update nexad main.rs to reference adapters**

Add `mod adapters;` to `crates/nexad/src/main.rs` (after existing mods).

- [ ] **Step 4: Verify nexad compiles**

Run: `cargo check -p nexad 2>&1`
Expected: compiles (warnings OK). The engine/orchestrator.rs will still use old imports — we'll replace that entirely in Task 3.

- [ ] **Step 5: Commit**

```bash
git add crates/nexad/src/adapters/
git commit -m "refactor: move Docker runtime adapter to nexad/adapters/"
```

---

### Task 3: Build the Command enum and OrchestratorHandle

**Files:**
- Create: `crates/nexa-core/src/domain/orchestrator.rs`
- Test: `crates/nexa-core/src/domain/orchestrator.rs` (inline tests)

- [ ] **Step 1: Write the failing test**

Add to `crates/nexa-core/src/domain/orchestrator.rs`:

```rust
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::domain::models::*;
use crate::error::{NexaError, Result};
use crate::ports::runtime::{ContainerRuntime, LogStream};

pub enum Command {
    Deploy {
        spec: DeploymentSpec,
        reply: oneshot::Sender<Result<Deployment>>,
    },
    ListDeployments {
        project: Option<String>,
        reply: oneshot::Sender<Vec<Deployment>>,
    },
    ListPods {
        project: Option<String>,
        reply: oneshot::Sender<Vec<Pod>>,
    },
    CreateProject {
        name: String,
        reply: oneshot::Sender<Result<Project>>,
    },
    ListProjects {
        reply: oneshot::Sender<Vec<Project>>,
    },
    Stop {
        project: String,
        name: String,
        reply: oneshot::Sender<Result<()>>,
    },
    RemoveDeployment {
        project: String,
        name: String,
        reply: oneshot::Sender<Result<()>>,
    },
    Scale {
        project: String,
        name: String,
        replicas: u32,
        reply: oneshot::Sender<Result<Deployment>>,
    },
    PodLogs {
        project: String,
        name: String,
        tail: Option<u64>,
        reply: oneshot::Sender<Result<LogStream>>,
    },
}

#[derive(Clone)]
pub struct OrchestratorHandle {
    tx: mpsc::Sender<Command>,
}

impl OrchestratorHandle {
    pub async fn deploy(&self, spec: DeploymentSpec) -> Result<Deployment> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::Deploy { spec, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }

    pub async fn list_deployments(&self, project: Option<String>) -> Vec<Deployment> {
        let (reply, rx) = oneshot::channel();
        let _ = self.tx.send(Command::ListDeployments { project, reply }).await;
        rx.await.unwrap_or_default()
    }

    pub async fn list_pods(&self, project: Option<String>) -> Vec<Pod> {
        let (reply, rx) = oneshot::channel();
        let _ = self.tx.send(Command::ListPods { project, reply }).await;
        rx.await.unwrap_or_default()
    }

    pub async fn create_project(&self, name: String) -> Result<Project> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::CreateProject { name, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }

    pub async fn list_projects(&self) -> Vec<Project> {
        let (reply, rx) = oneshot::channel();
        let _ = self.tx.send(Command::ListProjects { reply }).await;
        rx.await.unwrap_or_default()
    }

    pub async fn stop(&self, project: String, name: String) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::Stop { project, name, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }

    pub async fn remove_deployment(&self, project: String, name: String) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::RemoveDeployment { project, name, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }

    pub async fn scale(&self, project: String, name: String, replicas: u32) -> Result<Deployment> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::Scale { project, name, replicas, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }

    pub async fn pod_logs(&self, project: String, name: String, tail: Option<u64>) -> Result<LogStream> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::PodLogs { project, name, tail, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn handle_create_project_sends_command() {
        let (tx, mut rx) = mpsc::channel(16);
        let handle = OrchestratorHandle { tx };

        tokio::spawn(async move {
            if let Some(Command::CreateProject { name, reply }) = rx.recv().await {
                assert_eq!(name, "test-project");
                let _ = reply.send(Ok(Project::new("test-project")));
            }
        });

        let result = handle.create_project("test-project".into()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "test-project");
    }

    #[tokio::test]
    async fn handle_returns_error_when_orchestrator_stopped() {
        let (tx, rx) = mpsc::channel(1);
        let handle = OrchestratorHandle { tx };
        drop(rx);

        let result = handle.create_project("test".into()).await;
        assert!(result.is_err());
    }
}
```

Update `crates/nexa-core/src/domain/mod.rs`:
```rust
pub mod models;
pub mod orchestrator;
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test -p nexa-core -- domain::orchestrator 2>&1`
Expected: 2 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/nexa-core/src/domain/
git commit -m "feat: add Command enum and OrchestratorHandle with channel-based communication"
```

---

### Task 4: Build the Orchestrator actor loop

**Files:**
- Modify: `crates/nexa-core/src/domain/orchestrator.rs`

- [ ] **Step 1: Write tests for the actor loop**

Add to the `tests` module in `orchestrator.rs`:

```rust
use std::collections::HashMap;
use std::pin::Pin;
use futures::Stream;
use crate::ports::runtime::*;

struct MockRuntime;

#[async_trait::async_trait]
impl ContainerRuntime for MockRuntime {
    async fn pull_image(&self, _image: &str) -> Result<()> { Ok(()) }
    async fn create_container(&self, config: &ContainerConfig) -> Result<String> {
        Ok(format!("mock-{}", config.name))
    }
    async fn start_container(&self, _id: &str) -> Result<()> { Ok(()) }
    async fn stop_container(&self, _id: &str, _timeout: u64) -> Result<()> { Ok(()) }
    async fn remove_container(&self, _id: &str, _force: bool) -> Result<()> { Ok(()) }
    async fn inspect_container(&self, _id: &str) -> Result<ContainerInfo> {
        Ok(ContainerInfo {
            id: "mock".into(),
            name: "mock".into(),
            image: "mock".into(),
            state: ContainerState::Running,
        })
    }
    async fn logs(&self, _id: &str, _tail: Option<u64>) -> Result<LogStream> {
        Ok(Box::pin(futures::stream::empty()))
    }
    async fn container_exists(&self, _name: &str) -> Result<bool> { Ok(false) }
    async fn create_network(&self, _name: &str) -> Result<String> { Ok("net-id".into()) }
    async fn remove_network(&self, _name: &str) -> Result<()> { Ok(()) }
    async fn connect_to_network(&self, _id: &str, _net: &str) -> Result<()> { Ok(()) }
}

fn spawn_test_orchestrator() -> OrchestratorHandle {
    Orchestrator::spawn(Arc::new(MockRuntime))
}

#[tokio::test]
async fn deploy_creates_pods() {
    let handle = spawn_test_orchestrator();

    let spec = DeploymentSpec {
        project: "test".into(),
        deployment: DeploymentMeta { name: "api".into() },
        replicas: 2,
        image: "nginx:latest".into(),
        ports: vec![8080],
        env: HashMap::new(),
        volumes: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::default(),
    };

    let deployment = handle.deploy(spec).await.unwrap();
    assert_eq!(deployment.name(), "api");
    assert_eq!(deployment.project(), "test");

    let pods = handle.list_pods(Some("test".into())).await;
    assert_eq!(pods.len(), 2);
    assert!(pods.iter().all(|p| p.status == PodStatus::Running));
}

#[tokio::test]
async fn list_projects_returns_auto_created() {
    let handle = spawn_test_orchestrator();

    let spec = DeploymentSpec {
        project: "myapp".into(),
        deployment: DeploymentMeta { name: "web".into() },
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
    let projects = handle.list_projects().await;
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "myapp");
}

#[tokio::test]
async fn scale_changes_pod_count() {
    let handle = spawn_test_orchestrator();

    let spec = DeploymentSpec {
        project: "test".into(),
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
    assert_eq!(handle.list_pods(None).await.len(), 1);

    handle.scale("test".into(), "api".into(), 3).await.unwrap();
    assert_eq!(handle.list_pods(None).await.len(), 3);

    handle.scale("test".into(), "api".into(), 1).await.unwrap();
    assert_eq!(handle.list_pods(None).await.len(), 1);
}

#[tokio::test]
async fn stop_removes_pods() {
    let handle = spawn_test_orchestrator();

    let spec = DeploymentSpec {
        project: "test".into(),
        deployment: DeploymentMeta { name: "api".into() },
        replicas: 2,
        image: "nginx".into(),
        ports: vec![],
        env: HashMap::new(),
        volumes: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::default(),
    };

    handle.deploy(spec).await.unwrap();
    handle.stop("test".into(), "api".into()).await.unwrap();

    let pods = handle.list_pods(None).await;
    assert_eq!(pods.len(), 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p nexa-core -- domain::orchestrator 2>&1`
Expected: FAIL — `Orchestrator::spawn` does not exist yet

- [ ] **Step 3: Implement the Orchestrator actor loop**

Add to `crates/nexa-core/src/domain/orchestrator.rs` (above the `tests` module):

```rust
use std::collections::HashMap as StdHashMap;

pub struct Orchestrator {
    runtime: Arc<dyn ContainerRuntime>,
    projects: StdHashMap<String, Project>,
    deployments: StdHashMap<Uuid, Deployment>,
    pods: StdHashMap<Uuid, Pod>,
}

impl Orchestrator {
    pub fn spawn(runtime: Arc<dyn ContainerRuntime>) -> OrchestratorHandle {
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(async move {
            let mut orch = Self {
                runtime,
                projects: StdHashMap::new(),
                deployments: StdHashMap::new(),
                pods: StdHashMap::new(),
            };
            orch.run(rx).await;
        });
        OrchestratorHandle { tx }
    }

    async fn run(&mut self, mut rx: mpsc::Receiver<Command>) {
        while let Some(cmd) = rx.recv().await {
            match cmd {
                Command::Deploy { spec, reply } => {
                    let result = self.handle_deploy(spec).await;
                    let _ = reply.send(result);
                }
                Command::ListDeployments { project, reply } => {
                    let _ = reply.send(self.handle_list_deployments(project.as_deref()));
                }
                Command::ListPods { project, reply } => {
                    let _ = reply.send(self.handle_list_pods(project.as_deref()));
                }
                Command::CreateProject { name, reply } => {
                    let _ = reply.send(self.handle_create_project(&name));
                }
                Command::ListProjects { reply } => {
                    let _ = reply.send(self.handle_list_projects());
                }
                Command::Stop { project, name, reply } => {
                    let result = self.handle_stop(&project, &name).await;
                    let _ = reply.send(result);
                }
                Command::RemoveDeployment { project, name, reply } => {
                    let result = self.handle_remove_deployment(&project, &name).await;
                    let _ = reply.send(result);
                }
                Command::Scale { project, name, replicas, reply } => {
                    let result = self.handle_scale(&project, &name, replicas).await;
                    let _ = reply.send(result);
                }
                Command::PodLogs { project, name, tail, reply } => {
                    let result = self.handle_pod_logs(&project, &name, tail).await;
                    let _ = reply.send(result);
                }
            }
        }
    }

    fn ensure_project(&mut self, name: &str) {
        if !self.projects.contains_key(name) {
            self.projects.insert(name.to_string(), Project::new(name));
        }
    }

    fn handle_create_project(&mut self, name: &str) -> Result<Project> {
        if self.projects.contains_key(name) {
            return Err(NexaError::InvalidSpec(format!("project '{name}' already exists")));
        }
        let project = Project::new(name);
        self.projects.insert(name.to_string(), project.clone());
        Ok(project)
    }

    fn handle_list_projects(&self) -> Vec<Project> {
        self.projects.values().cloned().collect()
    }

    async fn handle_deploy(&mut self, spec: DeploymentSpec) -> Result<Deployment> {
        self.ensure_project(&spec.project);

        let existing_id = self.find_deployment_id(&spec.project, &spec.deployment.name);

        if let Some(id) = existing_id {
            let deployment = self.deployments.get_mut(&id).unwrap();
            deployment.spec = spec.clone();
            deployment.updated_at = chrono::Utc::now();
            let id = deployment.id;
            self.reconcile_deployment(id).await?;
            return Ok(self.deployments[&id].clone());
        }

        let deployment = Deployment::from_spec(spec);
        let id = deployment.id;
        self.deployments.insert(id, deployment);
        self.reconcile_deployment(id).await?;
        Ok(self.deployments[&id].clone())
    }

    fn handle_list_deployments(&self, project: Option<&str>) -> Vec<Deployment> {
        self.deployments
            .values()
            .filter(|d| match project {
                Some(p) => d.project() == p,
                None => true,
            })
            .cloned()
            .collect()
    }

    fn handle_list_pods(&self, project: Option<&str>) -> Vec<Pod> {
        self.pods
            .values()
            .filter(|p| match project {
                Some(proj) => p.project == proj,
                None => true,
            })
            .cloned()
            .collect()
    }

    async fn handle_stop(&mut self, project: &str, name: &str) -> Result<()> {
        let deployment_id = self
            .find_deployment_id(project, name)
            .ok_or_else(|| NexaError::DeploymentNotFound(format!("{project}/{name}")))?;

        let pod_ids: Vec<Uuid> = self
            .pods
            .values()
            .filter(|p| p.deployment_id == deployment_id)
            .map(|p| p.id)
            .collect();

        for pod_id in &pod_ids {
            if let Some(pod) = self.pods.get(pod_id) {
                if let Some(cid) = &pod.container_id {
                    let _ = self.runtime.stop_container(cid, 10).await;
                    let _ = self.runtime.remove_container(cid, true).await;
                }
            }
            self.pods.remove(pod_id);
        }

        if let Some(d) = self.deployments.get_mut(&deployment_id) {
            d.status = DeploymentStatus::Stopped;
        }

        Ok(())
    }

    async fn handle_remove_deployment(&mut self, project: &str, name: &str) -> Result<()> {
        self.handle_stop(project, name).await?;
        let id = self
            .find_deployment_id(project, name)
            .ok_or_else(|| NexaError::DeploymentNotFound(format!("{project}/{name}")))?;
        self.deployments.remove(&id);
        Ok(())
    }

    async fn handle_scale(&mut self, project: &str, name: &str, replicas: u32) -> Result<Deployment> {
        let deployment_id = self
            .find_deployment_id(project, name)
            .ok_or_else(|| NexaError::DeploymentNotFound(format!("{project}/{name}")))?;

        if let Some(d) = self.deployments.get_mut(&deployment_id) {
            d.spec.replicas = replicas;
            d.updated_at = chrono::Utc::now();
        }

        self.reconcile_deployment(deployment_id).await?;
        Ok(self.deployments[&deployment_id].clone())
    }

    async fn handle_pod_logs(&self, project: &str, name: &str, tail: Option<u64>) -> Result<LogStream> {
        let pod = self
            .pods
            .values()
            .find(|p| p.project == project && p.deployment_name == name)
            .ok_or_else(|| NexaError::PodNotFound(format!("{project}/{name}")))?;

        let container_id = pod
            .container_id
            .as_ref()
            .ok_or_else(|| NexaError::Runtime("pod has no container".into()))?;

        self.runtime.logs(container_id, tail).await
    }

    async fn reconcile_deployment(&mut self, deployment_id: Uuid) -> Result<()> {
        let spec = self.deployments[&deployment_id].spec.clone();
        let desired = spec.replicas;

        let network_name = format!("nexa-{}", spec.project);
        let _ = self.runtime.create_network(&network_name).await;

        let mut current_pods: Vec<Uuid> = self
            .pods
            .values()
            .filter(|p| p.deployment_id == deployment_id)
            .map(|p| p.id)
            .collect();

        let current_count = current_pods.len() as u32;

        if current_count < desired {
            for i in current_count..desired {
                self.create_pod(deployment_id, &spec, i).await?;
            }
        } else if current_count > desired {
            current_pods.sort();
            for &pod_id in &current_pods[(desired as usize)..] {
                if let Some(pod) = self.pods.get(&pod_id) {
                    if let Some(cid) = &pod.container_id {
                        let _ = self.runtime.stop_container(cid, 10).await;
                        let _ = self.runtime.remove_container(cid, true).await;
                    }
                }
                self.pods.remove(&pod_id);
            }
        }

        let (all_running, any_failed) = self
            .pods
            .values()
            .filter(|p| p.deployment_id == deployment_id)
            .fold((true, false), |(all_r, any_f), p| {
                match p.status {
                    PodStatus::Running => (all_r, any_f),
                    PodStatus::Failed => (false, true),
                    _ => (false, any_f),
                }
            });

        if let Some(d) = self.deployments.get_mut(&deployment_id) {
            d.status = if all_running && desired > 0 {
                DeploymentStatus::Running
            } else if any_failed {
                DeploymentStatus::Degraded
            } else {
                DeploymentStatus::Pending
            };
        }

        Ok(())
    }

    async fn create_pod(&mut self, deployment_id: Uuid, spec: &DeploymentSpec, index: u32) -> Result<()> {
        let mut pod = Pod::new(
            deployment_id,
            &spec.project,
            &spec.deployment.name,
            index,
            &spec.image,
        );

        let container_name = pod.container_name();
        let network_name = format!("nexa-{}", spec.project);

        pod.status = PodStatus::Creating;

        let _ = self.runtime.pull_image(&spec.image).await;

        if self.runtime.container_exists(&container_name).await? {
            let _ = self.runtime.stop_container(&container_name, 5).await;
            let _ = self.runtime.remove_container(&container_name, true).await;
        }

        let ports: Vec<crate::ports::runtime::PortBinding> = spec
            .ports
            .iter()
            .map(|&p| crate::ports::runtime::PortBinding {
                container_port: p,
                host_port: if spec.replicas == 1 { Some(p) } else { None },
            })
            .collect();

        let mut labels = StdHashMap::new();
        labels.insert("managed-by".to_string(), "nexanet".to_string());
        labels.insert("nexa.project".to_string(), spec.project.clone());
        labels.insert("nexa.deployment".to_string(), spec.deployment.name.clone());
        labels.insert("nexa.pod-id".to_string(), pod.id.to_string());

        let config = ContainerConfig {
            name: container_name,
            image: spec.image.clone(),
            env: spec.env.clone(),
            ports,
            volumes: spec
                .volumes
                .iter()
                .map(|v| crate::ports::runtime::VolumeBinding {
                    source: v.name.clone(),
                    target: v.mount_path.clone(),
                    read_only: false,
                })
                .collect(),
            labels,
            network: Some(network_name),
        };

        match self.runtime.create_container(&config).await {
            Ok(container_id) => {
                self.runtime.start_container(&container_id).await?;
                pod.container_id = Some(container_id);
                pod.status = PodStatus::Running;
            }
            Err(_) => {
                pod.status = PodStatus::Failed;
            }
        }

        self.pods.insert(pod.id, pod);
        Ok(())
    }

    fn find_deployment_id(&self, project: &str, name: &str) -> Option<Uuid> {
        self.deployments
            .values()
            .find(|d| d.project() == project && d.name() == name)
            .map(|d| d.id)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p nexa-core -- domain::orchestrator 2>&1`
Expected: all 6 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-core/src/domain/orchestrator.rs
git commit -m "feat: implement Orchestrator actor loop with channel-based state management"
```

---

### Task 5: Update nexad API to use OrchestratorHandle

**Files:**
- Modify: `crates/nexad/src/api/mod.rs`
- Modify: `crates/nexad/src/api/routes.rs`
- Modify: `crates/nexad/src/api/handlers.rs`
- Modify: `crates/nexad/src/main.rs`
- Delete: `crates/nexad/src/engine/mod.rs`
- Delete: `crates/nexad/src/engine/orchestrator.rs`

- [ ] **Step 1: Update api/mod.rs**

Replace `crates/nexad/src/api/mod.rs`:
```rust
mod handlers;
mod routes;

use nexa_core::domain::orchestrator::OrchestratorHandle;

pub async fn serve(handle: OrchestratorHandle, addr: &str) -> anyhow::Result<()> {
    let app = routes::build(handle);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("nexad API listening on {addr}");

    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 2: Update api/routes.rs**

Replace `crates/nexad/src/api/routes.rs`:
```rust
use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

use super::handlers;
use nexa_core::domain::orchestrator::OrchestratorHandle;

pub fn build(handle: OrchestratorHandle) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/api/v1/projects", get(handlers::list_projects))
        .route("/api/v1/projects", post(handlers::create_project))
        .route("/api/v1/deployments", get(handlers::list_deployments))
        .route("/api/v1/deploy", post(handlers::deploy))
        .route(
            "/api/v1/projects/{project}/deployments/{name}",
            delete(handlers::remove_deployment),
        )
        .route(
            "/api/v1/projects/{project}/deployments/{name}/stop",
            post(handlers::stop_deployment),
        )
        .route(
            "/api/v1/projects/{project}/deployments/{name}/scale",
            post(handlers::scale_deployment),
        )
        .route("/api/v1/pods", get(handlers::list_pods))
        .route(
            "/api/v1/projects/{project}/deployments/{name}/logs",
            get(handlers::logs),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(handle)
}
```

- [ ] **Step 3: Update api/handlers.rs**

Replace `crates/nexad/src/api/handlers.rs`:
```rust
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::Json;
use futures::StreamExt;
use serde::Deserialize;

use nexa_core::config::parse_deployment;
use nexa_core::domain::orchestrator::OrchestratorHandle;

type AppState = State<OrchestratorHandle>;

pub async fn health() -> &'static str {
    "ok"
}

pub async fn list_projects(State(handle): AppState) -> impl IntoResponse {
    Json(handle.list_projects().await)
}

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    name: String,
}

pub async fn create_project(
    State(handle): AppState,
    Json(req): Json<CreateProjectRequest>,
) -> impl IntoResponse {
    match handle.create_project(req.name).await {
        Ok(project) => (StatusCode::CREATED, Json(serde_json::json!(project))).into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct DeploymentFilter {
    project: Option<String>,
}

pub async fn list_deployments(
    State(handle): AppState,
    Query(filter): Query<DeploymentFilter>,
) -> impl IntoResponse {
    Json(handle.list_deployments(filter.project).await)
}

pub async fn deploy(State(handle): AppState, body: String) -> impl IntoResponse {
    let spec = match serde_json::from_str(&body) {
        Ok(spec) => spec,
        Err(_) => match parse_deployment(&body) {
            Ok(spec) => spec,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
        },
    };

    match handle.deploy(spec).await {
        Ok(deployment) => (StatusCode::CREATED, Json(serde_json::json!(deployment))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn stop_deployment(
    State(handle): AppState,
    Path((project, name)): Path<(String, String)>,
) -> impl IntoResponse {
    match handle.stop(project, name).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn remove_deployment(
    State(handle): AppState,
    Path((project, name)): Path<(String, String)>,
) -> impl IntoResponse {
    match handle.remove_deployment(project, name).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct ScaleRequest {
    replicas: u32,
}

pub async fn scale_deployment(
    State(handle): AppState,
    Path((project, name)): Path<(String, String)>,
    Json(req): Json<ScaleRequest>,
) -> impl IntoResponse {
    match handle.scale(project, name, req.replicas).await {
        Ok(deployment) => Json(serde_json::json!(deployment)).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn list_pods(
    State(handle): AppState,
    Query(filter): Query<DeploymentFilter>,
) -> impl IntoResponse {
    Json(handle.list_pods(filter.project).await)
}

#[derive(Deserialize)]
pub struct LogsQuery {
    tail: Option<u64>,
}

pub async fn logs(
    State(handle): AppState,
    Path((project, name)): Path<(String, String)>,
    Query(query): Query<LogsQuery>,
) -> impl IntoResponse {
    match handle.pod_logs(project, name, query.tail).await {
        Ok(stream) => {
            let event_stream =
                stream.map(|result| -> std::result::Result<Event, std::convert::Infallible> {
                    match result {
                        Ok(line) => Ok(Event::default().data(line)),
                        Err(e) => Ok(Event::default().data(format!("error: {e}"))),
                    }
                });
            Sse::new(event_stream).into_response()
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
```

- [ ] **Step 4: Update main.rs — composition root**

Replace `crates/nexad/src/main.rs`:
```rust
mod adapters;
mod api;

use std::sync::Arc;

use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

use nexa_core::domain::orchestrator::Orchestrator;

#[derive(Parser)]
#[command(name = "nexad", about = "NexaNet daemon", version)]
struct Cli {
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    #[arg(long, default_value = "6443")]
    port: u16,

    #[arg(long, default_value = "/var/lib/nexa")]
    data_dir: String,
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

    let runtime = adapters::runtime::DockerRuntime::new()?;
    runtime.ping().await?;
    info!("connected to Docker runtime");

    let handle = Orchestrator::spawn(Arc::new(runtime));
    let addr = format!("{}:{}", cli.host, cli.port);

    api::serve(handle, &addr).await
}
```

- [ ] **Step 5: Delete old engine module**

```bash
rm -rf crates/nexad/src/engine
```

- [ ] **Step 6: Remove dashmap dependency from nexad Cargo.toml**

In `crates/nexad/Cargo.toml`, remove the line:
```
dashmap = { workspace = true }
```

- [ ] **Step 7: Verify full workspace compiles**

Run: `cargo check 2>&1`
Expected: compiles (warnings OK)

- [ ] **Step 8: Run all tests**

Run: `cargo test 2>&1`
Expected: all tests pass (config tests + orchestrator tests)

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "refactor: wire nexad to actor-based orchestrator, remove old engine module"
```

---

### Task 6: Update nexa-cli imports

**Files:**
- Modify: `crates/nexa-cli/src/commands.rs`

- [ ] **Step 1: Update import paths**

In `crates/nexa-cli/src/commands.rs`, change:
```rust
use nexa_core::models::{Deployment, Pod, Project};
```
to:
```rust
use nexa_core::domain::models::{Deployment, Pod, Project};
```

- [ ] **Step 2: Verify CLI compiles**

Run: `cargo check -p nexa-cli 2>&1`
Expected: compiles

- [ ] **Step 3: Run full test suite**

Run: `cargo test 2>&1`
Expected: all tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/nexa-cli/src/commands.rs
git commit -m "fix: update nexa-cli imports for hexagonal layout"
```

---

### Task 7: Final cleanup and push

**Files:**
- Modify: `Cargo.toml` (remove unused workspace deps if any)

- [ ] **Step 1: Check for unused dependencies**

Run: `cargo check 2>&1 | grep "unused"`
Remove any unused workspace dependencies.

- [ ] **Step 2: Run final test suite**

Run: `cargo test 2>&1`
Expected: all tests pass

- [ ] **Step 3: Push to remote**

```bash
git push origin main
```
