# Health Checking — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an active health-checking subsystem that probes running pods over HTTP, tracks per-pod health state through a Healthy/Failing/Unhealthy state machine, and triggers restart actions when a pod exceeds its failure threshold.

**Architecture:** A dedicated `HealthChecker` actor runs as a `tokio::spawn`ed task alongside the orchestrator. Every second it collects pods due for a probe, spawns parallel HTTP `GET` requests (one per pod, with per-pod timeout), and sends `Command::HealthReport` messages back to the orchestrator via its existing `mpsc` command channel. The orchestrator maintains an in-memory `HashMap<Uuid, HealthState>` (transient, not persisted) and advances the state machine on each report — resetting to Healthy on success, incrementing failure count on failure, and triggering a pod restart when consecutive failures reach the configured `retries` threshold.

**Tech Stack:** `reqwest` (workspace dep, HTTP probes), `tokio` (timers, spawn, mpsc), `bollard` (container IP inspection), `std::time::Duration` (interval/timeout parsing)

---

### Task 1: Add `container_ip` to Pod Model + `container_ip()` to ContainerRuntime Trait

**Files:**
- Modify: `crates/nexa-core/src/models/pod.rs`
- Modify: `crates/nexa-core/src/runtime/traits.rs`
- Modify: `crates/nexa-core/src/runtime/docker.rs`

- [ ] **Step 1: Write failing test — Pod serialization includes `container_ip`**

  In `crates/nexa-core/src/models/pod.rs`, add a `#[cfg(test)]` module at the bottom:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn pod_container_ip_defaults_to_none() {
          let pod = Pod::new(
              Uuid::new_v4(),
              "proj",
              "deploy",
              0,
              "nginx:latest",
          );
          assert!(pod.container_ip.is_none());
      }

      #[test]
      fn pod_serialization_roundtrip_with_ip() {
          let mut pod = Pod::new(
              Uuid::new_v4(),
              "proj",
              "deploy",
              0,
              "nginx:latest",
          );
          pod.container_ip = Some("172.17.0.2".to_string());

          let json = serde_json::to_string(&pod).unwrap();
          let deserialized: Pod = serde_json::from_str(&json).unwrap();
          assert_eq!(deserialized.container_ip.as_deref(), Some("172.17.0.2"));
      }
  }
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo test -p nexa-core pod`
  Expected: **FAILS** — `Pod` has no field `container_ip`.

- [ ] **Step 2: Add `container_ip` field to Pod struct**

  In `crates/nexa-core/src/models/pod.rs`, add the field to `Pod`:

  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct Pod {
      pub id: Uuid,
      pub deployment_id: Uuid,
      pub project: String,
      pub deployment_name: String,
      pub replica_index: u32,
      pub container_id: Option<String>,
      pub container_ip: Option<String>,
      pub status: PodStatus,
      pub image: String,
      pub created_at: DateTime<Utc>,
  }
  ```

  Update `Pod::new()` to initialize `container_ip: None`.

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo test -p nexa-core pod`
  Expected: **PASSES** — both tests green.

- [ ] **Step 3: Add `container_ip()` method to ContainerRuntime trait**

  In `crates/nexa-core/src/runtime/traits.rs`, add to the trait:

  ```rust
  async fn container_ip(&self, container_id: &str, network: &str) -> Result<String>;
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo check -p nexa-core`
  Expected: **FAILS** — `DockerRuntime` doesn't implement the new method.

- [ ] **Step 4: Implement `container_ip()` in DockerRuntime**

  In `crates/nexa-core/src/runtime/docker.rs`, add:

  ```rust
  async fn container_ip(&self, container_id: &str, network: &str) -> Result<String> {
      let info = self
          .client
          .inspect_container(container_id, None)
          .await
          .map_err(|e| NexaError::Runtime(e.to_string()))?;

      let ip = info
          .network_settings
          .and_then(|ns| ns.networks)
          .and_then(|mut nets| nets.remove(network))
          .and_then(|ep| ep.ip_address)
          .filter(|ip| !ip.is_empty())
          .ok_or_else(|| {
              NexaError::Runtime(format!(
                  "no IP found for container {container_id} on network {network}"
              ))
          })?;

      Ok(ip)
  }
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo check`
  Expected: **PASSES** — full workspace compiles.

- [ ] **Step 5: Populate `container_ip` after pod creation in orchestrator**

  In `crates/nexad/src/engine/orchestrator.rs`, inside `create_pod()`, after the successful `start_container` call (inside the `Ok(container_id)` arm), add IP lookup:

  ```rust
  Ok(container_id) => {
      self.runtime.start_container(&container_id).await?;
      pod.container_id = Some(container_id.clone());
      pod.status = PodStatus::Running;

      // Populate container IP for health checking
      let network_name = format!("nexa-{}", spec.project);
      match self.runtime.container_ip(&container_id, &network_name).await {
          Ok(ip) => {
              pod.container_ip = Some(ip);
          }
          Err(e) => {
              warn!(name = container_name, error = %e, "failed to get container IP");
          }
      }

      info!(name = container_name, "pod running");
  }
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo build`
  Expected: **PASSES** — compiles cleanly.

- [ ] **Step 6: Commit**

  ```bash
  cd /Users/nassime/GitHub/NexaNet
  git add crates/nexa-core/src/models/pod.rs crates/nexa-core/src/runtime/traits.rs crates/nexa-core/src/runtime/docker.rs crates/nexad/src/engine/orchestrator.rs
  git commit -m "feat(core): add container_ip to Pod model and ContainerRuntime trait"
  ```

---

### Task 2: Duration Parsing Utility

**Files:**
- Create: `crates/nexa-core/src/duration.rs`
- Modify: `crates/nexa-core/src/lib.rs`

- [ ] **Step 1: Write failing tests for duration parsing**

  Create `crates/nexa-core/src/duration.rs` with only tests:

  ```rust
  use std::time::Duration;

  use crate::error::{NexaError, Result};

  /// Parses a human-friendly duration string into `std::time::Duration`.
  ///
  /// Supported formats: `"10s"`, `"5m"`, `"1h"`, `"500ms"`.
  /// Bare integers (no suffix) are treated as seconds.
  pub fn parse_duration(s: &str) -> Result<Duration> {
      todo!()
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn parse_seconds() {
          assert_eq!(parse_duration("10s").unwrap(), Duration::from_secs(10));
          assert_eq!(parse_duration("1s").unwrap(), Duration::from_secs(1));
          assert_eq!(parse_duration("0s").unwrap(), Duration::from_secs(0));
      }

      #[test]
      fn parse_minutes() {
          assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
          assert_eq!(parse_duration("1m").unwrap(), Duration::from_secs(60));
      }

      #[test]
      fn parse_hours() {
          assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
      }

      #[test]
      fn parse_milliseconds() {
          assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
          assert_eq!(parse_duration("100ms").unwrap(), Duration::from_millis(100));
      }

      #[test]
      fn parse_bare_number_as_seconds() {
          assert_eq!(parse_duration("30").unwrap(), Duration::from_secs(30));
      }

      #[test]
      fn reject_empty_string() {
          assert!(parse_duration("").is_err());
      }

      #[test]
      fn reject_invalid_format() {
          assert!(parse_duration("abc").is_err());
          assert!(parse_duration("10x").is_err());
          assert!(parse_duration("s10").is_err());
      }
  }
  ```

  Add `pub mod duration;` to `crates/nexa-core/src/lib.rs`.

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo test -p nexa-core duration`
  Expected: **FAILS** — `todo!()` panics.

