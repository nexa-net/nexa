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
