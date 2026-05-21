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

use super::traits::*;
use crate::error::{NexaError, Result};

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

#[async_trait]
impl ContainerRuntime for DockerRuntime {
    async fn pull_image(&self, image: &str) -> Result<()> {
        info!(image, "pulling image");

        let (repo, tag) = match image.rsplit_once(':') {
            Some((r, t)) => (r.to_string(), t.to_string()),
            None => (image.to_string(), "latest".to_string()),
        };

        let options = CreateImageOptions {
            from_image: repo,
            tag,
            ..Default::default()
        };

        let mut stream = self.client.create_image(Some(options), None, None);
        while let Some(result) = stream.next().await {
            result.map_err(|e| NexaError::ImagePull(e.to_string()))?;
        }

        info!(image, "image pulled");
        Ok(())
    }

    async fn create_container(&self, config: &ContainerConfig) -> Result<String> {
        debug!(name = config.name, image = config.image, "creating container");

        let env: Vec<String> = config
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();

        let mut port_bindings: HashMap<String, Option<Vec<BollardPortBinding>>> = HashMap::new();
        let mut exposed_ports: HashMap<String, HashMap<(), ()>> = HashMap::new();

        for port in &config.ports {
            let key = format!("{}/tcp", port.container_port);
            exposed_ports.insert(key.clone(), HashMap::new());
            port_bindings.insert(
                key,
                Some(vec![BollardPortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: port.host_port.map(|p| p.to_string()),
                }]),
            );
        }

        let binds: Vec<String> = config
            .volumes
            .iter()
            .map(|v| {
                if v.read_only {
                    format!("{}:{}:ro", v.source, v.target)
                } else {
                    format!("{}:{}", v.source, v.target)
                }
            })
            .collect();

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            binds: Some(binds),
            network_mode: config.network.clone(),
            ..Default::default()
        };

        let container_config = Config {
            image: Some(config.image.clone()),
            env: Some(env),
            exposed_ports: Some(exposed_ports),
            labels: Some(config.labels.clone()),
            host_config: Some(host_config),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: &config.name,
            platform: None,
        };

        let response = self
            .client
            .create_container(Some(options), container_config)
            .await
            .map_err(|e| NexaError::Runtime(e.to_string()))?;

        info!(id = response.id, name = config.name, "container created");
        Ok(response.id)
    }

    async fn start_container(&self, id: &str) -> Result<()> {
        self.client
            .start_container::<String>(id, None)
            .await
            .map_err(|e| NexaError::Runtime(e.to_string()))?;

        debug!(id, "container started");
        Ok(())
    }

    async fn stop_container(&self, id: &str, timeout_secs: u64) -> Result<()> {
        let options = StopContainerOptions {
            t: timeout_secs as i64,
        };

        self.client
            .stop_container(id, Some(options))
            .await
            .map_err(|e| NexaError::Runtime(e.to_string()))?;

        debug!(id, "container stopped");
        Ok(())
    }

    async fn remove_container(&self, id: &str, force: bool) -> Result<()> {
        let options = RemoveContainerOptions {
            force,
            v: true,
            ..Default::default()
        };

        self.client
            .remove_container(id, Some(options))
            .await
            .map_err(|e| NexaError::Runtime(e.to_string()))?;

        debug!(id, "container removed");
        Ok(())
    }

    async fn inspect_container(&self, id: &str) -> Result<ContainerInfo> {
        let info = self
            .client
            .inspect_container(id, None)
            .await
            .map_err(|e| NexaError::Runtime(e.to_string()))?;

        let state = match info.state.and_then(|s| s.status) {
            Some(bollard::models::ContainerStateStatusEnum::RUNNING) => ContainerState::Running,
            Some(bollard::models::ContainerStateStatusEnum::CREATED) => ContainerState::Created,
            Some(bollard::models::ContainerStateStatusEnum::EXITED) => ContainerState::Exited,
            Some(bollard::models::ContainerStateStatusEnum::PAUSED) => ContainerState::Paused,
            Some(bollard::models::ContainerStateStatusEnum::RESTARTING) => {
                ContainerState::Restarting
            }
            Some(bollard::models::ContainerStateStatusEnum::REMOVING) => ContainerState::Removing,
            Some(bollard::models::ContainerStateStatusEnum::DEAD) => ContainerState::Dead,
            _ => ContainerState::Unknown,
        };

        Ok(ContainerInfo {
            id: info.id.unwrap_or_default(),
            name: info.name.unwrap_or_default().trim_start_matches('/').to_string(),
            image: info
                .config
                .and_then(|c| c.image)
                .unwrap_or_default(),
            state,
        })
    }

    async fn logs(&self, id: &str, tail: Option<u64>) -> Result<LogStream> {
        let options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            follow: true,
            tail: tail.map(|t| t.to_string()).unwrap_or("all".to_string()),
            ..Default::default()
        };

        let stream = self.client.logs(id, Some(options));

        let mapped = stream.map(|result| match result {
            Ok(output) => Ok(output.to_string()),
            Err(e) => Err(NexaError::Runtime(e.to_string())),
        });

        Ok(Box::pin(mapped))
    }

    async fn container_exists(&self, name: &str) -> Result<bool> {
        let filters: HashMap<&str, Vec<&str>> =
            HashMap::from([("name", vec![name])]);
        let options = ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        };

        let containers = self
            .client
            .list_containers(Some(options))
            .await
            .map_err(|e| NexaError::Runtime(e.to_string()))?;

        let full_name = format!("/{name}");
        Ok(containers
            .iter()
            .any(|c| c.names.as_ref().is_some_and(|n| n.contains(&full_name))))
    }

    async fn create_network(&self, name: &str) -> Result<String> {
        let options = CreateNetworkOptions {
            name: name.to_string(),
            driver: "bridge".to_string(),
            labels: HashMap::from([("managed-by".to_string(), "nexanet".to_string())]),
            ..Default::default()
        };

        let response = self
            .client
            .create_network(options)
            .await
            .map_err(|e| NexaError::Runtime(e.to_string()))?;

        info!(name, "network created");
        Ok(response.id)
    }

    async fn remove_network(&self, name: &str) -> Result<()> {
        self.client
            .remove_network(name)
            .await
            .map_err(|e| NexaError::Runtime(e.to_string()))?;

        debug!(name, "network removed");
        Ok(())
    }

    async fn connect_to_network(&self, container_id: &str, network: &str) -> Result<()> {
        let options = ConnectNetworkOptions {
            container: container_id.to_string(),
            endpoint_config: EndpointSettings::default(),
        };

        self.client
            .connect_network(network, options)
            .await
            .map_err(|e| NexaError::Runtime(e.to_string()))?;

        debug!(container_id, network, "connected to network");
        Ok(())
    }
}