- [ ] **Step 2: Implement `parse_duration`**

  Replace the `todo!()` body of `parse_duration` in `crates/nexa-core/src/duration.rs`:

  ```rust
  pub fn parse_duration(s: &str) -> Result<Duration> {
      let s = s.trim();
      if s.is_empty() {
          return Err(NexaError::InvalidSpec("empty duration string".into()));
      }

      // Try "ms" suffix first (before "m" and "s")
      if let Some(num) = s.strip_suffix("ms") {
          let millis: u64 = num
              .parse()
              .map_err(|_| NexaError::InvalidSpec(format!("invalid duration: {s}")))?;
          return Ok(Duration::from_millis(millis));
      }

      let (num_str, multiplier) = if let Some(num) = s.strip_suffix('s') {
          (num, 1u64)
      } else if let Some(num) = s.strip_suffix('m') {
          (num, 60u64)
      } else if let Some(num) = s.strip_suffix('h') {
          (num, 3600u64)
      } else {
          // Bare number — treat as seconds
          (s, 1u64)
      };

      let value: u64 = num_str
          .parse()
          .map_err(|_| NexaError::InvalidSpec(format!("invalid duration: {s}")))?;

      Ok(Duration::from_secs(value * multiplier))
  }
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo test -p nexa-core duration`
  Expected: **PASSES** — all 7 tests green.

- [ ] **Step 3: Commit**

  ```bash
  cd /Users/nassime/GitHub/NexaNet
  git add crates/nexa-core/src/duration.rs crates/nexa-core/src/lib.rs
  git commit -m "feat(core): add duration parsing utility for healthcheck intervals"
  ```

---

### Task 3: Health State Machine + Domain Types

**Files:**
- Create: `crates/nexa-core/src/domain/health.rs`
- Create: `crates/nexa-core/src/domain/mod.rs`
- Modify: `crates/nexa-core/src/lib.rs`

