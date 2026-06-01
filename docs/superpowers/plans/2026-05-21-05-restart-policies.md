# Restart Policies — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Multi-Repo Path Mapping:** This project uses separate repos. Translate paths as follows:
> | Plan path prefix | Repo | Local path |
> |---|---|---|
> | `crates/nexa-core/` | [`nexa-core`](https://github.com/nexa-net/nexa-core) | `/Users/nassime/GitHub/nexa-core/` |
> | `crates/nexad/` | [`nexad`](https://github.com/nexa-net/nexad) | `/Users/nassime/GitHub/nexad/` |
> | `crates/nexa-cli/` | [`nexa-cli`](https://github.com/nexa-net/nexa-cli) | `/Users/nassime/GitHub/nexa-cli/` |

>
> `cargo check -p <crate>` → `cargo check` in the target repo. `nexa-core` dep: `git = "https://github.com/nexa-net/nexa-core"`

**Goal:** Implement automatic pod restart with exponential backoff, crash-loop protection, and container event detection so pods self-heal on failure according to their deployment's restart policy.

**Architecture:** Two event sources feed the orchestrator actor: the existing health checker (sends `HealthReport` when a pod crosses the unhealthy threshold) and a new container event watcher (`tokio::spawn`ed task streaming Docker events, sending `ContainerExited` commands). The orchestrator handles both by consulting `should_restart()` + `RestartPolicy`, computing exponential backoff, and scheduling delayed `RestartPod` commands. Per-pod `RestartState` tracks restart count, timing, and crash-loop detection. After 10 consecutive restarts without 10 minutes of healthy runtime, the pod is permanently marked `CrashLoopBackoff`.

**Tech Stack:** tokio (spawn, sleep, mpsc), bollard 0.18 (system events), chrono, futures (StreamExt), uuid

---

### Task 1: Add RuntimeEvent, EventStream, and events() to ContainerRuntime trait

**Files:**
- Modify: `crates/nexa-core/src/ports/runtime.rs`

- [ ] **Step 1: Write a failing test that references the new types**

Add to `crates/nexa-core/src/ports/runtime.rs` at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_event_variants_exist() {
        let died = RuntimeEvent::ContainerDied {
            container_id: "abc123".into(),
            exit_code: 137,
        };
        let started = RuntimeEvent::ContainerStarted {
            container_id: "abc123".into(),
        };
        let oom = RuntimeEvent::ContainerOom {
            container_id: "abc123".into(),
        };

        // Verify Debug trait works
        let _ = format!("{died:?}");
        let _ = format!("{started:?}");
        let _ = format!("{oom:?}");
    }
}
```

Run: `cargo test -p nexa-core -- ports::runtime::tests 2>&1`
Expected: FAIL — `RuntimeEvent` does not exist

- [ ] **Step 2: Add RuntimeEvent enum and EventStream type alias**

Add to `crates/nexa-core/src/ports/runtime.rs`, after the `LogStream` type alias:

```rust
/// Stream of container runtime events (deaths, starts, OOMs).
pub type EventStream = Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>;

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    ContainerDied { container_id: String, exit_code: i64 },
    ContainerStarted { container_id: String },
    ContainerOom { container_id: String },
}
```

- [ ] **Step 3: Add events() method to ContainerRuntime trait**

Add to the `ContainerRuntime` trait body:

```rust
    /// Returns a stream of container lifecycle events.
    /// Implementations should filter to containers labeled `managed-by=nexanet`.
    async fn events(&self) -> Result<EventStream>;
```

Run: `cargo test -p nexa-core -- ports::runtime::tests 2>&1`
Expected: test passes (but the workspace will not compile until the Docker adapter implements `events()` — that is Task 7)

- [ ] **Step 4: Commit**

```bash
git add crates/nexa-core/src/ports/runtime.rs
git commit -m "feat: add RuntimeEvent, EventStream, and events() to ContainerRuntime trait"
```

---

### Task 2: Create domain/restart.rs with RestartState, should_restart(), and backoff logic

**Files:**
- Create: `crates/nexa-core/src/domain/restart.rs`
- Modify: `crates/nexa-core/src/domain/mod.rs`

- [ ] **Step 1: Write failing tests for should_restart() and backoff calculation**

Create `crates/nexa-core/src/domain/restart.rs`:

```rust
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::domain::models::RestartPolicy;

/// Maximum backoff delay: 5 minutes.
const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// Base delay for exponential backoff: 1 second.
const BASE_DELAY: Duration = Duration::from_secs(1);

/// If a pod stays healthy for this long, restart_count resets to 0.
const HEALTHY_RESET_WINDOW: Duration = Duration::from_secs(600); // 10 minutes

/// After this many consecutive restarts without a healthy window, mark CrashLoopBackoff.
const CRASH_LOOP_THRESHOLD: u32 = 10;

#[derive(Debug, Clone)]
pub struct RestartState {
    pub count: u32,
    pub last_restart: Option<DateTime<Utc>>,
    pub last_healthy_since: Option<DateTime<Utc>>,
}

impl Default for RestartState {
    fn default() -> Self {
        Self {
            count: 0,
            last_restart: None,
            last_healthy_since: None,
        }
    }
}

impl RestartState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if the pod has been healthy long enough to reset its restart counter.
    pub fn should_reset_count(&self, now: DateTime<Utc>) -> bool {
        if let Some(healthy_since) = self.last_healthy_since {
            let elapsed = now.signed_duration_since(healthy_since);
            elapsed >= chrono::Duration::from_std(HEALTHY_RESET_WINDOW).unwrap()
        } else {
            false
        }
    }

    /// Reset restart count after sustained healthy period.
    pub fn reset_if_healthy(&mut self, now: DateTime<Utc>) {
        if self.should_reset_count(now) {
            self.count = 0;
            self.last_restart = None;
        }
    }

    /// Record that the pod became healthy at the given time.
    pub fn mark_healthy(&mut self, now: DateTime<Utc>) {
        if self.last_healthy_since.is_none() {
            self.last_healthy_since = Some(now);
        }
    }

    /// Clear healthy timestamp when the pod is no longer healthy.
    pub fn mark_unhealthy(&mut self) {
        self.last_healthy_since = None;
    }

    /// Returns true if the pod has exceeded the crash loop threshold.
    pub fn is_crash_loop(&self) -> bool {
        self.count >= CRASH_LOOP_THRESHOLD
    }

    /// Increment restart count and record restart time. Returns the new count.
    pub fn record_restart(&mut self, now: DateTime<Utc>) -> u32 {
        self.count += 1;
        self.last_restart = Some(now);
        self.last_healthy_since = None;
        self.count
    }
}

