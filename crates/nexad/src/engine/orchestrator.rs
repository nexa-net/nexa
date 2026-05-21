use std::sync::Arc;

use dashmap::DashMap;
use nexa_core::error::{NexaError, Result};
use nexa_core::models::*;
use nexa_core::runtime::*;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use uuid::Uuid;

pub struct Orchestrator {
    runtime: Arc<dyn ContainerRuntime>,
    projects: DashMap<String, Project>,
    deployments: DashMap<Uuid, Arc<RwLock<Deployment>>>,
    pods: DashMap<Uuid, Arc<RwLock<Pod>>>,
}

impl Orchestrator {
    pub async fn new() -> anyhow::Result<Arc<Self>> {
        let runtime = DockerRuntime::new()?;
        runtime.ping().await?;
        info!("connected to Docker runtime");

        Ok(Arc::new(Self {
            runtime: Arc::new(runtime),
            projects: DashMap::new(),
            deployments: DashMap::new(),
            pods: DashMap::new(),
        }))
    }

    pub fn create_project(&self, name: &str) -> Result<Project> {
        if self.projects.contains_key(name) {
            return Err(NexaError::InvalidSpec(format!(
                "project '{name}' already exists"
            )));
        }
        let project = Project::new(name);
        self.projects.insert(name.to_string(), project.clone());
        info!(name, "project created");
        Ok(project)
    }

    pub fn list_projects(&self) -> Vec<Project> {
        self.projects.iter().map(|r| r.value().clone()).collect()
    }

    pub fn ensure_project(&self, name: &str) {
        if !self.projects.contains_key(name) {
            self.projects
                .insert(name.to_string(), Project::new(name));
        }
    }

    pub async fn deploy(&self, spec: DeploymentSpec) -> Result<Deployment> {
        self.ensure_project(&spec.project);

        let existing = self.find_deployment(&spec.project, &spec.deployment.name);
        if let Some(existing) = existing {
            return self.update_deployment(existing, spec).await;
        }

        let deployment = Deployment::from_spec(spec);
        let deployment_id = deployment.id;
        let deployment = Arc::new(RwLock::new(deployment));
        self.deployments.insert(deployment_id, deployment.clone());

        self.reconcile_deployment(deployment_id).await?;

        let d = deployment.read().await;
        Ok(d.clone())
    }

    async fn update_deployment(
        &self,
        deployment: Arc<RwLock<Deployment>>,
        new_spec: DeploymentSpec,
    ) -> Result<Deployment> {
        let deployment_id = {
            let mut d = deployment.write().await;
            d.spec = new_spec;
            d.updated_at = chrono::Utc::now();
            d.id
        };

        self.reconcile_deployment(deployment_id).await?;

        let d = deployment.read().await;
        Ok(d.clone())
    }

    async fn reconcile_deployment(&self, deployment_id: Uuid) -> Result<()> {
        let deployment = self
            .deployments
            .get(&deployment_id)
            .ok_or(NexaError::DeploymentNotFound(deployment_id.to_string()))?
            .clone();

        let (spec, desired_replicas) = {
            let d = deployment.read().await;
            (d.spec.clone(), d.spec.replicas)
        };

        let network_name = format!("nexa-{}", spec.project);
        if !self
            .runtime
            .container_exists(&network_name)
            .await
            .unwrap_or(false)
        {
            let _ = self.runtime.create_network(&network_name).await;
        }

        let current_pods: Vec<(Uuid, Arc<RwLock<Pod>>)> = self
            .pods
            .iter()
            .filter(|entry| {
                let pod = entry.value().clone();
                let rt = tokio::runtime::Handle::current();
                let p = rt.block_on(pod.read());
                p.deployment_id == deployment_id
            })
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();

        let current_count = current_pods.len() as u32;

        if current_count < desired_replicas {
            for i in current_count..desired_replicas {
                self.create_pod(deployment_id, &spec, i).await?;
            }
        } else if current_count > desired_replicas {
            let to_remove = &current_pods[(desired_replicas as usize)..];
            for (pod_id, pod) in to_remove {
                let p = pod.read().await;
                if let Some(cid) = &p.container_id {
                    let _ = self.runtime.stop_container(cid, 10).await;
                    let _ = self.runtime.remove_container(cid, true).await;
                }
                self.pods.remove(pod_id);
            }
        }

        let mut all_running = true;
        let mut any_failed = false;

        for entry in self.pods.iter() {
            let pod = entry.value().read().await;
            if pod.deployment_id == deployment_id {
                match pod.status {
                    PodStatus::Running => {}
                    PodStatus::Failed => {
                        any_failed = true;
                        all_running = false;
                    }
                    _ => all_running = false,
                }
            }
        }

        let new_status = if all_running && desired_replicas > 0 {
            DeploymentStatus::Running
        } else if any_failed {
            DeploymentStatus::Degraded
        } else {
            DeploymentStatus::Pending
        };

        {
            let mut d = deployment.write().await;
            d.status = new_status;
        }

        Ok(())
    }