- [ ] **Step 1: Write failing tests for health state machine transitions**

  Create directory: `mkdir -p crates/nexa-core/src/domain`

  Create `crates/nexa-core/src/domain/mod.rs`:

  ```rust
  pub mod health;
  ```

  Create `crates/nexa-core/src/domain/health.rs` with types and tests (impl bodies as `todo!()`):

  ```rust
  use std::collections::HashMap;
  use std::time::{Duration, Instant};

  use uuid::Uuid;

  /// Per-pod health state, following the state machine:
  ///
  /// ```text
  /// Healthy --(failure)--> Failing(count=1)
  ///    ^                        |
  ///    |                   (failure)
  /// (success)                   |
  ///    |                   Failing(count=2)
  ///    |                        |
  ///    |                  (count >= retries)
  ///    |                        v
  ///    +------------------ Unhealthy
  /// ```
  #[derive(Debug, Clone, PartialEq)]
  pub enum HealthState {
      Healthy,
      Failing { consecutive_failures: u32 },
      Unhealthy,
  }

  /// Configuration for health-checking a single pod.
  #[derive(Debug, Clone)]
  pub struct PodHealthConfig {
      pub pod_id: Uuid,
      pub container_ip: String,
      pub port: u16,
      pub path: String,
      pub interval: Duration,
      pub timeout: Duration,
      pub retries: u32,
  }

  /// Tracks health state and probe scheduling for all pods.
  #[derive(Debug)]
  pub struct HealthTracker {
      states: HashMap<Uuid, HealthState>,
      configs: HashMap<Uuid, PodHealthConfig>,
      last_probe: HashMap<Uuid, Instant>,
  }

  impl HealthTracker {
      pub fn new() -> Self {
          Self {
              states: HashMap::new(),
              configs: HashMap::new(),
              last_probe: HashMap::new(),
          }
      }

      /// Register a pod for health tracking.  Starts in `Healthy` state.
      pub fn register(&mut self, config: PodHealthConfig) {
          let pod_id = config.pod_id;
          self.states.insert(pod_id, HealthState::Healthy);
          self.configs.insert(pod_id, config);
          // Set last_probe to now so the first probe fires after one interval
          self.last_probe.insert(pod_id, Instant::now());
      }

      /// Remove a pod from health tracking (e.g. on stop/delete).
      pub fn unregister(&mut self, pod_id: &Uuid) {
          self.states.remove(pod_id);
          self.configs.remove(pod_id);
          self.last_probe.remove(pod_id);
      }

      /// Get the current health state for a pod.
      pub fn state(&self, pod_id: &Uuid) -> Option<&HealthState> {
          self.states.get(pod_id)
      }

      /// Get the config for a pod.
      pub fn config(&self, pod_id: &Uuid) -> Option<&PodHealthConfig> {
          self.configs.get(pod_id)
      }

      /// Return pod IDs that are due for a health probe right now.
      pub fn pods_due_for_probe(&self) -> Vec<Uuid> {
          let now = Instant::now();
          self.configs
              .iter()
              .filter(|(pod_id, config)| {
                  // Don't probe pods already marked Unhealthy
                  if self.states.get(pod_id) == Some(&HealthState::Unhealthy) {
                      return false;
                  }
                  match self.last_probe.get(pod_id) {
                      Some(last) => now.duration_since(*last) >= config.interval,
                      None => true,
                  }
              })
              .map(|(pod_id, _)| *pod_id)
              .collect()
      }

      /// Record that a probe was just sent for this pod (resets the interval timer).
      pub fn mark_probed(&mut self, pod_id: &Uuid) {
          self.last_probe.insert(*pod_id, Instant::now());
      }

      /// Process a health report.  Returns the new state and whether a restart
      /// should be triggered (`true` only on the transition *into* `Unhealthy`).
      pub fn record_result(&mut self, pod_id: &Uuid, healthy: bool) -> Option<(HealthState, bool)> {
          let retries = self.configs.get(pod_id)?.retries;
          let current = self.states.get(pod_id)?;

          let (new_state, trigger_restart) = if healthy {
              (HealthState::Healthy, false)
          } else {
              match current {
                  HealthState::Healthy => (
                      HealthState::Failing {
                          consecutive_failures: 1,
                      },
                      false,
                  ),
                  HealthState::Failing {
                      consecutive_failures,
                  } => {
                      let next_count = consecutive_failures + 1;
                      if next_count >= retries {
                          (HealthState::Unhealthy, true)
                      } else {
                          (
                              HealthState::Failing {
                                  consecutive_failures: next_count,
                              },
                              false,
                          )
                      }
                  }
                  HealthState::Unhealthy => (HealthState::Unhealthy, false),
              }
          };

          self.states.insert(*pod_id, new_state.clone());
          Some((new_state, trigger_restart))
      }

      /// Reset a pod back to Healthy (e.g. after a restart).
      pub fn reset(&mut self, pod_id: &Uuid) {
          if self.states.contains_key(pod_id) {
              self.states.insert(*pod_id, HealthState::Healthy);
              self.last_probe.insert(*pod_id, Instant::now());
          }
      }

      /// All registered pod IDs.
      pub fn tracked_pods(&self) -> Vec<Uuid> {
          self.configs.keys().copied().collect()
      }
  }

  #[cfg(test)]
  mod tests {
      use super::*;

      fn test_config(pod_id: Uuid) -> PodHealthConfig {
          PodHealthConfig {
              pod_id,
              container_ip: "172.17.0.2".into(),
              port: 3000,
              path: "/health".into(),
              interval: Duration::from_secs(10),
              timeout: Duration::from_secs(5),
              retries: 3,
          }
      }

      #[test]
      fn register_starts_healthy() {
          let mut tracker = HealthTracker::new();
          let pod_id = Uuid::new_v4();
          tracker.register(test_config(pod_id));
          assert_eq!(tracker.state(&pod_id), Some(&HealthState::Healthy));
      }

      #[test]
      fn success_keeps_healthy() {
          let mut tracker = HealthTracker::new();
          let pod_id = Uuid::new_v4();
          tracker.register(test_config(pod_id));

          let (state, restart) = tracker.record_result(&pod_id, true).unwrap();
          assert_eq!(state, HealthState::Healthy);
          assert!(!restart);
      }

      #[test]
      fn single_failure_transitions_to_failing() {
          let mut tracker = HealthTracker::new();
          let pod_id = Uuid::new_v4();
          tracker.register(test_config(pod_id));

          let (state, restart) = tracker.record_result(&pod_id, false).unwrap();
          assert_eq!(
              state,
              HealthState::Failing {
                  consecutive_failures: 1
              }
          );
          assert!(!restart);
      }

      #[test]
      fn consecutive_failures_increment() {
          let mut tracker = HealthTracker::new();
          let pod_id = Uuid::new_v4();
          tracker.register(test_config(pod_id));

          tracker.record_result(&pod_id, false); // count=1
          let (state, restart) = tracker.record_result(&pod_id, false).unwrap(); // count=2
          assert_eq!(
              state,
              HealthState::Failing {
                  consecutive_failures: 2
              }
          );
          assert!(!restart);
      }

      #[test]
      fn reaching_retries_transitions_to_unhealthy() {
          let mut tracker = HealthTracker::new();
          let pod_id = Uuid::new_v4();
          tracker.register(test_config(pod_id)); // retries=3

          tracker.record_result(&pod_id, false); // count=1
          tracker.record_result(&pod_id, false); // count=2
          let (state, restart) = tracker.record_result(&pod_id, false).unwrap(); // count=3 >= 3
          assert_eq!(state, HealthState::Unhealthy);
          assert!(restart); // triggers restart
      }

      #[test]
      fn success_resets_from_failing_to_healthy() {
          let mut tracker = HealthTracker::new();
          let pod_id = Uuid::new_v4();
          tracker.register(test_config(pod_id));

          tracker.record_result(&pod_id, false); // Failing(1)
          tracker.record_result(&pod_id, false); // Failing(2)
          let (state, restart) = tracker.record_result(&pod_id, true).unwrap(); // back to Healthy
          assert_eq!(state, HealthState::Healthy);
          assert!(!restart);
      }

      #[test]
      fn unhealthy_does_not_re_trigger_restart() {
          let mut tracker = HealthTracker::new();
          let pod_id = Uuid::new_v4();
          tracker.register(test_config(pod_id));

          // Drive to Unhealthy
          tracker.record_result(&pod_id, false);
          tracker.record_result(&pod_id, false);
          tracker.record_result(&pod_id, false); // now Unhealthy, restart=true

          // Another failure should not trigger restart again
          let (state, restart) = tracker.record_result(&pod_id, false).unwrap();
          assert_eq!(state, HealthState::Unhealthy);
          assert!(!restart);
      }

      #[test]
      fn reset_restores_healthy() {
          let mut tracker = HealthTracker::new();
          let pod_id = Uuid::new_v4();
          tracker.register(test_config(pod_id));

          // Drive to Unhealthy
          tracker.record_result(&pod_id, false);
          tracker.record_result(&pod_id, false);
          tracker.record_result(&pod_id, false);

          tracker.reset(&pod_id);
          assert_eq!(tracker.state(&pod_id), Some(&HealthState::Healthy));
      }

      #[test]
      fn unregister_removes_all_tracking() {
          let mut tracker = HealthTracker::new();
          let pod_id = Uuid::new_v4();
          tracker.register(test_config(pod_id));

          tracker.unregister(&pod_id);
          assert!(tracker.state(&pod_id).is_none());
          assert!(tracker.config(&pod_id).is_none());
      }

      #[test]
      fn pods_due_for_probe_respects_interval() {
          let mut tracker = HealthTracker::new();
          let pod_id = Uuid::new_v4();

          let mut config = test_config(pod_id);
          config.interval = Duration::from_millis(50);
          tracker.register(config);

          // Just registered — last_probe is now, so not yet due
          assert!(tracker.pods_due_for_probe().is_empty());

          // Wait for interval to elapse
          std::thread::sleep(Duration::from_millis(60));
          let due = tracker.pods_due_for_probe();
          assert_eq!(due.len(), 1);
          assert_eq!(due[0], pod_id);
      }

      #[test]
      fn unhealthy_pods_are_not_probed() {
          let mut tracker = HealthTracker::new();
          let pod_id = Uuid::new_v4();

          let mut config = test_config(pod_id);
          config.interval = Duration::from_millis(1); // very short
          tracker.register(config);

          // Drive to Unhealthy
          tracker.record_result(&pod_id, false);
          tracker.record_result(&pod_id, false);
          tracker.record_result(&pod_id, false);

          std::thread::sleep(Duration::from_millis(10));
          assert!(tracker.pods_due_for_probe().is_empty());
      }

      #[test]
      fn record_result_for_unknown_pod_returns_none() {
          let mut tracker = HealthTracker::new();
          assert!(tracker.record_result(&Uuid::new_v4(), true).is_none());
      }
  }
  ```

  Add `pub mod domain;` to `crates/nexa-core/src/lib.rs`.

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo test -p nexa-core health`
  Expected: **PASSES** — all 11 tests green (implementation is inline above since the state machine logic is the deliverable).