/// Determine whether a pod should be restarted given its policy and exit code.
pub fn should_restart(policy: &RestartPolicy, exit_code: i64) -> bool {
    match policy {
        RestartPolicy::Never => false,
        RestartPolicy::Always => true,
        RestartPolicy::OnFailure => exit_code != 0,
    }
}

/// Calculate exponential backoff delay: min(1s * 2^restart_count, 5m).
pub fn backoff_delay(restart_count: u32) -> Duration {
    let delay = BASE_DELAY.saturating_mul(2u32.saturating_pow(restart_count));
    delay.min(MAX_BACKOFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- should_restart tests ---

    #[test]
    fn never_policy_never_restarts() {
        assert!(!should_restart(&RestartPolicy::Never, 0));
        assert!(!should_restart(&RestartPolicy::Never, 1));
        assert!(!should_restart(&RestartPolicy::Never, 137));
    }

    #[test]
    fn always_policy_always_restarts() {
        assert!(should_restart(&RestartPolicy::Always, 0));
        assert!(should_restart(&RestartPolicy::Always, 1));
        assert!(should_restart(&RestartPolicy::Always, 137));
    }

    #[test]
    fn on_failure_restarts_only_on_nonzero() {
        assert!(!should_restart(&RestartPolicy::OnFailure, 0));
        assert!(should_restart(&RestartPolicy::OnFailure, 1));
        assert!(should_restart(&RestartPolicy::OnFailure, 137));
        assert!(should_restart(&RestartPolicy::OnFailure, -1));
    }

    // --- backoff_delay tests ---

    #[test]
    fn backoff_starts_at_one_second() {
        assert_eq!(backoff_delay(0), Duration::from_secs(1));
    }

    #[test]
    fn backoff_doubles_each_restart() {
        assert_eq!(backoff_delay(0), Duration::from_secs(1));
        assert_eq!(backoff_delay(1), Duration::from_secs(2));
        assert_eq!(backoff_delay(2), Duration::from_secs(4));
        assert_eq!(backoff_delay(3), Duration::from_secs(8));
        assert_eq!(backoff_delay(4), Duration::from_secs(16));
        assert_eq!(backoff_delay(5), Duration::from_secs(32));
        assert_eq!(backoff_delay(6), Duration::from_secs(64));
        assert_eq!(backoff_delay(7), Duration::from_secs(128));
        assert_eq!(backoff_delay(8), Duration::from_secs(256));
    }

    #[test]
    fn backoff_caps_at_five_minutes() {
        assert_eq!(backoff_delay(9), Duration::from_secs(300)); // 512 capped to 300
        assert_eq!(backoff_delay(10), Duration::from_secs(300));
        assert_eq!(backoff_delay(20), Duration::from_secs(300));
        assert_eq!(backoff_delay(100), Duration::from_secs(300));
    }

    // --- RestartState tests ---

    #[test]
    fn new_state_has_zero_count() {
        let state = RestartState::new();
        assert_eq!(state.count, 0);
        assert!(state.last_restart.is_none());
        assert!(state.last_healthy_since.is_none());
    }

    #[test]
    fn record_restart_increments_count() {
        let mut state = RestartState::new();
        let now = Utc::now();
        assert_eq!(state.record_restart(now), 1);
        assert_eq!(state.record_restart(now), 2);
        assert_eq!(state.record_restart(now), 3);
        assert_eq!(state.count, 3);
        assert_eq!(state.last_restart, Some(now));
    }

    #[test]
    fn record_restart_clears_healthy_since() {
        let mut state = RestartState::new();
        let now = Utc::now();
        state.mark_healthy(now);
        assert!(state.last_healthy_since.is_some());
        state.record_restart(now);
        assert!(state.last_healthy_since.is_none());
    }

    #[test]
    fn crash_loop_detected_after_threshold() {
        let mut state = RestartState::new();
        let now = Utc::now();
        for _ in 0..9 {
            state.record_restart(now);
            assert!(!state.is_crash_loop());
        }
        state.record_restart(now); // 10th restart
        assert!(state.is_crash_loop());
    }

    #[test]
    fn healthy_reset_clears_count_after_ten_minutes() {
        let mut state = RestartState::new();
        let start = Utc::now();
        state.record_restart(start);
        state.record_restart(start);
        state.record_restart(start);
        assert_eq!(state.count, 3);

        state.mark_healthy(start);

        // 9 minutes later: should NOT reset
        let nine_min = start + chrono::Duration::minutes(9);
        state.reset_if_healthy(nine_min);
        assert_eq!(state.count, 3);

        // 10 minutes later: should reset
        let ten_min = start + chrono::Duration::minutes(10);
        state.reset_if_healthy(ten_min);
        assert_eq!(state.count, 0);
        assert!(state.last_restart.is_none());
    }

    #[test]
    fn mark_healthy_only_sets_once() {
        let mut state = RestartState::new();
        let t1 = Utc::now();
        let t2 = t1 + chrono::Duration::seconds(30);

        state.mark_healthy(t1);
        state.mark_healthy(t2); // should NOT overwrite

        assert_eq!(state.last_healthy_since, Some(t1));
    }

    #[test]
    fn mark_unhealthy_clears_healthy_since() {
        let mut state = RestartState::new();
        state.mark_healthy(Utc::now());
        assert!(state.last_healthy_since.is_some());
        state.mark_unhealthy();
        assert!(state.last_healthy_since.is_none());
    }
}
```

- [ ] **Step 2: Register the module in domain/mod.rs**

Add to `crates/nexa-core/src/domain/mod.rs`:

```rust
pub mod restart;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p nexa-core -- domain::restart 2>&1`
Expected: all 11 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/nexa-core/src/domain/restart.rs crates/nexa-core/src/domain/mod.rs
git commit -m "feat: add restart domain logic with should_restart, backoff, and RestartState"
```

---

### Task 3: Add CrashLoopBackoff to PodStatus, add ContainerExited + RestartPod to Command enum

**Files:**
- Modify: `crates/nexa-core/src/domain/models/pod.rs`
- Modify: `crates/nexa-core/src/domain/orchestrator.rs`

- [ ] **Step 1: Write failing test that uses CrashLoopBackoff variant**

Add to the bottom of `crates/nexa-core/src/domain/models/pod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_loop_backoff_status_exists_and_displays() {
        let status = PodStatus::CrashLoopBackoff;
        assert_eq!(format!("{status}"), "CrashLoopBackoff");
    }

    #[test]
    fn pod_has_restart_count_field() {
        let mut pod = Pod::new(
            Uuid::new_v4(),
            "proj",
            "deploy",
            0,
            "nginx:latest",
        );
        assert_eq!(pod.restart_count, 0);
        pod.restart_count = 5;
        assert_eq!(pod.restart_count, 5);
    }
}
```

Run: `cargo test -p nexa-core -- domain::models::pod::tests 2>&1`
Expected: FAIL — `CrashLoopBackoff` variant and `restart_count` field do not exist

- [ ] **Step 2: Add CrashLoopBackoff to PodStatus and restart_count to Pod**

In `crates/nexa-core/src/domain/models/pod.rs`, add the variant to the `PodStatus` enum:

```rust
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
    CrashLoopBackoff,
}
```

Add `restart_count` field to the `Pod` struct:

```rust
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
    pub restart_count: u32,
    pub created_at: DateTime<Utc>,
}
```

Update `Pod::new()` to initialize `restart_count: 0`.

Add the Display arm:

```rust
PodStatus::CrashLoopBackoff => write!(f, "CrashLoopBackoff"),
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test -p nexa-core -- domain::models::pod::tests 2>&1`
Expected: 2 tests pass

- [ ] **Step 4: Add ContainerExited and RestartPod to the Command enum**

In `crates/nexa-core/src/domain/orchestrator.rs`, add two new variants to `Command`:

```rust
pub enum Command {
    // ... existing variants ...

    /// Sent by the container event watcher when a container exits.
    ContainerExited {
        pod_id: Uuid,
        exit_code: i64,
    },

    /// Scheduled by the orchestrator after backoff delay to perform actual restart.
    RestartPod {
        pod_id: Uuid,
    },

    /// Sent by the health checker when a pod's health state changes.
    HealthReport {
        pod_id: Uuid,
        healthy: bool,
    },
}
```

Add placeholder match arms in the `run()` loop so it compiles:

```rust
Command::ContainerExited { pod_id, exit_code } => {
    // Implemented in Task 5
    let _ = (pod_id, exit_code);
}
Command::RestartPod { pod_id } => {
    // Implemented in Task 6
    let _ = pod_id;
}
Command::HealthReport { pod_id, healthy } => {
    // Already exists from Plan #4; ensure restart integration in Task 5
    let _ = (pod_id, healthy);
}
```

- [ ] **Step 5: Verify workspace compiles and all tests pass**

Run: `cargo test -p nexa-core 2>&1`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/nexa-core/src/domain/models/pod.rs crates/nexa-core/src/domain/orchestrator.rs
git commit -m "feat: add CrashLoopBackoff status, restart_count, ContainerExited and RestartPod commands"
```

---

### Task 4: Implement container event watcher

**Files:**
- Create: `crates/nexad/src/adapters/event_watcher.rs`
- Modify: `crates/nexad/src/adapters/mod.rs`

- [ ] **Step 1: Write a test for the event-to-command mapping logic**

Create `crates/nexad/src/adapters/event_watcher.rs`:

```rust
use std::collections::HashMap;
use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

use nexa_core::domain::orchestrator::Command;
use nexa_core::ports::runtime::{ContainerRuntime, RuntimeEvent};

/// Spawns a background task that listens to container runtime events and
/// forwards relevant ones as Commands to the orchestrator.
pub fn spawn_event_watcher(
    runtime: Arc<dyn ContainerRuntime>,
    tx: mpsc::Sender<Command>,
) {
    tokio::spawn(async move {
        info!("container event watcher starting");
        loop {
            match runtime.events().await {
                Ok(stream) => {
                    handle_event_stream(stream, &tx).await;
                    warn!("event stream ended, reconnecting in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    error!(error = %e, "failed to open event stream, retrying in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    });
}

async fn handle_event_stream(
    mut stream: nexa_core::ports::runtime::EventStream,
    tx: &mpsc::Sender<Command>,
) {
    while let Some(event) = stream.next().await {
        match event {
            RuntimeEvent::ContainerDied { container_id, exit_code } => {
                info!(container_id, exit_code, "container died event");
                if let Some(pod_id) = extract_pod_id_from_container(&container_id) {
                    let cmd = Command::ContainerExited { pod_id, exit_code };
                    if tx.send(cmd).await.is_err() {
                        error!("orchestrator channel closed, stopping event watcher");
                        return;
                    }
                }
            }
            RuntimeEvent::ContainerOom { container_id } => {
                warn!(container_id, "container OOM event");
                if let Some(pod_id) = extract_pod_id_from_container(&container_id) {
                    // Treat OOM as exit code 137 (SIGKILL)
                    let cmd = Command::ContainerExited { pod_id, exit_code: 137 };
                    if tx.send(cmd).await.is_err() {
                        error!("orchestrator channel closed, stopping event watcher");
                        return;
                    }
                }
            }
            RuntimeEvent::ContainerStarted { container_id } => {
                info!(container_id, "container started event");
                // No action needed — the orchestrator already knows about starts it initiated
            }
        }
    }
}

/// Extract the pod ID from a container ID.
///
/// The Docker adapter's event stream is already filtered to `managed-by=nexanet`
/// containers and includes the `nexa.pod-id` label in the RuntimeEvent's container_id
/// field (we use the label value, not the Docker container hash).
///
/// However, if the event stream only provides the Docker container ID, we need
/// the orchestrator to resolve it. For now, the Docker adapter maps events
/// using labels and passes the pod UUID as the container_id.
fn extract_pod_id_from_container(container_id: &str) -> Option<Uuid> {
    Uuid::parse_str(container_id).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexa_core::ports::runtime::RuntimeEvent;

    #[test]
    fn extract_pod_id_parses_valid_uuid() {
        let id = Uuid::new_v4();
        assert_eq!(extract_pod_id_from_container(&id.to_string()), Some(id));
    }

    #[test]
    fn extract_pod_id_returns_none_for_non_uuid() {
        assert_eq!(extract_pod_id_from_container("abc123def"), None);
    }

    #[tokio::test]
    async fn event_watcher_forwards_container_died() {
        let pod_id = Uuid::new_v4();

        let events: Vec<RuntimeEvent> = vec![
            RuntimeEvent::ContainerDied {
                container_id: pod_id.to_string(),
                exit_code: 1,
            },
        ];

        let (tx, mut rx) = mpsc::channel(16);
        let stream: nexa_core::ports::runtime::EventStream =
            Box::pin(futures::stream::iter(events));

        handle_event_stream(stream, &tx).await;

        let cmd = rx.try_recv().expect("should have received a command");
        match cmd {
            Command::ContainerExited { pod_id: pid, exit_code } => {
                assert_eq!(pid, pod_id);
                assert_eq!(exit_code, 1);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn event_watcher_forwards_oom_as_exit_137() {
        let pod_id = Uuid::new_v4();

        let events: Vec<RuntimeEvent> = vec![
            RuntimeEvent::ContainerOom {
                container_id: pod_id.to_string(),
            },
        ];

        let (tx, mut rx) = mpsc::channel(16);
        let stream: nexa_core::ports::runtime::EventStream =
            Box::pin(futures::stream::iter(events));

        handle_event_stream(stream, &tx).await;

        let cmd = rx.try_recv().expect("should have received a command");
        match cmd {
            Command::ContainerExited { pod_id: pid, exit_code } => {
                assert_eq!(pid, pod_id);
                assert_eq!(exit_code, 137);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn event_watcher_ignores_started_events() {
        let events: Vec<RuntimeEvent> = vec![
            RuntimeEvent::ContainerStarted {
                container_id: Uuid::new_v4().to_string(),
            },
        ];

        let (tx, mut rx) = mpsc::channel(16);
        let stream: nexa_core::ports::runtime::EventStream =
            Box::pin(futures::stream::iter(events));

        handle_event_stream(stream, &tx).await;

        assert!(rx.try_recv().is_err(), "should not forward started events");
    }
}
```

- [ ] **Step 2: Register the module**

Add to `crates/nexad/src/adapters/mod.rs`:

```rust
pub mod event_watcher;
pub mod runtime;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p nexad -- adapters::event_watcher 2>&1`
Expected: all 5 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/nexad/src/adapters/event_watcher.rs crates/nexad/src/adapters/mod.rs
git commit -m "feat: add container event watcher that translates runtime events to orchestrator commands"
```

---

### Task 5: Handle ContainerExited in the orchestrator loop

**Files:**
- Modify: `crates/nexa-core/src/domain/orchestrator.rs`

- [ ] **Step 1: Write failing tests for restart decision flow**

Add to the `tests` module in `crates/nexa-core/src/domain/orchestrator.rs`:

```rust
#[tokio::test]
async fn container_exited_with_always_policy_triggers_restart() {
    let handle = spawn_test_orchestrator();

    let spec = DeploymentSpec {
        project: "test".into(),
        deployment: DeploymentMeta { name: "web".into() },
        replicas: 1,
        image: "nginx:latest".into(),
        ports: vec![],
        env: HashMap::new(),
        volumes: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::Always,
    };

    let deployment = handle.deploy(spec).await.unwrap();
    let pods = handle.list_pods(Some("test".into())).await;
    assert_eq!(pods.len(), 1);
    let pod_id = pods[0].id;

    // Simulate container exit
    handle.send_container_exited(pod_id, 0).await;

    // Give the orchestrator time to process + the 1s backoff for restart_count=0
    tokio::time::sleep(Duration::from_secs(3)).await;

    let pods = handle.list_pods(Some("test".into())).await;
    assert_eq!(pods.len(), 1);
    assert_eq!(pods[0].status, PodStatus::Running);
    assert_eq!(pods[0].restart_count, 1);
}

#[tokio::test]
async fn container_exited_with_never_policy_marks_failed() {
    let handle = spawn_test_orchestrator();

    let spec = DeploymentSpec {
        project: "test".into(),
        deployment: DeploymentMeta { name: "web".into() },
        replicas: 1,
        image: "nginx:latest".into(),
        ports: vec![],
        env: HashMap::new(),
        volumes: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::Never,
    };

    handle.deploy(spec).await.unwrap();
    let pods = handle.list_pods(Some("test".into())).await;
    let pod_id = pods[0].id;

    handle.send_container_exited(pod_id, 1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let pods = handle.list_pods(Some("test".into())).await;
    assert_eq!(pods[0].status, PodStatus::Failed);
}

#[tokio::test]
async fn on_failure_policy_does_not_restart_on_clean_exit() {
    let handle = spawn_test_orchestrator();

    let spec = DeploymentSpec {
        project: "test".into(),
        deployment: DeploymentMeta { name: "web".into() },
        replicas: 1,
        image: "nginx:latest".into(),
        ports: vec![],
        env: HashMap::new(),
        volumes: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::OnFailure,
    };

    handle.deploy(spec).await.unwrap();
    let pods = handle.list_pods(Some("test".into())).await;
    let pod_id = pods[0].id;

    handle.send_container_exited(pod_id, 0).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let pods = handle.list_pods(Some("test".into())).await;
    // exit_code 0 + OnFailure = no restart → mark Failed (graceful exit)
    assert_eq!(pods[0].status, PodStatus::Failed);
}

#[tokio::test]
async fn crash_loop_backoff_after_ten_restarts() {
    let handle = spawn_test_orchestrator();

    let spec = DeploymentSpec {
        project: "test".into(),
        deployment: DeploymentMeta { name: "web".into() },
        replicas: 1,
        image: "nginx:latest".into(),
        ports: vec![],
        env: HashMap::new(),
        volumes: vec![],
        network: None,
        healthcheck: None,
        restart: RestartPolicy::Always,
    };

    handle.deploy(spec).await.unwrap();
    let pods = handle.list_pods(Some("test".into())).await;
    let pod_id = pods[0].id;

    // Simulate 10 consecutive quick failures
    for i in 0..10 {
        handle.send_container_exited(pod_id, 1).await;
        // Wait enough for processing + short backoff (tests should use
        // a test-friendly backoff or the real backoff accumulates)
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // After 10 restarts, the pod should be in CrashLoopBackoff
    tokio::time::sleep(Duration::from_millis(200)).await;
    let pods = handle.list_pods(Some("test".into())).await;
    assert_eq!(pods[0].status, PodStatus::CrashLoopBackoff);
}
```

Add a helper method to `OrchestratorHandle`:

```rust
impl OrchestratorHandle {
    // ... existing methods ...

    /// Send a ContainerExited command (used by event watcher and tests).
    pub async fn send_container_exited(&self, pod_id: Uuid, exit_code: i64) {
        let _ = self.tx.send(Command::ContainerExited { pod_id, exit_code }).await;
    }

    /// Get a clone of the command sender (used by event watcher to send events).
    pub fn command_sender(&self) -> mpsc::Sender<Command> {
        self.tx.clone()
    }
}
```

Run: `cargo test -p nexa-core -- container_exited 2>&1`
Expected: FAIL — placeholder match arms do nothing

- [ ] **Step 2: Add RestartState tracking to Orchestrator**

In `crates/nexa-core/src/domain/orchestrator.rs`, add the import and field:

```rust
use std::collections::HashMap as StdHashMap;

use crate::domain::restart::{self, RestartState};

pub struct Orchestrator {
    runtime: Arc<dyn ContainerRuntime>,
    projects: StdHashMap<String, Project>,
    deployments: StdHashMap<Uuid, Deployment>,
    pods: StdHashMap<Uuid, Pod>,
    restart_states: StdHashMap<Uuid, RestartState>,
    tx: mpsc::Sender<Command>,
}
```

Update `Orchestrator::spawn()` to store a `tx` clone:

```rust
pub fn spawn(runtime: Arc<dyn ContainerRuntime>) -> OrchestratorHandle {
    let (tx, rx) = mpsc::channel(256);
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut orch = Self {
            runtime,
            projects: StdHashMap::new(),
            deployments: StdHashMap::new(),
            pods: StdHashMap::new(),
            restart_states: StdHashMap::new(),
            tx: tx_clone,
        };
        orch.run(rx).await;
    });
    OrchestratorHandle { tx }
}
```

- [ ] **Step 3: Implement handle_container_exited**

Replace the placeholder `Command::ContainerExited` arm:

```rust
Command::ContainerExited { pod_id, exit_code } => {
    self.handle_container_exited(pod_id, exit_code).await;
}
```

Add the method:

```rust
async fn handle_container_exited(&mut self, pod_id: Uuid, exit_code: i64) {
    // Look up the pod
    let pod = match self.pods.get(&pod_id) {
        Some(p) => p,
        None => {
            warn!(pod_id = %pod_id, "ContainerExited for unknown pod");
            return;
        }
    };

    // Find the deployment's restart policy
    let policy = match self.deployments.get(&pod.deployment_id) {
        Some(d) => d.spec.restart.clone(),
        None => {
            warn!(pod_id = %pod_id, "deployment not found for pod");
            return;
        }
    };

    // Check restart policy
    if !restart::should_restart(&policy, exit_code) {
        info!(pod_id = %pod_id, exit_code, "pod will not be restarted (policy: {policy:?})");
        if let Some(p) = self.pods.get_mut(&pod_id) {
            p.status = PodStatus::Failed;
        }
        return;
    }

    // Get or create restart state
    let state = self.restart_states.entry(pod_id).or_insert_with(RestartState::new);

    // Check crash loop
    if state.is_crash_loop() {
        warn!(pod_id = %pod_id, count = state.count, "crash loop detected");
        if let Some(p) = self.pods.get_mut(&pod_id) {
            p.status = PodStatus::CrashLoopBackoff;
        }
        return;
    }

    // Record this restart
    let now = Utc::now();
    let count = state.record_restart(now);

    // Check crash loop again after incrementing
    if state.is_crash_loop() {
        warn!(pod_id = %pod_id, count, "crash loop threshold reached");
        if let Some(p) = self.pods.get_mut(&pod_id) {
            p.status = PodStatus::CrashLoopBackoff;
            p.restart_count = count;
        }
        return;
    }

    // Update pod
    if let Some(p) = self.pods.get_mut(&pod_id) {
        p.status = PodStatus::Restarting;
        p.restart_count = count;
    }

    // Calculate backoff and schedule delayed restart
    let delay = restart::backoff_delay(count.saturating_sub(1));
    let tx = self.tx.clone();

    info!(
        pod_id = %pod_id,
        restart_count = count,
        delay_secs = delay.as_secs(),
        "scheduling pod restart"
    );

    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let _ = tx.send(Command::RestartPod { pod_id }).await;
    });
}
```

- [ ] **Step 4: Run tests to verify restart decision logic**

Run: `cargo test -p nexa-core -- container_exited 2>&1`
Expected: `container_exited_with_never_policy_marks_failed` and `on_failure_policy_does_not_restart_on_clean_exit` pass; `container_exited_with_always_policy_triggers_restart` still fails because `RestartPod` handler is not yet implemented (Task 6)

Run: `cargo test -p nexa-core -- crash_loop_backoff 2>&1`
Expected: passes (the crash loop test only checks status, not actual restart)

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-core/src/domain/orchestrator.rs
git commit -m "feat: handle ContainerExited with restart policy evaluation and crash-loop detection"
```

---

### Task 6: Handle RestartPod in the orchestrator loop

**Files:**
- Modify: `crates/nexa-core/src/domain/orchestrator.rs`

- [ ] **Step 1: Replace the RestartPod placeholder with real handler**

Replace the placeholder `Command::RestartPod` arm:

```rust
Command::RestartPod { pod_id } => {
    self.handle_restart_pod(pod_id).await;
}
```

Add the method:

```rust
async fn handle_restart_pod(&mut self, pod_id: Uuid) {
    // Look up the pod
    let (deployment_id, old_container_id) = match self.pods.get(&pod_id) {
        Some(p) => {
            if p.status == PodStatus::CrashLoopBackoff || p.status == PodStatus::Failed {
                info!(pod_id = %pod_id, "skipping restart for terminal pod");
                return;
            }
            (p.deployment_id, p.container_id.clone())
        }
        None => {
            warn!(pod_id = %pod_id, "RestartPod for unknown pod");
            return;
        }
    };

    // Remove old container
    if let Some(cid) = &old_container_id {
        info!(pod_id = %pod_id, container_id = cid, "removing old container");
        let _ = self.runtime.stop_container(cid, 5).await;
        let _ = self.runtime.remove_container(cid, true).await;
    }

    // Get deployment spec for creating a new container
    let spec = match self.deployments.get(&deployment_id) {
        Some(d) => d.spec.clone(),
        None => {
            warn!(pod_id = %pod_id, "deployment gone, marking pod failed");
            if let Some(p) = self.pods.get_mut(&pod_id) {
                p.status = PodStatus::Failed;
            }
            return;
        }
    };

    // Rebuild the container
    let pod = match self.pods.get(&pod_id) {
        Some(p) => p.clone(),
        None => return,
    };

    let container_name = pod.container_name();
    let network_name = format!("nexa-{}", spec.project);

    let _ = self.runtime.pull_image(&spec.image).await;

    if self.runtime.container_exists(&container_name).await.unwrap_or(false) {
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
    labels.insert("nexa.pod-id".to_string(), pod_id.to_string());

    let config = ContainerConfig {
        name: container_name.clone(),
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
            if let Err(e) = self.runtime.start_container(&container_id).await {
                error!(pod_id = %pod_id, error = %e, "failed to start restarted container");
                if let Some(p) = self.pods.get_mut(&pod_id) {
                    p.status = PodStatus::Failed;
                }
                return;
            }

            info!(pod_id = %pod_id, container_name, "pod restarted successfully");
            if let Some(p) = self.pods.get_mut(&pod_id) {
                p.container_id = Some(container_id);
                p.status = PodStatus::Running;
            }

            // Mark healthy start time for reset tracking
            if let Some(state) = self.restart_states.get_mut(&pod_id) {
                state.mark_healthy(Utc::now());
            }
        }
        Err(e) => {
            error!(pod_id = %pod_id, error = %e, "failed to create restarted container");
            if let Some(p) = self.pods.get_mut(&pod_id) {
                p.status = PodStatus::Failed;
            }
        }
    }
}
```

- [ ] **Step 2: Integrate HealthReport with restart state tracking**

Update the `Command::HealthReport` arm:

```rust
Command::HealthReport { pod_id, healthy } => {
    self.handle_health_report(pod_id, healthy).await;
}
```

Add the method:

```rust
async fn handle_health_report(&mut self, pod_id: Uuid, healthy: bool) {
    if healthy {
        // Record healthy timestamp; reset restart count after 10min window
        let now = Utc::now();
        let state = self.restart_states.entry(pod_id).or_insert_with(RestartState::new);
        state.mark_healthy(now);
        state.reset_if_healthy(now);

        // Sync restart_count to pod
        if let Some(p) = self.pods.get_mut(&pod_id) {
            p.restart_count = state.count;
        }
    } else {
        // Mark unhealthy — the health checker's threshold logic
        // will eventually trigger HealthState::Unhealthy which sends
        // a HealthReport(healthy=false). Treat as container exit with code 1.
        if let Some(state) = self.restart_states.get_mut(&pod_id) {
            state.mark_unhealthy();
        }

        // Delegate to the same restart path as ContainerExited
        self.handle_container_exited(pod_id, 1).await;
    }
}
```

- [ ] **Step 3: Run all restart-related tests**

Run: `cargo test -p nexa-core -- domain::orchestrator 2>&1`
Expected: all tests pass, including the restart tests from Task 5

- [ ] **Step 4: Commit**

```bash
git add crates/nexa-core/src/domain/orchestrator.rs
git commit -m "feat: implement RestartPod handler with container recreation and health report integration"
```

---

### Task 7: Implement Docker events() in the Docker adapter

**Files:**
- Modify: `crates/nexad/src/adapters/runtime/docker.rs`

- [ ] **Step 1: Add events() implementation to DockerRuntime**

Add the following import at the top of `docker.rs`:

```rust
use bollard::system::EventsOptions;
use bollard::models::EventMessageTypeEnum;
```

Add the `events()` method to the `impl ContainerRuntime for DockerRuntime` block:

```rust
async fn events(&self) -> Result<EventStream> {
    use std::collections::HashMap;

    let mut filters = HashMap::new();
    filters.insert("type".to_string(), vec!["container".to_string()]);
    filters.insert(
        "event".to_string(),
        vec!["die".to_string(), "start".to_string(), "oom".to_string()],
    );
    filters.insert(
        "label".to_string(),
        vec!["managed-by=nexanet".to_string()],
    );

    let options = EventsOptions::<String> {
        since: None,
        until: None,
        filters,
    };

    let stream = self.client.events(Some(options));

    let mapped = stream.filter_map(|result| async move {
        match result {
            Ok(event) => {
                let action = event.action.as_deref().unwrap_or("");
                let actor = event.actor.as_ref();
                let container_id = actor
                    .and_then(|a| a.attributes.as_ref())
                    .and_then(|attrs| attrs.get("nexa.pod-id"))
                    .cloned()
                    .unwrap_or_default();

                match action {
                    "die" => {
                        let exit_code = actor
                            .and_then(|a| a.attributes.as_ref())
                            .and_then(|attrs| attrs.get("exitCode"))
                            .and_then(|code| code.parse::<i64>().ok())
                            .unwrap_or(-1);

                        Some(RuntimeEvent::ContainerDied {
                            container_id,
                            exit_code,
                        })
                    }
                    "start" => Some(RuntimeEvent::ContainerStarted { container_id }),
                    "oom" => Some(RuntimeEvent::ContainerOom { container_id }),
                    _ => None,
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "error in Docker event stream");
                None
            }
        }
    });

    Ok(Box::pin(mapped))
}
```

- [ ] **Step 2: Update imports at the top of docker.rs**

Ensure these are present:

```rust
use nexa_core::ports::runtime::{EventStream, RuntimeEvent};
```

(These should already be available via `use nexa_core::ports::runtime::*;`)

- [ ] **Step 3: Verify the full workspace compiles**

Run: `cargo check 2>&1`
Expected: compiles (warnings OK)

- [ ] **Step 4: Commit**

```bash
git add crates/nexad/src/adapters/runtime/docker.rs
git commit -m "feat: implement Docker event stream with filtering for managed containers"
```

---

### Task 8: Wire event watcher into nexad main.rs

**Files:**
- Modify: `crates/nexad/src/main.rs`

- [ ] **Step 1: Add event watcher startup to main()**

In `crates/nexad/src/main.rs`, after the orchestrator handle is created and before `api::serve()`, add:

```rust
use crate::adapters::event_watcher;

// ... inside main(), after `let handle = Orchestrator::spawn(Arc::new(runtime));`

// Start the container event watcher
let event_tx = handle.command_sender();
event_watcher::spawn_event_watcher(Arc::clone(&runtime_arc), event_tx);
info!("container event watcher started");
```

Update the full main function to store the runtime in an Arc:

```rust
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

    let runtime_arc: Arc<dyn nexa_core::ports::runtime::ContainerRuntime> = Arc::new(runtime);
    let handle = Orchestrator::spawn(Arc::clone(&runtime_arc));

    // Start container event watcher
    adapters::event_watcher::spawn_event_watcher(Arc::clone(&runtime_arc), handle.command_sender());
    info!("container event watcher started");

    let addr = format!("{}:{}", cli.host, cli.port);
    api::serve(handle, &addr).await
}
```

- [ ] **Step 2: Update MockRuntime in orchestrator tests to implement events()**

In the orchestrator test module's `MockRuntime`, add:

```rust
async fn events(&self) -> Result<EventStream> {
    // Return an empty stream for tests — event watcher is tested separately
    Ok(Box::pin(futures::stream::pending()))
}
```

- [ ] **Step 3: Verify full workspace compiles and all tests pass**

Run: `cargo check 2>&1`
Expected: compiles

Run: `cargo test 2>&1`
Expected: all tests pass (domain::restart, domain::orchestrator, adapters::event_watcher, config)

- [ ] **Step 4: Commit**

```bash
git add crates/nexad/src/main.rs crates/nexa-core/src/domain/orchestrator.rs
git commit -m "feat: wire container event watcher into nexad startup"
```

- [ ] **Step 5: Final integration verification**

Run: `cargo build 2>&1`
Expected: clean build

Run: `cargo test 2>&1`
Expected: all tests pass

Run: `cargo clippy 2>&1`
Expected: no errors (warnings OK)

- [ ] **Step 6: Push to remote**

```bash
git push origin main
```