    async fn create_pod(
        &self,
        deployment_id: Uuid,
        spec: &DeploymentSpec,
        replica_index: u32,
    ) -> Result<()> {
        let mut pod = Pod::new(
            deployment_id,
            &spec.project,
            &spec.deployment.name,
            replica_index,
            &spec.image,
        );

        let container_name = pod.container_name();
        let network_name = format!("nexa-{}", spec.project);

        info!(
            name = container_name,
            image = spec.image,
            "creating pod"
        );

        pod.status = PodStatus::Creating;

        if let Err(e) = self.runtime.pull_image(&spec.image).await {
            warn!(image = spec.image, error = %e, "image pull failed, trying with local image");
        }

        if self.runtime.container_exists(&container_name).await? {
            let _ = self.runtime.stop_container(&container_name, 5).await;
            let _ = self.runtime.remove_container(&container_name, true).await;
        }

        let ports: Vec<PortBinding> = spec
            .ports
            .iter()
            .map(|&p| PortBinding {
                container_port: p,
                host_port: if spec.replicas == 1 { Some(p) } else { None },
            })
            .collect();

        let mut labels = std::collections::HashMap::new();
        labels.insert("managed-by".to_string(), "nexanet".to_string());
        labels.insert("nexa.project".to_string(), spec.project.clone());
        labels.insert(
            "nexa.deployment".to_string(),
            spec.deployment.name.clone(),
        );
        labels.insert("nexa.pod-id".to_string(), pod.id.to_string());

        let config = ContainerConfig {
            name: container_name.clone(),
            image: spec.image.clone(),
            env: spec.env.clone(),
            ports,
            volumes: spec
                .volumes
                .iter()
                .map(|v| VolumeBinding {
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
                info!(name = container_name, "pod running");
            }
            Err(e) => {
                error!(name = container_name, error = %e, "failed to create pod");
                pod.status = PodStatus::Failed;
            }
        }

        self.pods.insert(pod.id, Arc::new(RwLock::new(pod)));
        Ok(())
    }

    pub fn list_deployments(&self, project: Option<&str>) -> Vec<Deployment> {
        let rt = tokio::runtime::Handle::current();
        self.deployments
            .iter()
            .filter_map(|entry| {
                let d = rt.block_on(entry.value().read());
                match project {
                    Some(p) if d.project() != p => None,
                    _ => Some(d.clone()),
                }
            })
            .collect()
    }

    pub fn list_pods(&self, project: Option<&str>) -> Vec<Pod> {
        let rt = tokio::runtime::Handle::current();
        self.pods
            .iter()
            .filter_map(|entry| {
                let p = rt.block_on(entry.value().read());
                match project {
                    Some(proj) if p.project != proj => None,
                    _ => Some(p.clone()),
                }
            })
            .collect()
    }

    pub async fn stop_deployment(&self, project: &str, name: &str) -> Result<()> {
        let deployment = self
            .find_deployment(project, name)
            .ok_or_else(|| NexaError::DeploymentNotFound(format!("{project}/{name}")))?;

        let deployment_id = deployment.read().await.id;

        let pod_ids: Vec<Uuid> = self
            .pods
            .iter()
            .filter_map(|entry| {
                let rt = tokio::runtime::Handle::current();
                let p = rt.block_on(entry.value().read());
                if p.deployment_id == deployment_id {
                    Some(*entry.key())
                } else {
                    None
                }
            })
            .collect();

        for pod_id in &pod_ids {
            if let Some(entry) = self.pods.get(pod_id) {
                let pod = entry.value().read().await;
                if let Some(cid) = &pod.container_id {
                    let _ = self.runtime.stop_container(cid, 10).await;
                    let _ = self.runtime.remove_container(cid, true).await;
                }
            }
            self.pods.remove(pod_id);
        }

        {
            let mut d = deployment.write().await;
            d.status = DeploymentStatus::Stopped;
        }

        info!(project, name, "deployment stopped");
        Ok(())
    }

    pub async fn remove_deployment(&self, project: &str, name: &str) -> Result<()> {
        self.stop_deployment(project, name).await?;

        let deployment_id = {
            let d = self
                .find_deployment(project, name)
                .ok_or_else(|| NexaError::DeploymentNotFound(format!("{project}/{name}")))?;
            d.read().await.id
        };

        self.deployments.remove(&deployment_id);
        info!(project, name, "deployment removed");
        Ok(())
    }

    pub async fn scale_deployment(
        &self,
        project: &str,
        name: &str,
        replicas: u32,
    ) -> Result<Deployment> {
        let deployment = self
            .find_deployment(project, name)
            .ok_or_else(|| NexaError::DeploymentNotFound(format!("{project}/{name}")))?;

        let deployment_id = {
            let mut d = deployment.write().await;
            d.spec.replicas = replicas;
            d.updated_at = chrono::Utc::now();
            d.id
        };

        self.reconcile_deployment(deployment_id).await?;

        let d = deployment.read().await;
        Ok(d.clone())
    }

    pub async fn pod_logs(&self, project: &str, name: &str, tail: Option<u64>) -> Result<LogStream> {
        let pod = self
            .pods
            .iter()
            .find(|entry| {
                let rt = tokio::runtime::Handle::current();
                let p = rt.block_on(entry.value().read());
                p.project == project && p.deployment_name == name
            })
            .ok_or_else(|| NexaError::PodNotFound(format!("{project}/{name}")))?;

        let p = pod.value().read().await;
        let container_id = p
            .container_id
            .as_ref()
            .ok_or_else(|| NexaError::Runtime("pod has no container".into()))?;

        self.runtime.logs(container_id, tail).await
    }

    fn find_deployment(&self, project: &str, name: &str) -> Option<Arc<RwLock<Deployment>>> {
        let rt = tokio::runtime::Handle::current();
        self.deployments.iter().find_map(|entry| {
            let d = rt.block_on(entry.value().read());
            if d.project() == project && d.name() == name {
                Some(entry.value().clone())
            } else {
                None
            }
        })
    }
}