- [ ] **Step 2: Commit**

  ```bash
  cd /Users/nassime/GitHub/NexaNet
  git add crates/nexa-core/src/domain/ crates/nexa-core/src/lib.rs
  git commit -m "feat(core): add HealthTracker with state machine for pod health"
  ```

---

### Task 4: HealthChecker Actor (Probes + Command Channel)

**Files:**
- Create: `crates/nexad/src/engine/health_checker.rs`
- Modify: `crates/nexad/src/engine/mod.rs`
- Modify: `crates/nexad/Cargo.toml`

- [ ] **Step 1: Add `reqwest` dependency to nexad**

  In `crates/nexad/Cargo.toml`, add under `[dependencies]`:

  ```toml
  reqwest = { workspace = true }
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo check -p nexad`
  Expected: **PASSES** — reqwest resolves from workspace.

- [ ] **Step 2: Define the Command enum for orchestrator communication**

  Before creating the health checker, we need a `Command` type. In `crates/nexad/src/engine/orchestrator.rs`, add above the `Orchestrator` struct:

  ```rust
  use tokio::sync::mpsc;

  #[derive(Debug)]
  pub enum Command {
      HealthReport { pod_id: Uuid, healthy: bool },
  }
  ```

  Add a `cmd_tx` field to `Orchestrator`:

  ```rust
  pub struct Orchestrator {
      runtime: Arc<dyn ContainerRuntime>,
      projects: DashMap<String, Project>,
      deployments: DashMap<Uuid, Arc<RwLock<Deployment>>>,
      pods: DashMap<Uuid, Arc<RwLock<Pod>>>,
      cmd_tx: mpsc::Sender<Command>,
      cmd_rx: RwLock<Option<mpsc::Receiver<Command>>>,
  }
  ```

  Update `Orchestrator::new()` to create the channel:

  ```rust
  pub async fn new() -> anyhow::Result<Arc<Self>> {
      let runtime = DockerRuntime::new()?;
      runtime.ping().await?;
      info!("connected to Docker runtime");

      let (cmd_tx, cmd_rx) = mpsc::channel(256);

      Ok(Arc::new(Self {
          runtime: Arc::new(runtime),
          projects: DashMap::new(),
          deployments: DashMap::new(),
          pods: DashMap::new(),
          cmd_tx,
          cmd_rx: RwLock::new(Some(cmd_rx)),
      }))
  }
  ```

  Add a getter for cloning the sender:

  ```rust
  /// Clone the command sender for use by subsystems (e.g. health checker).
  pub fn command_sender(&self) -> mpsc::Sender<Command> {
      self.cmd_tx.clone()
  }
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo check -p nexad`
  Expected: **PASSES**.

