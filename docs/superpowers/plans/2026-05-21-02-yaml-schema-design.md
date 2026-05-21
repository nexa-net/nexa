# YAML Schema Design — Implementation Plan

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

**Goal:** Extend the YAML deployment spec with `secrets`, polymorphic `volumes` (named + bind mounts), `resources`, and strict DNS-safe validation rules for names and ports.

**Architecture:** `VolumeMount` is replaced by a `VolumeSpec` serde-untagged enum with `NamedVolume` and `BindMount` variants, each exposing `mount_point()` and `source_name()` methods. A new `ResourceSpec` struct handles `memory` + `cpu`. Validation moves from simple emptiness checks to regex-based DNS name validation, port range checks, and resource string parsing. The orchestrator's volume mapping is updated to use the new `VolumeSpec` API when building `VolumeBinding` for the container runtime.

**Tech Stack:** serde (untagged enum), regex (DNS validation), serde_yaml (parsing)

---

### Task 1: Add `secrets` field and `ResourceSpec` struct to deployment model

**Files:**
- Modify: `crates/nexa-core/src/domain/models/deployment.rs`
- Test: `crates/nexa-core/src/config.rs` (inline tests)

- [ ] **Step 1: Write failing tests for secrets and resources parsing**

Add these tests to the `#[cfg(test)] mod tests` block in `crates/nexa-core/src/config.rs`:

```rust
    #[test]
    fn parse_secrets_field() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
secrets:
  - DATABASE_URL
  - STRIPE_KEY
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.secrets, vec!["DATABASE_URL", "STRIPE_KEY"]);
    }

    #[test]
    fn parse_empty_secrets_defaults_to_empty() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert!(spec.secrets.is_empty());
    }

    #[test]
    fn parse_resources_field() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
resources:
  memory: 512m
  cpu: 0.5
"#;
        let spec = parse_deployment(yaml).unwrap();
        let res = spec.resources.unwrap();
        assert_eq!(res.memory, "512m");
        assert!((res.cpu - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_no_resources_defaults_to_none() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert!(spec.resources.is_none());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p nexa-core -- config::tests 2>&1`
Expected: FAIL — `DeploymentSpec` has no field `secrets` or `resources`

- [ ] **Step 3: Add `secrets`, `ResourceSpec`, and update `DeploymentSpec`**

Replace the full contents of `crates/nexa-core/src/domain/models/deployment.rs` with:

```rust
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
    pub secrets: Vec<String>,
    #[serde(default)]
    pub volumes: Vec<VolumeMount>,
    pub network: Option<NetworkConfig>,
    pub healthcheck: Option<HealthCheck>,
    #[serde(default)]
    pub restart: RestartPolicy,
    pub resources: Option<ResourceSpec>,
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
pub struct ResourceSpec {
    pub memory: String,
    pub cpu: f64,
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p nexa-core -- config::tests 2>&1`
Expected: all tests pass (existing + 4 new)

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-core/src/domain/models/deployment.rs crates/nexa-core/src/config.rs
git commit -m "feat: add secrets and resources fields to DeploymentSpec"
```

---

### Task 2: Replace `VolumeMount` with polymorphic `VolumeSpec`

**Files:**
- Modify: `crates/nexa-core/src/domain/models/deployment.rs`
- Test: `crates/nexa-core/src/config.rs` (inline tests)

- [ ] **Step 1: Write failing tests for the new volume format**

Add these tests to the `#[cfg(test)] mod tests` block in `crates/nexa-core/src/config.rs`:

```rust
    #[test]
    fn parse_named_volume() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
volumes:
  - name: data
    mount: /app/data
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.volumes.len(), 1);
        assert_eq!(spec.volumes[0].mount_point(), "/app/data");
        assert_eq!(spec.volumes[0].source_name(), "data");
        assert!(!spec.volumes[0].is_read_only());
    }

    #[test]
    fn parse_bind_mount_volume() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
volumes:
  - path: /host/uploads
    mount: /app/uploads
    readonly: true
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.volumes.len(), 1);
        assert_eq!(spec.volumes[0].mount_point(), "/app/uploads");
        assert_eq!(spec.volumes[0].source_name(), "/host/uploads");
        assert!(spec.volumes[0].is_read_only());
    }

    #[test]
    fn parse_mixed_volumes() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
volumes:
  - name: data
    mount: /app/data
  - path: /host/uploads
    mount: /app/uploads
    readonly: true
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.volumes.len(), 2);
        assert_eq!(spec.volumes[0].source_name(), "data");
        assert_eq!(spec.volumes[1].source_name(), "/host/uploads");
    }

    #[test]
    fn parse_bind_mount_readonly_defaults_false() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
volumes:
  - path: /host/data
    mount: /app/data
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert!(!spec.volumes[0].is_read_only());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p nexa-core -- config::tests 2>&1`
Expected: FAIL — `VolumeMount` does not have `mount_point()`, `source_name()`, `is_read_only()` methods, and YAML field `mount` does not match `mount_path`

