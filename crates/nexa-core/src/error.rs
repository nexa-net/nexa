use thiserror::Error;

#[derive(Debug, Error)]
pub enum NexaError {
    #[error("project not found: {0}")]
    ProjectNotFound(String),

    #[error("deployment not found: {0}")]
    DeploymentNotFound(String),

    #[error("pod not found: {0}")]
    PodNotFound(String),

    #[error("container runtime error: {0}")]
    Runtime(String),

    #[error("invalid deployment spec: {0}")]
    InvalidSpec(String),

    #[error("port conflict: port {0} is already in use")]
    PortConflict(u16),

    #[error("image pull failed: {0}")]
    ImagePull(String),

    #[error("health check failed for {0}")]
    HealthCheckFailed(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

pub type Result<T> = std::result::Result<T, NexaError>;