- [ ] **Step 3: Create the HealthChecker actor**

  Create `crates/nexad/src/engine/health_checker.rs`:

  ```rust
  use std::sync::Arc;
  use std::time::Duration;

  use nexa_core::domain::health::{HealthTracker, PodHealthConfig};
  use nexa_core::duration::parse_duration;
  use nexa_core::models::{DeploymentSpec, Pod, PodStatus};
  use reqwest::Client;
  use tokio::sync::{mpsc, Mutex};
  use tracing::{debug, error, info, warn};
  use uuid::Uuid;

  use super::orchestrator::Command;

  /// The HealthChecker runs as a background task, probing pods on their
  /// configured intervals and reporting results to the orchestrator.
  pub struct HealthChecker {
      tracker: Arc<Mutex<HealthTracker>>,
      cmd_tx: mpsc::Sender<Command>,
      http_client: Client,
  }

  impl HealthChecker {
      pub fn new(cmd_tx: mpsc::Sender<Command>) -> Self {
          Self {
              tracker: Arc::new(Mutex::new(HealthTracker::new())),
              cmd_tx,
              http_client: Client::builder()
                  .no_proxy()
                  .build()
                  .expect("failed to build reqwest client"),
          }
      }

      /// Get a handle to the tracker for external registration/unregistration.
      pub fn tracker(&self) -> Arc<Mutex<HealthTracker>> {
          self.tracker.clone()
      }

      /// Register a pod for health checking.  Call after a pod starts running.
      /// Returns `false` if the pod has no healthcheck config or missing IP.
      pub async fn register_pod(
          &self,
          pod: &Pod,
          spec: &DeploymentSpec,
      ) -> bool {
          let healthcheck = match &spec.healthcheck {
              Some(hc) => hc,
              None => return false,
          };

          let container_ip = match &pod.container_ip {
              Some(ip) => ip.clone(),
              None => {
                  warn!(pod_id = %pod.id, "cannot register pod for health check: no container IP");
                  return false;
              }
          };

          let port = match spec.ports.first() {
              Some(&p) => p,
              None => {
                  warn!(pod_id = %pod.id, "cannot register pod for health check: no ports");
                  return false;
              }
          };

          let interval = parse_duration(&healthcheck.interval).unwrap_or(Duration::from_secs(10));
          let timeout = parse_duration(&healthcheck.timeout).unwrap_or(Duration::from_secs(5));

          let config = PodHealthConfig {
              pod_id: pod.id,
              container_ip,
              port,
              path: healthcheck.path.clone(),
              interval,
              timeout,
              retries: healthcheck.retries,
          };

          info!(
              pod_id = %pod.id,
              path = %config.path,
              interval_secs = interval.as_secs(),
              retries = config.retries,
              "registered pod for health checking"
          );

          self.tracker.lock().await.register(config);
          true
      }

      /// Unregister a pod from health checking.
      pub async fn unregister_pod(&self, pod_id: &Uuid) {
          self.tracker.lock().await.unregister(pod_id);
          debug!(pod_id = %pod_id, "unregistered pod from health checking");
      }

      /// Run the health check loop.  This should be `tokio::spawn`ed.
      /// It wakes every 1 second, collects due pods, and spawns parallel probes.
      pub async fn run(self: Arc<Self>) {
          info!("health checker started");
          let mut tick = tokio::time::interval(Duration::from_secs(1));

          loop {
              tick.tick().await;

              let due_pods: Vec<(Uuid, PodHealthConfig)> = {
                  let mut tracker = self.tracker.lock().await;
                  let due_ids = tracker.pods_due_for_probe();
                  let mut results = Vec::with_capacity(due_ids.len());
                  for pod_id in due_ids {
                      if let Some(config) = tracker.config(&pod_id).cloned() {
                          tracker.mark_probed(&pod_id);
                          results.push((pod_id, config));
                      }
                  }
                  results
              };

              if due_pods.is_empty() {
                  continue;
              }

              debug!(count = due_pods.len(), "probing pods");

              for (pod_id, config) in due_pods {
                  let client = self.http_client.clone();
                  let cmd_tx = self.cmd_tx.clone();

                  tokio::spawn(async move {
                      let url = format!(
                          "http://{}:{}{}",
                          config.container_ip, config.port, config.path
                      );

                      let healthy = match tokio::time::timeout(
                          config.timeout,
                          client.get(&url).send(),
                      )
                      .await
                      {
                          Ok(Ok(response)) => response.status().is_success(),
                          Ok(Err(e)) => {
                              debug!(pod_id = %pod_id, url = %url, error = %e, "health probe failed");
                              false
                          }
                          Err(_) => {
                              debug!(pod_id = %pod_id, url = %url, "health probe timed out");
                              false
                          }
                      };

                      if let Err(e) = cmd_tx
                          .send(Command::HealthReport { pod_id, healthy })
                          .await
                      {
                          error!(error = %e, "failed to send health report to orchestrator");
                      }
                  });
              }
          }
      }
  }
  ```

  Update `crates/nexad/src/engine/mod.rs`:

  ```rust
  mod health_checker;
  mod orchestrator;

  pub use health_checker::HealthChecker;
  pub use orchestrator::{Command, Orchestrator};
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo check -p nexad`
  Expected: **PASSES**.

- [ ] **Step 4: Write a unit test for the HTTP probe logic**

  Add to the bottom of `crates/nexad/src/engine/health_checker.rs`:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use tokio::sync::mpsc;

      #[tokio::test]
      async fn probe_reports_unhealthy_for_unreachable_host() {
          let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
          let checker = Arc::new(HealthChecker::new(cmd_tx));

          let pod_id = Uuid::new_v4();
          let config = PodHealthConfig {
              pod_id,
              container_ip: "127.0.0.1".into(),
              port: 1, // nothing listens here
              path: "/health".into(),
              interval: Duration::from_millis(50),
              timeout: Duration::from_secs(1),
              retries: 3,
          };

          checker.tracker.lock().await.register(config);

          // Run the checker in background
          let checker_clone = checker.clone();
          let handle = tokio::spawn(async move {
              checker_clone.run().await;
          });

          // Wait for at least one report
          let report = tokio::time::timeout(Duration::from_secs(5), cmd_rx.recv())
              .await
              .expect("timed out waiting for health report")
              .expect("channel closed");

          match report {
              Command::HealthReport { pod_id: id, healthy } => {
                  assert_eq!(id, pod_id);
                  assert!(!healthy);
              }
          }

          handle.abort();
      }
  }
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo test -p nexad probe_reports`
  Expected: **PASSES** — the probe hits `127.0.0.1:1` which refuses connection, reports unhealthy.

- [ ] **Step 5: Commit**

  ```bash
  cd /Users/nassime/GitHub/NexaNet
  git add crates/nexad/src/engine/health_checker.rs crates/nexad/src/engine/mod.rs crates/nexad/src/engine/orchestrator.rs crates/nexad/Cargo.toml
  git commit -m "feat(nexad): add HealthChecker actor with HTTP probing and command reporting"
  ```

---

### Task 5: Handle HealthReport in Orchestrator Command Loop

**Files:**
- Modify: `crates/nexad/src/engine/orchestrator.rs`

- [ ] **Step 1: Write failing test — orchestrator processes health reports**

  Add test module to `crates/nexad/src/engine/orchestrator.rs`:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[tokio::test]
      async fn health_report_failure_tracks_state() {
          // We test the command processing logic in isolation by calling
          // handle_command directly.
          let (cmd_tx, cmd_rx) = mpsc::channel(16);
          let orch = Orchestrator::new_for_test(cmd_tx, cmd_rx);

          let pod_id = Uuid::new_v4();

          // Register a pod in the tracker
          {
              let mut tracker = orch.health_tracker.lock().await;
              tracker.register(nexa_core::domain::health::PodHealthConfig {
                  pod_id,
                  container_ip: "172.17.0.2".into(),
                  port: 3000,
                  path: "/health".into(),
                  interval: std::time::Duration::from_secs(10),
                  timeout: std::time::Duration::from_secs(5),
                  retries: 3,
              });
          }

          // Process a failure
          orch.handle_health_report(pod_id, false).await;
          let tracker = orch.health_tracker.lock().await;
          assert_eq!(
              tracker.state(&pod_id),
              Some(&nexa_core::domain::health::HealthState::Failing {
                  consecutive_failures: 1
              })
          );
      }
  }
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo test -p nexad health_report`
  Expected: **FAILS** — `health_tracker` field and `handle_health_report` method don't exist yet.

