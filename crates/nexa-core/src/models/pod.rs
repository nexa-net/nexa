use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pod {
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub project: String,
    pub deployment_name: String,
    pub replica_index: u32,
    pub container_id: Option<String>,
    pub status: PodStatus,
    pub image: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PodStatus {
    Pending,
    Creating,
    Running,
    Stopping,
    Stopped,
    Failed,
    Restarting,
}

impl Pod {
    pub fn new(
        deployment_id: Uuid,
        project: &str,
        deployment_name: &str,
        replica_index: u32,
        image: &str,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            deployment_id,
            project: project.to_string(),
            deployment_name: deployment_name.to_string(),
            replica_index,
            container_id: None,
            status: PodStatus::Pending,
            image: image.to_string(),
            created_at: Utc::now(),
        }
    }

    pub fn container_name(&self) -> String {
        format!(
            "nexa-{}-{}-{}",
            self.project, self.deployment_name, self.replica_index
        )
    }
}

impl std::fmt::Display for PodStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PodStatus::Pending => write!(f, "Pending"),
            PodStatus::Creating => write!(f, "Creating"),
            PodStatus::Running => write!(f, "Running"),
            PodStatus::Stopping => write!(f, "Stopping"),
            PodStatus::Stopped => write!(f, "Stopped"),
            PodStatus::Failed => write!(f, "Failed"),
            PodStatus::Restarting => write!(f, "Restarting"),
        }
    }
}