- [ ] **Step 3: Replace `VolumeMount` with `VolumeSpec` enum**

In `crates/nexa-core/src/domain/models/deployment.rs`, remove the `VolumeMount` struct and replace it with the `VolumeSpec` enum. Replace these sections:

Remove:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    pub name: String,
    pub mount_path: String,
}
```

Add in its place:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VolumeSpec {
    Named(NamedVolume),
    Bind(BindMount),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedVolume {
    pub name: String,
    pub mount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindMount {
    pub path: String,
    pub mount: String,
    #[serde(default)]
    pub readonly: bool,
}

impl VolumeSpec {
    pub fn mount_point(&self) -> &str {
        match self {
            VolumeSpec::Named(v) => &v.mount,
            VolumeSpec::Bind(v) => &v.mount,
        }
    }

    pub fn source_name(&self) -> &str {
        match self {
            VolumeSpec::Named(v) => &v.name,
            VolumeSpec::Bind(v) => &v.path,
        }
    }

    pub fn is_read_only(&self) -> bool {
        match self {
            VolumeSpec::Named(_) => false,
            VolumeSpec::Bind(v) => v.readonly,
        }
    }
}
```

Also update the `volumes` field type in `DeploymentSpec`:

Change:
```rust
    #[serde(default)]
    pub volumes: Vec<VolumeMount>,
```
To:
```rust
    #[serde(default)]
    pub volumes: Vec<VolumeSpec>,
```

- [ ] **Step 4: Update the existing `parse_full_spec` test for new volume format**

The existing `parse_full_spec` test in `crates/nexa-core/src/config.rs` does not use volumes, so it should still pass without changes. However, verify the existing test suite compiles.

Run: `cargo test -p nexa-core -- config::tests 2>&1`
Expected: all tests pass (existing + 4 new volume tests)

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-core/src/domain/models/deployment.rs crates/nexa-core/src/config.rs
git commit -m "feat: replace VolumeMount with polymorphic VolumeSpec (named + bind mount)"
```

---

### Task 3: Add DNS-safe name validation and port range checks

**Files:**
- Modify: `crates/nexa-core/src/config.rs`
- Modify: `crates/nexa-core/Cargo.toml`
- Test: `crates/nexa-core/src/config.rs` (inline tests)

- [ ] **Step 1: Add `regex` dependency to nexa-core**

In `crates/nexa-core/Cargo.toml`, add `regex` to `[dependencies]`:

```toml
[package]
name = "nexa-core"
description = "Core types, traits, and abstractions for NexaNet"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
serde_yaml = { workspace = true }
thiserror = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }
async-trait = { workspace = true }
tracing = { workspace = true }
bollard = { workspace = true }
tokio = { workspace = true }
futures = { workspace = true }
regex = "1"
```

If `regex` is already a workspace dependency, use `regex = { workspace = true }` instead.

- [ ] **Step 2: Write failing tests for validation rules**

Add these tests to the `#[cfg(test)] mod tests` block in `crates/nexa-core/src/config.rs`:

```rust
    #[test]
    fn reject_uppercase_project_name() {
        let yaml = r#"
project: MyApp
deployment:
  name: api
image: nginx
"#;
        let err = parse_deployment(yaml).unwrap_err();
        assert!(err.to_string().contains("DNS-safe"));
    }

    #[test]
    fn reject_project_starting_with_hyphen() {
        let yaml = r#"
project: -myapp
deployment:
  name: api
image: nginx
"#;
        let err = parse_deployment(yaml).unwrap_err();
        assert!(err.to_string().contains("DNS-safe"));
    }

    #[test]
    fn reject_project_name_too_long() {
        let long_name = "a".repeat(64);
        let yaml = format!(
            r#"
project: {long_name}
deployment:
  name: api
image: nginx
"#
        );
        let err = parse_deployment(&yaml).unwrap_err();
        assert!(err.to_string().contains("63 characters"));
    }

    #[test]
    fn reject_deployment_name_with_underscore() {
        let yaml = r#"
project: myapp
deployment:
  name: my_api
image: nginx
"#;
        let err = parse_deployment(yaml).unwrap_err();
        assert!(err.to_string().contains("DNS-safe"));
    }

    #[test]
    fn accept_valid_dns_names() {
        let yaml = r#"
project: my-app-123
deployment:
  name: api-v2
image: nginx:latest
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.project, "my-app-123");
        assert_eq!(spec.deployment.name, "api-v2");
    }

    #[test]
    fn reject_port_zero() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx
ports:
  - 0
"#;
        let err = parse_deployment(yaml).unwrap_err();
        assert!(err.to_string().contains("port"));
    }

    #[test]
    fn accept_valid_port_range() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx
ports:
  - 1
  - 8080
  - 65535
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.ports, vec![1, 8080, 65535]);
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p nexa-core -- config::tests 2>&1`
Expected: FAIL — uppercase names, hyphens, and port 0 are not rejected by current validation

- [ ] **Step 4: Implement DNS-safe validation and port checks**

Replace the full contents of `crates/nexa-core/src/config.rs` with:

```rust
use std::path::Path;

use regex::Regex;

use crate::domain::models::DeploymentSpec;
use crate::error::{NexaError, Result};

pub fn parse_deployment_file(path: &Path) -> Result<DeploymentSpec> {
    let content = std::fs::read_to_string(path)?;
    parse_deployment(&content)
}

pub fn parse_deployment(yaml: &str) -> Result<DeploymentSpec> {
    let spec: DeploymentSpec =
        serde_yaml::from_str(yaml).map_err(|e| NexaError::InvalidSpec(e.to_string()))?;
    validate_spec(&spec)?;
    Ok(spec)
}

fn validate_dns_name(value: &str, field: &str) -> Result<()> {
    if value.is_empty() {
        return Err(NexaError::InvalidSpec(format!("{field} is required")));
    }
    if value.len() > 63 {
        return Err(NexaError::InvalidSpec(format!(
            "{field} must be at most 63 characters, got {}",
            value.len()
        )));
    }
    let dns_re = Regex::new(r"^[a-z0-9][a-z0-9-]*$").unwrap();
    if !dns_re.is_match(value) {
        return Err(NexaError::InvalidSpec(format!(
            "{field} must be DNS-safe: start with [a-z0-9], then [a-z0-9-] only (got '{value}')"
        )));
    }
    Ok(())
}

fn validate_spec(spec: &DeploymentSpec) -> Result<()> {
    validate_dns_name(&spec.project, "project")?;
    validate_dns_name(&spec.deployment.name, "deployment name")?;

    if spec.image.is_empty() {
        return Err(NexaError::InvalidSpec("image is required".into()));
    }
    if spec.replicas == 0 {
        return Err(NexaError::InvalidSpec(
            "replicas must be at least 1".into(),
        ));
    }

    for &port in &spec.ports {
        if port == 0 {
            return Err(NexaError::InvalidSpec(
                "port must be between 1 and 65535, got 0".into(),
            ));
        }
    }

    if let Some(ref res) = spec.resources {
        validate_resource_memory(&res.memory)?;
        if res.cpu <= 0.0 {
            return Err(NexaError::InvalidSpec(
                "resources.cpu must be greater than 0".into(),
            ));
        }
    }

    Ok(())
}

fn validate_resource_memory(memory: &str) -> Result<()> {
    if memory.is_empty() {
        return Err(NexaError::InvalidSpec(
            "resources.memory is required when resources is specified".into(),
        ));
    }
    let mem_re = Regex::new(r"^[0-9]+[kmgKMG]$").unwrap();
    if !mem_re.is_match(memory) {
        return Err(NexaError::InvalidSpec(format!(
            "resources.memory must match format like '512m', '1g', '256k' (got '{memory}')"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_spec() {
        let yaml = r#"
project: myapp

deployment:
  name: api

image: nginx:latest
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.project, "myapp");
        assert_eq!(spec.deployment.name, "api");
        assert_eq!(spec.image, "nginx:latest");
        assert_eq!(spec.replicas, 1);
    }

    #[test]
    fn parse_full_spec() {
        let yaml = r#"
project: ecommerce

deployment:
  name: api

replicas: 3
image: ghcr.io/company/api:latest

ports:
  - 3000

network:
  public: true
  domain: api.example.com
  https: true

env:
  DATABASE_URL: "postgres://localhost/db"
  REDIS_URL: "redis://localhost"

healthcheck:
  path: /health
  interval: 10s
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.replicas, 3);
        assert_eq!(spec.ports, vec![3000]);
        assert!(spec.network.as_ref().unwrap().public);
        assert_eq!(spec.env.len(), 2);
        assert_eq!(spec.healthcheck.as_ref().unwrap().path, "/health");
    }

    #[test]
    fn reject_empty_project() {
        let yaml = r#"
project: ""
deployment:
  name: api
image: nginx
"#;
        assert!(parse_deployment(yaml).is_err());
    }

    #[test]
    fn reject_zero_replicas() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx
replicas: 0
"#;
        assert!(parse_deployment(yaml).is_err());
    }

    #[test]
    fn parse_secrets_field() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
secrets:
  - DATABASE_URL
  - STRIPE_KEY
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.secrets, vec!["DATABASE_URL", "STRIPE_KEY"]);
    }

    #[test]
    fn parse_empty_secrets_defaults_to_empty() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert!(spec.secrets.is_empty());
    }

    #[test]
    fn parse_resources_field() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
resources:
  memory: 512m
  cpu: 0.5
"#;
        let spec = parse_deployment(yaml).unwrap();
        let res = spec.resources.unwrap();
        assert_eq!(res.memory, "512m");
        assert!((res.cpu - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_no_resources_defaults_to_none() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert!(spec.resources.is_none());
    }

    #[test]
    fn parse_named_volume() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
volumes:
  - name: data
    mount: /app/data
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.volumes.len(), 1);
        assert_eq!(spec.volumes[0].mount_point(), "/app/data");
        assert_eq!(spec.volumes[0].source_name(), "data");
        assert!(!spec.volumes[0].is_read_only());
    }

    #[test]
    fn parse_bind_mount_volume() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
volumes:
  - path: /host/uploads
    mount: /app/uploads
    readonly: true
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.volumes.len(), 1);
        assert_eq!(spec.volumes[0].mount_point(), "/app/uploads");
        assert_eq!(spec.volumes[0].source_name(), "/host/uploads");
        assert!(spec.volumes[0].is_read_only());
    }

    #[test]
    fn parse_mixed_volumes() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
volumes:
  - name: data
    mount: /app/data
  - path: /host/uploads
    mount: /app/uploads
    readonly: true
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.volumes.len(), 2);
        assert_eq!(spec.volumes[0].source_name(), "data");
        assert_eq!(spec.volumes[1].source_name(), "/host/uploads");
    }

    #[test]
    fn parse_bind_mount_readonly_defaults_false() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx:latest
volumes:
  - path: /host/data
    mount: /app/data
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert!(!spec.volumes[0].is_read_only());
    }

    #[test]
    fn reject_uppercase_project_name() {
        let yaml = r#"
project: MyApp
deployment:
  name: api
image: nginx
"#;
        let err = parse_deployment(yaml).unwrap_err();
        assert!(err.to_string().contains("DNS-safe"));
    }

    #[test]
    fn reject_project_starting_with_hyphen() {
        let yaml = r#"
project: -myapp
deployment:
  name: api
image: nginx
"#;
        let err = parse_deployment(yaml).unwrap_err();
        assert!(err.to_string().contains("DNS-safe"));
    }

    #[test]
    fn reject_project_name_too_long() {
        let long_name = "a".repeat(64);
        let yaml = format!(
            r#"
project: {long_name}
deployment:
  name: api
image: nginx
"#
        );
        let err = parse_deployment(&yaml).unwrap_err();
        assert!(err.to_string().contains("63 characters"));
    }

    #[test]
    fn reject_deployment_name_with_underscore() {
        let yaml = r#"
project: myapp
deployment:
  name: my_api
image: nginx
"#;
        let err = parse_deployment(yaml).unwrap_err();
        assert!(err.to_string().contains("DNS-safe"));
    }

    #[test]
    fn accept_valid_dns_names() {
        let yaml = r#"
project: my-app-123
deployment:
  name: api-v2
image: nginx:latest
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.project, "my-app-123");
        assert_eq!(spec.deployment.name, "api-v2");
    }

    #[test]
    fn reject_port_zero() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx
ports:
  - 0
"#;
        let err = parse_deployment(yaml).unwrap_err();
        assert!(err.to_string().contains("port"));
    }

    #[test]
    fn accept_valid_port_range() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx
ports:
  - 1
  - 8080
  - 65535
"#;
        let spec = parse_deployment(yaml).unwrap();
        assert_eq!(spec.ports, vec![1, 8080, 65535]);
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p nexa-core -- config::tests 2>&1`
Expected: all 20 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/nexa-core/Cargo.toml crates/nexa-core/src/config.rs
git commit -m "feat: add DNS-safe name validation and port range checks"
```

---

### Task 4: Add resource validation tests and edge cases

**Files:**
- Modify: `crates/nexa-core/src/config.rs`
- Test: `crates/nexa-core/src/config.rs` (inline tests)

- [ ] **Step 1: Write failing tests for resource validation**

Add these tests to the `#[cfg(test)] mod tests` block in `crates/nexa-core/src/config.rs`:

```rust
    #[test]
    fn reject_invalid_memory_format() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx
resources:
  memory: 512mb
  cpu: 0.5
"#;
        let err = parse_deployment(yaml).unwrap_err();
        assert!(err.to_string().contains("resources.memory"));
    }

    #[test]
    fn reject_zero_cpu() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx
resources:
  memory: 512m
  cpu: 0.0
"#;
        let err = parse_deployment(yaml).unwrap_err();
        assert!(err.to_string().contains("resources.cpu"));
    }

    #[test]
    fn reject_negative_cpu() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx
resources:
  memory: 512m
  cpu: -1.0
"#;
        let err = parse_deployment(yaml).unwrap_err();
        assert!(err.to_string().contains("resources.cpu"));
    }

    #[test]
    fn accept_valid_memory_formats() {
        for mem in &["512m", "1g", "256k", "2G", "100M", "64K"] {
            let yaml = format!(
                r#"
project: myapp
deployment:
  name: api
image: nginx
resources:
  memory: {mem}
  cpu: 1.0
"#
            );
            let spec = parse_deployment(&yaml).unwrap();
            assert_eq!(spec.resources.as_ref().unwrap().memory, *mem);
        }
    }

    #[test]
    fn reject_empty_memory() {
        let yaml = r#"
project: myapp
deployment:
  name: api
image: nginx
resources:
  memory: ""
  cpu: 1.0
"#;
        let err = parse_deployment(yaml).unwrap_err();
        assert!(err.to_string().contains("resources.memory"));
    }
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p nexa-core -- config::tests 2>&1`
Expected: all 25 tests pass (the validation logic from Task 3 already covers these cases)

- [ ] **Step 3: Commit**

```bash
git add crates/nexa-core/src/config.rs
git commit -m "test: add resource validation edge case tests"
```

---

### Task 5: Update orchestrator volume mapping for new VolumeSpec API

**Files:**
- Modify: `crates/nexa-core/src/domain/orchestrator.rs`
- Test: `crates/nexa-core/src/domain/orchestrator.rs` (inline tests)

- [ ] **Step 1: Write failing test for volume mapping**

Add this test to the `tests` module in `crates/nexa-core/src/domain/orchestrator.rs`:

```rust
    #[tokio::test]
    async fn deploy_maps_volume_spec_to_volume_binding() {
        use std::sync::Mutex;

        struct CapturingRuntime {
            configs: Mutex<Vec<ContainerConfig>>,
        }

        #[async_trait::async_trait]
        impl ContainerRuntime for CapturingRuntime {
            async fn pull_image(&self, _image: &str) -> Result<()> { Ok(()) }
            async fn create_container(&self, config: &ContainerConfig) -> Result<String> {
                self.configs.lock().unwrap().push(config.clone());
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

        let runtime = Arc::new(CapturingRuntime {
            configs: Mutex::new(Vec::new()),
        });
        let handle = Orchestrator::spawn(runtime.clone());

        let spec = DeploymentSpec {
            project: "test".into(),
            deployment: DeploymentMeta { name: "api".into() },
            replicas: 1,
            image: "nginx".into(),
            ports: vec![],
            env: HashMap::new(),
            secrets: vec![],
            volumes: vec![
                VolumeSpec::Named(NamedVolume {
                    name: "data".into(),
                    mount: "/app/data".into(),
                }),
                VolumeSpec::Bind(BindMount {
                    path: "/host/uploads".into(),
                    mount: "/app/uploads".into(),
                    readonly: true,
                }),
            ],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
            resources: None,
        };

        handle.deploy(spec).await.unwrap();

        let configs = runtime.configs.lock().unwrap();
        assert_eq!(configs.len(), 1);

        let vols = &configs[0].volumes;
        assert_eq!(vols.len(), 2);

        assert_eq!(vols[0].source, "data");
        assert_eq!(vols[0].target, "/app/data");
        assert!(!vols[0].read_only);

        assert_eq!(vols[1].source, "/host/uploads");
        assert_eq!(vols[1].target, "/app/uploads");
        assert!(vols[1].read_only);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nexa-core -- domain::orchestrator::tests::deploy_maps_volume_spec 2>&1`
Expected: FAIL — the `create_pod` method references `v.name` and `v.mount_path` which no longer exist on `VolumeSpec`, and `DeploymentSpec` construction in test data is missing `secrets` and `resources` fields

- [ ] **Step 3: Update the `create_pod` volume mapping in orchestrator.rs**

In the `create_pod` method of the `Orchestrator` impl in `crates/nexa-core/src/domain/orchestrator.rs`, find the volume mapping block:

```rust
            volumes: spec
                .volumes
                .iter()
                .map(|v| crate::ports::runtime::VolumeBinding {
                    source: v.name.clone(),
                    target: v.mount_path.clone(),
                    read_only: false,
                })
                .collect(),
```

Replace it with:

```rust
            volumes: spec
                .volumes
                .iter()
                .map(|v| crate::ports::runtime::VolumeBinding {
                    source: v.source_name().to_string(),
                    target: v.mount_point().to_string(),
                    read_only: v.is_read_only(),
                })
                .collect(),
```

- [ ] **Step 4: Update all existing test DeploymentSpec constructions**

In the `tests` module of `crates/nexa-core/src/domain/orchestrator.rs`, every `DeploymentSpec { ... }` construction must be updated to include the new `secrets` and `resources` fields. Update each instance to include:

```rust
            secrets: vec![],
            resources: None,
```

There are 4 existing tests that construct `DeploymentSpec`: `deploy_creates_pods`, `list_projects_returns_auto_created`, `scale_changes_pod_count`, and `stop_removes_pods`. In each one, add the two new fields after `volumes: vec![],`.

For example, in `deploy_creates_pods`:
```rust
        let spec = DeploymentSpec {
            project: "test".into(),
            deployment: DeploymentMeta { name: "api".into() },
            replicas: 2,
            image: "nginx:latest".into(),
            ports: vec![8080],
            env: HashMap::new(),
            secrets: vec![],
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
            resources: None,
        };
```

In `list_projects_returns_auto_created`:
```rust
        let spec = DeploymentSpec {
            project: "myapp".into(),
            deployment: DeploymentMeta { name: "web".into() },
            replicas: 1,
            image: "nginx".into(),
            ports: vec![],
            env: HashMap::new(),
            secrets: vec![],
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
            resources: None,
        };
```

In `scale_changes_pod_count`:
```rust
        let spec = DeploymentSpec {
            project: "test".into(),
            deployment: DeploymentMeta { name: "api".into() },
            replicas: 1,
            image: "nginx".into(),
            ports: vec![],
            env: HashMap::new(),
            secrets: vec![],
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
            resources: None,
        };
```

In `stop_removes_pods`:
```rust
        let spec = DeploymentSpec {
            project: "test".into(),
            deployment: DeploymentMeta { name: "api".into() },
            replicas: 2,
            image: "nginx".into(),
            ports: vec![],
            env: HashMap::new(),
            secrets: vec![],
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
            resources: None,
        };
```

- [ ] **Step 5: Run all tests to verify they pass**

Run: `cargo test -p nexa-core -- domain::orchestrator 2>&1`
Expected: all 7 tests pass (6 existing + 1 new volume mapping test)

- [ ] **Step 6: Commit**