- [ ] **Step 2: Add health tracker to Orchestrator and implement health report handling**

  In `crates/nexad/src/engine/orchestrator.rs`, add the tracker field and imports:

  ```rust
  use std::sync::Arc;
  use std::time::Duration;

  use dashmap::DashMap;
  use nexa_core::domain::health::{HealthState, HealthTracker};
  use nexa_core::error::{NexaError, Result};
  use nexa_core::models::*;
  use nexa_core::runtime::*;
  use tokio::sync::{mpsc, Mutex, RwLock};
  use tracing::{error, info, warn};
  use uuid::Uuid;
  ```

  Update the struct:

  ```rust
  pub struct Orchestrator {
      runtime: Arc<dyn ContainerRuntime>,
      projects: DashMap<String, Project>,
      deployments: DashMap<Uuid, Arc<RwLock<Deployment>>>,
      pods: DashMap<Uuid, Arc<RwLock<Pod>>>,
      cmd_tx: mpsc::Sender<Command>,
      cmd_rx: RwLock<Option<mpsc::Receiver<Command>>>,
      pub health_tracker: Arc<Mutex<HealthTracker>>,
  }
  ```

  Update `Orchestrator::new()`:

  ```rust
  pub async fn new() -> anyhow::Result<Arc<Self>> {
      let runtime = DockerRuntime::new()?;
      runtime.ping().await?;
      info!("connected to Docker runtime");

      let (cmd_tx, cmd_rx) = mpsc::channel(256);

      Ok(Arc::new(Self {
          runtime: Arc::new(runtime),
          projects: DashMap::new(),
          deployments: DashMap::new(),
          pods: DashMap::new(),
          cmd_tx,
          cmd_rx: RwLock::new(Some(cmd_rx)),
          health_tracker: Arc::new(Mutex::new(HealthTracker::new())),
      }))
  }
  ```

  Add a test-only constructor (behind `#[cfg(test)]`):

  ```rust
  #[cfg(test)]
  impl Orchestrator {
      pub fn new_for_test(
          cmd_tx: mpsc::Sender<Command>,
          cmd_rx: mpsc::Receiver<Command>,
      ) -> Self {
          use nexa_core::runtime::DockerRuntime;
          Self {
              // For tests that don't touch Docker, we still need a runtime.
              // This will fail if Docker isn't available, which is fine for CI.
              runtime: Arc::new(DockerRuntime::new().expect("Docker required for tests")),
              projects: DashMap::new(),
              deployments: DashMap::new(),
              pods: DashMap::new(),
              cmd_tx,
              cmd_rx: RwLock::new(Some(cmd_rx)),
              health_tracker: Arc::new(Mutex::new(HealthTracker::new())),
          }
      }
  }
  ```

  Add the health report handler:

  ```rust
  /// Process a health report from the HealthChecker.
  pub async fn handle_health_report(&self, pod_id: Uuid, healthy: bool) {
      let result = {
          let mut tracker = self.health_tracker.lock().await;
          tracker.record_result(&pod_id, healthy)
      };

      match result {
          Some((HealthState::Unhealthy, true)) => {
              info!(pod_id = %pod_id, "pod unhealthy — triggering restart");
              if let Err(e) = self.restart_pod(pod_id).await {
                  error!(pod_id = %pod_id, error = %e, "failed to restart unhealthy pod");
              }
          }
          Some((HealthState::Failing { consecutive_failures }, _)) => {
              warn!(
                  pod_id = %pod_id,
                  failures = consecutive_failures,
                  "pod health check failing"
              );
          }
          Some((HealthState::Healthy, _)) => {
              debug_health(pod_id, "healthy");
          }
          _ => {}
      }
  }

  /// Restart a pod: stop + remove the old container, create a new one.
  async fn restart_pod(&self, pod_id: Uuid) -> Result<()> {
      let (deployment_id, container_id) = {
          let entry = self
              .pods
              .get(&pod_id)
              .ok_or_else(|| NexaError::PodNotFound(pod_id.to_string()))?;
          let pod = entry.value().read().await;
          (pod.deployment_id, pod.container_id.clone())
      };

      // Stop and remove the old container
      if let Some(cid) = &container_id {
          let _ = self.runtime.stop_container(cid, 10).await;
          let _ = self.runtime.remove_container(cid, true).await;
      }

      // Update pod status to Restarting
      if let Some(entry) = self.pods.get(&pod_id) {
          let mut pod = entry.value().write().await;
          pod.status = PodStatus::Restarting;
          pod.container_id = None;
          pod.container_ip = None;
      }

      // Get the deployment spec for re-creation
      let spec = {
          let entry = self
              .deployments
              .get(&deployment_id)
              .ok_or_else(|| NexaError::DeploymentNotFound(deployment_id.to_string()))?;
          let d = entry.value().read().await;
          d.spec.clone()
      };

      // Get the replica index
      let replica_index = {
          let entry = self
              .pods
              .get(&pod_id)
              .ok_or_else(|| NexaError::PodNotFound(pod_id.to_string()))?;
          entry.value().read().await.replica_index
      };

      // Remove old pod entry — create_pod will insert a new one
      self.pods.remove(&pod_id);

      // Create a fresh pod
      self.create_pod(deployment_id, &spec, replica_index).await?;

      // Reset health state for the old pod_id (cleanup) and the new pod
      // will be registered by the wiring layer
      {
          let mut tracker = self.health_tracker.lock().await;
          tracker.unregister(&pod_id);
      }

      info!(pod_id = %pod_id, "pod restarted");
      Ok(())
  }
  ```

  Add a small helper at the module level:

  ```rust
  fn debug_health(pod_id: Uuid, status: &str) {
      tracing::debug!(pod_id = %pod_id, status, "health check result");
  }
  ```

  Add the command processing loop method:

  ```rust
  /// Start the command processing loop.  Takes ownership of the receiver.
  /// Should be `tokio::spawn`ed.
  pub async fn run_command_loop(self: Arc<Self>) {
      let mut rx = {
          let mut guard = self.cmd_rx.write().await;
          match guard.take() {
              Some(rx) => rx,
              None => {
                  error!("command receiver already taken — cannot start command loop");
                  return;
              }
          }
      };

      info!("orchestrator command loop started");

      while let Some(cmd) = rx.recv().await {
          match cmd {
              Command::HealthReport { pod_id, healthy } => {
                  self.handle_health_report(pod_id, healthy).await;
              }
          }
      }

      warn!("orchestrator command loop exited — channel closed");
  }
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo test -p nexad health_report`
  Expected: **PASSES**.

