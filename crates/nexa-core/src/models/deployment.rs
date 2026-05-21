use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentSpec {
    pub project: String,
    pub deployment: DeploymentMeta,
    #[serde(default = "default_replicas")]
    pub replicas: u32,
    pub image: String,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub volumes: Vec<VolumeMount>,
    pub network: Option<NetworkConfig>,
    pub healthcheck: Option<HealthCheck>,
    #[serde(default)]
    pub restart: RestartPolicy,
}

fn default_replicas() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentMeta {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    pub name: String,
    pub mount_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    #[serde(default)]
    pub public: bool,
    pub domain: Option<String>,
    #[serde(default)]
    pub https: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub path: String,
    #[serde(default = "default_interval")]
    pub interval: String,
    #[serde(default = "default_timeout")]
    pub timeout: String,
    #[serde(default = "default_retries")]
    pub retries: u32,
}

fn default_interval() -> String {
    "10s".into()
}

fn default_timeout() -> String {
    "5s".into()
}

fn default_retries() -> u32 {
    3
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RestartPolicy {
    #[default]
    Always,
    OnFailure,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deployment {
    pub id: Uuid,
    pub spec: DeploymentSpec,
    pub status: DeploymentStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentStatus {
    Pending,
    Running,
    Degraded,
    Stopped,
    Failed,
}

impl Deployment {
    pub fn from_spec(spec: DeploymentSpec) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            spec,
            status: DeploymentStatus::Pending,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn project(&self) -> &str {
        &self.spec.project
    }

    pub fn name(&self) -> &str {
        &self.spec.deployment.name
    }
}