```bash
git add crates/nexa-core/src/domain/orchestrator.rs
git commit -m "feat: update orchestrator volume mapping to use VolumeSpec API"
```

---

### Task 6: Update nexad orchestrator for new DeploymentSpec fields

**Files:**
- Modify: `crates/nexad/src/engine/orchestrator.rs`

- [ ] **Step 1: Update volume mapping in nexad orchestrator**

In `crates/nexad/src/engine/orchestrator.rs`, in the `create_pod` method, find the volume mapping block (around line 234):

```rust
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
```

Replace the `volumes:` block with:

```rust
            volumes: spec
                .volumes
                .iter()
                .map(|v| VolumeBinding {
                    source: v.source_name().to_string(),
                    target: v.mount_point().to_string(),
                    read_only: v.is_read_only(),
                })
                .collect(),
```

- [ ] **Step 2: Verify nexad compiles**

Run: `cargo check -p nexad 2>&1`
Expected: compiles (warnings OK)

- [ ] **Step 3: Commit**

```bash
git add crates/nexad/src/engine/orchestrator.rs
git commit -m "fix: update nexad orchestrator volume mapping for VolumeSpec API"
```

---

### Task 7: Full-spec integration test and final verification

**Files:**
- Modify: `crates/nexa-core/src/config.rs`
- Test: `crates/nexa-core/src/config.rs` (inline tests)

- [ ] **Step 1: Add the full target YAML integration test**

Add this test to the `#[cfg(test)] mod tests` block in `crates/nexa-core/src/config.rs`:

```rust
    #[test]
    fn parse_complete_target_yaml() {
        let yaml = r#"
project: ecommerce
deployment:
  name: api
replicas: 3
image: ghcr.io/company/api:latest
ports:
  - 3000
env:
  NODE_ENV: production
secrets:
  - DATABASE_URL
  - STRIPE_KEY
volumes:
  - name: data
    mount: /app/data
  - path: /host/uploads
    mount: /app/uploads
    readonly: true
network:
  public: true
  domain: api.example.com
  https: true
healthcheck:
  path: /health
  interval: 10s
  timeout: 5s
  retries: 3
restart: always
resources:
  memory: 512m
  cpu: 0.5
"#;
        let spec = parse_deployment(yaml).unwrap();

        assert_eq!(spec.project, "ecommerce");
        assert_eq!(spec.deployment.name, "api");
        assert_eq!(spec.replicas, 3);
        assert_eq!(spec.image, "ghcr.io/company/api:latest");
        assert_eq!(spec.ports, vec![3000]);
        assert_eq!(spec.env.get("NODE_ENV").unwrap(), "production");
        assert_eq!(spec.secrets, vec!["DATABASE_URL", "STRIPE_KEY"]);

        assert_eq!(spec.volumes.len(), 2);
        assert_eq!(spec.volumes[0].source_name(), "data");
        assert_eq!(spec.volumes[0].mount_point(), "/app/data");
        assert!(!spec.volumes[0].is_read_only());
        assert_eq!(spec.volumes[1].source_name(), "/host/uploads");
        assert_eq!(spec.volumes[1].mount_point(), "/app/uploads");
        assert!(spec.volumes[1].is_read_only());

        let net = spec.network.unwrap();
        assert!(net.public);
        assert_eq!(net.domain.unwrap(), "api.example.com");
        assert!(net.https);

        let hc = spec.healthcheck.unwrap();
        assert_eq!(hc.path, "/health");
        assert_eq!(hc.interval, "10s");
        assert_eq!(hc.timeout, "5s");
        assert_eq!(hc.retries, 3);

        let res = spec.resources.unwrap();
        assert_eq!(res.memory, "512m");
        assert!((res.cpu - 0.5).abs() < f64::EPSILON);
    }
```

- [ ] **Step 2: Run all config tests**

Run: `cargo test -p nexa-core -- config::tests 2>&1`
Expected: all 26 tests pass

- [ ] **Step 3: Run full workspace test suite**

Run: `cargo test 2>&1`
Expected: all tests across all crates pass

- [ ] **Step 4: Verify workspace compiles clean**

Run: `cargo check 2>&1`
Expected: compiles (warnings OK)

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-core/src/config.rs
git commit -m "test: add full target YAML integration test for complete schema"
```

- [ ] **Step 6: Push to remote**

```bash
git push origin main
```