- [ ] **Step 3: Commit**

  ```bash
  cd /Users/nassime/GitHub/NexaNet
  git add crates/nexad/src/engine/orchestrator.rs
  git commit -m "feat(nexad): handle HealthReport in orchestrator with restart logic"
  ```

---

### Task 6: Wire Health Checker into nexad Startup

**Files:**
- Modify: `crates/nexad/src/main.rs`
- Modify: `crates/nexad/src/engine/orchestrator.rs` (minor: expose `health_tracker` getter)

- [ ] **Step 1: Update nexad main.rs to spawn the health checker and command loop**

  Replace the contents of `crates/nexad/src/main.rs`:

  ```rust
  mod api;
  mod engine;

  use std::sync::Arc;

  use clap::Parser;
  use tracing::info;
  use tracing_subscriber::EnvFilter;

  use engine::{HealthChecker, Orchestrator};

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

      let orchestrator = Orchestrator::new().await?;

      // Create health checker with a sender into the orchestrator's command channel
      let health_checker = Arc::new(HealthChecker::new(orchestrator.command_sender()));

      // Spawn the orchestrator command processing loop
      let orch_clone = orchestrator.clone();
      tokio::spawn(async move {
          orch_clone.run_command_loop().await;
      });

      // Spawn the health checker background loop
      let checker_clone = health_checker.clone();
      tokio::spawn(async move {
          checker_clone.run().await;
      });

      info!("health checker started");

      let addr = format!("{}:{}", cli.host, cli.port);
      api::serve(orchestrator, &addr).await
  }
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo build`
  Expected: **PASSES** — full binary compiles.

- [ ] **Step 2: Register pods for health checking after creation**

  In `crates/nexad/src/engine/orchestrator.rs`, we need a way for the create_pod path to register with the health checker. Since the orchestrator doesn't own the HealthChecker directly, we store the tracker reference.

  The `health_tracker` is already on the Orchestrator. In `create_pod()`, after populating `container_ip`, add registration:

  ```rust
  // After: pod.container_ip = Some(ip);
  // Register for health checking if spec has a healthcheck
  if let (Some(hc), Some(ip)) = (&spec.healthcheck, &pod.container_ip) {
      if let Some(&port) = spec.ports.first() {
          let interval = nexa_core::duration::parse_duration(&hc.interval)
              .unwrap_or(std::time::Duration::from_secs(10));
          let timeout = nexa_core::duration::parse_duration(&hc.timeout)
              .unwrap_or(std::time::Duration::from_secs(5));

          let config = nexa_core::domain::health::PodHealthConfig {
              pod_id: pod.id,
              container_ip: ip.clone(),
              port,
              path: hc.path.clone(),
              interval,
              timeout,
              retries: hc.retries,
          };

          self.health_tracker.lock().await.register(config);
          info!(pod_id = %pod.id, "registered pod for health checking");
      }
  }
  ```

  In `stop_deployment()`, unregister pods from health tracking before stopping them. Inside the `for pod_id in &pod_ids` loop, before the container stop:

  ```rust
  // Unregister from health tracking
  self.health_tracker.lock().await.unregister(pod_id);
  ```

  In `remove_deployment()`, the stop is already handled by `stop_deployment`.

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo build`
  Expected: **PASSES**.

- [ ] **Step 3: Commit**

  ```bash
  cd /Users/nassime/GitHub/NexaNet
  git add crates/nexad/src/main.rs crates/nexad/src/engine/orchestrator.rs
  git commit -m "feat(nexad): wire health checker into daemon startup and pod lifecycle"
  ```

---

### Task 7: Docker Adapter — `container_ip()` Implementation Verification

**Files:**
- Modify: `crates/nexa-core/src/runtime/docker.rs` (already done in Task 1 Step 4)

This task is a verification-only task since `container_ip()` was implemented in Task 1.

- [ ] **Step 1: Write an integration test for container_ip (manual/Docker-required)**

  Add to `crates/nexa-core/src/runtime/docker.rs`:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      /// This test requires a running Docker daemon.
      /// Run with: cargo test -p nexa-core -- --ignored docker_container_ip
      #[tokio::test]
      #[ignore]
      async fn docker_container_ip_returns_valid_ip() {
          let runtime = DockerRuntime::new().unwrap();
          runtime.ping().await.unwrap();

          // Create a test network
          let network_name = "nexa-test-health";
          let _ = runtime.remove_network(network_name).await;
          runtime.create_network(network_name).await.unwrap();

          // Pull and create a container
          let _ = runtime.pull_image("nginx:alpine").await;

          let config = ContainerConfig {
              name: "nexa-health-test".into(),
              image: "nginx:alpine".into(),
              env: std::collections::HashMap::new(),
              ports: vec![PortBinding {
                  container_port: 80,
                  host_port: None,
              }],
              volumes: vec![],
              labels: std::collections::HashMap::new(),
              network: Some(network_name.to_string()),
          };

          // Clean up any previous test container
          let _ = runtime.stop_container(&config.name, 2).await;
          let _ = runtime.remove_container(&config.name, true).await;

          let container_id = runtime.create_container(&config).await.unwrap();
          runtime.start_container(&container_id).await.unwrap();

          // Get the container IP
          let ip = runtime
              .container_ip(&container_id, network_name)
              .await
              .unwrap();

          // Verify it looks like an IP address
          assert!(
              ip.contains('.'),
              "expected IPv4 address, got: {ip}"
          );
          assert!(
              ip.starts_with("172.") || ip.starts_with("10.") || ip.starts_with("192.168."),
              "expected private IP, got: {ip}"
          );

          // Cleanup
          let _ = runtime.stop_container(&container_id, 2).await;
          let _ = runtime.remove_container(&container_id, true).await;
          let _ = runtime.remove_network(network_name).await;
      }
  }
  ```

  Run: `cd /Users/nassime/GitHub/NexaNet && cargo test -p nexa-core -- --ignored docker_container_ip`
  Expected: **PASSES** (when Docker is available).

- [ ] **Step 2: Run full workspace build and test suite**

  ```bash
  cd /Users/nassime/GitHub/NexaNet && cargo build && cargo test
  ```

  Expected: **PASSES** — all unit tests green, binary compiles.

- [ ] **Step 3: Commit**

  ```bash
  cd /Users/nassime/GitHub/NexaNet
  git add crates/nexa-core/src/runtime/docker.rs
  git commit -m "test(docker): add integration test for container_ip lookup"
  ```

---

## Summary of All Changes

| File | Action | Purpose |
|------|--------|---------|
| `crates/nexa-core/src/models/pod.rs` | Modify | Add `container_ip: Option<String>` field |
| `crates/nexa-core/src/runtime/traits.rs` | Modify | Add `container_ip()` to `ContainerRuntime` trait |
| `crates/nexa-core/src/runtime/docker.rs` | Modify | Implement `container_ip()` via bollard inspect |
| `crates/nexa-core/src/duration.rs` | Create | `parse_duration("10s")` -> `Duration` utility |
| `crates/nexa-core/src/domain/mod.rs` | Create | Domain module declaration |
| `crates/nexa-core/src/domain/health.rs` | Create | `HealthTracker`, `HealthState`, `PodHealthConfig`, state machine |
| `crates/nexa-core/src/lib.rs` | Modify | Add `pub mod duration;` and `pub mod domain;` |
| `crates/nexad/Cargo.toml` | Modify | Add `reqwest` dependency |
| `crates/nexad/src/engine/health_checker.rs` | Create | `HealthChecker` actor with HTTP probing loop |
| `crates/nexad/src/engine/orchestrator.rs` | Modify | Add `Command` enum, command channel, health tracker, `handle_health_report()`, `restart_pod()`, `run_command_loop()` |
| `crates/nexad/src/engine/mod.rs` | Modify | Export `HealthChecker` and `Command` |
| `crates/nexad/src/main.rs` | Modify | Spawn health checker + command loop at startup |

## Test Coverage

| Test | File | What It Validates |
|------|------|-------------------|
| `pod_container_ip_defaults_to_none` | `pod.rs` | New field defaults correctly |
| `pod_serialization_roundtrip_with_ip` | `pod.rs` | Serialization with new field |
| `parse_seconds` | `duration.rs` | "10s" -> 10s |
| `parse_minutes` | `duration.rs` | "5m" -> 300s |
| `parse_hours` | `duration.rs` | "2h" -> 7200s |
| `parse_milliseconds` | `duration.rs` | "500ms" -> 500ms |
| `parse_bare_number_as_seconds` | `duration.rs` | "30" -> 30s |
| `reject_empty_string` | `duration.rs` | Error on "" |
| `reject_invalid_format` | `duration.rs` | Error on "abc", "10x", "s10" |
| `register_starts_healthy` | `health.rs` | Initial state |
| `success_keeps_healthy` | `health.rs` | Healthy -> success -> Healthy |
| `single_failure_transitions_to_failing` | `health.rs` | Healthy -> failure -> Failing(1) |
| `consecutive_failures_increment` | `health.rs` | Failing(1) -> failure -> Failing(2) |
| `reaching_retries_transitions_to_unhealthy` | `health.rs` | Failing(2) -> failure -> Unhealthy + restart |
| `success_resets_from_failing_to_healthy` | `health.rs` | Failing(2) -> success -> Healthy |
| `unhealthy_does_not_re_trigger_restart` | `health.rs` | Unhealthy -> failure -> no double-restart |
| `reset_restores_healthy` | `health.rs` | Manual reset after Unhealthy |
| `unregister_removes_all_tracking` | `health.rs` | Cleanup |
| `pods_due_for_probe_respects_interval` | `health.rs` | Interval scheduling |
| `unhealthy_pods_are_not_probed` | `health.rs` | Stop probing dead pods |
| `record_result_for_unknown_pod_returns_none` | `health.rs` | Graceful unknown pod |
| `probe_reports_unhealthy_for_unreachable_host` | `health_checker.rs` | End-to-end probe -> command |
| `health_report_failure_tracks_state` | `orchestrator.rs` | Command processing |
| `docker_container_ip_returns_valid_ip` | `docker.rs` | Integration (ignored, needs Docker) |
