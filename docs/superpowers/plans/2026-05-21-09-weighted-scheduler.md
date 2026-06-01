# Scheduler (Weighted Scoring) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Multi-Repo Path Mapping:** This project uses separate repos. Translate paths as follows:
> | Plan path prefix | Repo | Local path |
> |---|---|---|
> | `crates/nexa-core/` | [`nexa-core`](https://github.com/nexa-net/nexa-core) | `/Users/nassime/GitHub/nexa-core/` |
> | `crates/nexad/` | [`nexad`](https://github.com/nexa-net/nexad) | `/Users/nassime/GitHub/nexad/` |
> | `crates/nexa-cli/` | [`nexa-cli`](https://github.com/nexa-net/nexa-cli) | `/Users/nassime/GitHub/nexa-cli/` |
>
> `cargo check -p <crate>` → `cargo check` in the target repo. `nexa-core` dep: `git = "https://github.com/nexa-net/nexa-core"`

**Goal:** Implement a weighted scoring scheduler that assigns pods to cluster nodes based on configurable CPU, memory, load, and failure weights, supporting both spread and binpack strategies.

**Architecture:** The scheduler lives in `nexa-core/src/domain/scheduler.rs` as a pure domain component with zero infrastructure dependencies. `WeightedScheduler` scores each candidate `NodeSnapshot` using a normalized weighted formula, returning the highest-scoring node. Failure penalty uses exponential decay over recent failure timestamps. The orchestrator calls `scheduler.select_node()` during pod creation, and a new `cluster config` CLI subcommand persists scheduler weights to a `cluster_config` table. In single-node mode the same code path runs -- the scheduler trivially returns the only candidate.

**Tech Stack:** chrono (DateTime, Utc), uuid (Uuid), serde (Serialize, Deserialize for weights/config persistence), nexa-core error types

---

### Task 1: Create domain/scheduler.rs with core types

**Files:**
- Create: `crates/nexa-core/src/domain/scheduler.rs`
- Modify: `crates/nexa-core/src/domain/mod.rs`

- [ ] **Step 1: Write failing test for SchedulerWeights default (spread)**

Add `crates/nexa-core/src/domain/scheduler.rs` with test-first structure:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{NexaError, Result};

/// Configurable weights for the scoring function.
/// All weights should be positive; binpack inverts resource weights internally.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SchedulerWeights {
    pub cpu: f64,
    pub memory: f64,
    pub load: f64,
    pub failure: f64,
}

impl SchedulerWeights {
    /// Spread strategy: prefer nodes with the most available resources.
    pub fn spread() -> Self {
        Self {
            cpu: 0.35,
            memory: 0.35,
            load: 0.15,
            failure: 0.15,
        }
    }

    /// Binpack strategy: prefer nodes that are already busy (consolidate workloads).
    pub fn binpack() -> Self {
        Self {
            cpu: -0.30,
            memory: -0.30,
            load: -0.10,
            failure: 0.15,
        }
    }
}

impl Default for SchedulerWeights {
    fn default() -> Self {
        Self::spread()
    }
}

/// Point-in-time snapshot of a node's resources for scheduling decisions.
/// Uses *reserved* (sum of placed pods' requests), not *used* (actual from heartbeat).
#[derive(Debug, Clone)]
pub struct NodeSnapshot {
    pub node_id: Uuid,
    pub cpu_available: f64,
    pub cpu_total: f64,
    pub memory_available: u64,
    pub memory_total: u64,
    pub running_pods: u32,
    pub max_pods: u32,
    pub recent_failures: Vec<DateTime<Utc>>,
}

/// Resource request extracted from a pod's deployment spec.
/// Pods without resource spec are treated as 0 (best-effort).
#[derive(Debug, Clone)]
pub struct PodRequest {
    pub cpu_request: f64,
    pub memory_request: u64,
}

impl Default for PodRequest {
    fn default() -> Self {
        Self {
            cpu_request: 0.0,
            memory_request: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_weights_sum_to_one() {
        let w = SchedulerWeights::spread();
        let sum = w.cpu + w.memory + w.load + w.failure;
        assert!((sum - 1.0).abs() < 1e-9, "spread weights sum = {sum}");
    }

    #[test]
    fn binpack_has_negative_resource_weights() {
        let w = SchedulerWeights::binpack();
        assert!(w.cpu < 0.0);
        assert!(w.memory < 0.0);
        assert!(w.load < 0.0);
        assert!(w.failure > 0.0, "failure weight should always be positive");
    }

    #[test]
    fn default_is_spread() {
        assert_eq!(SchedulerWeights::default(), SchedulerWeights::spread());
    }

    #[test]
    fn pod_request_default_is_best_effort() {
        let req = PodRequest::default();
        assert_eq!(req.cpu_request, 0.0);
        assert_eq!(req.memory_request, 0);
    }
}
```

- [ ] **Step 2: Register the module in domain/mod.rs**

In `crates/nexa-core/src/domain/mod.rs`, add:
```rust
pub mod scheduler;
```

(This file already contains `pub mod models;` and `pub mod orchestrator;` from Plans #1/#3.)

- [ ] **Step 3: Run tests to verify they pass**

```bash
cargo test -p nexa-core -- domain::scheduler 2>&1
```

Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/nexa-core/src/domain/scheduler.rs crates/nexa-core/src/domain/mod.rs
git commit -m "feat(scheduler): add SchedulerWeights, NodeSnapshot, PodRequest domain types"
```

---

### Task 2: Implement failure_penalty() function with tests

**Files:**
- Modify: `crates/nexa-core/src/domain/scheduler.rs`

- [ ] **Step 1: Write failing tests for failure_penalty**

Add these tests to the `tests` module in `crates/nexa-core/src/domain/scheduler.rs`:

```rust
    #[test]
    fn failure_penalty_no_failures_is_zero() {
        let now = Utc::now();
        let penalty = failure_penalty(&[], now);
        assert_eq!(penalty, 0.0);
    }

    #[test]
    fn failure_penalty_recent_failure_is_high() {
        let now = Utc::now();
        let failures = vec![now]; // just happened
        let penalty = failure_penalty(&failures, now);
        // exp(0/10) = exp(0) = 1.0
        assert!((penalty - 1.0).abs() < 1e-9, "penalty = {penalty}");
    }

    #[test]
    fn failure_penalty_old_failure_decays() {
        let now = Utc::now();
        let old = now - chrono::Duration::minutes(30);
        let failures = vec![old];
        let penalty = failure_penalty(&failures, now);
        // exp(-30/10) = exp(-3) ~ 0.0498
        assert!(penalty < 0.06, "penalty = {penalty}");
        assert!(penalty > 0.04, "penalty = {penalty}");
    }

    #[test]
    fn failure_penalty_capped_at_one() {
        let now = Utc::now();
        // 10 recent failures should sum > 1.0 but get capped
        let failures: Vec<DateTime<Utc>> = (0..10).map(|_| now).collect();
        let penalty = failure_penalty(&failures, now);
        assert!((penalty - 1.0).abs() < 1e-9, "penalty = {penalty}");
    }

    #[test]
    fn failure_penalty_multiple_mixed_ages() {
        let now = Utc::now();
        let f1 = now; // exp(0) = 1.0
        let f2 = now - chrono::Duration::minutes(10); // exp(-1) ~ 0.368
        let failures = vec![f1, f2];
        let penalty = failure_penalty(&failures, now);
        // sum = 1.0 + 0.368 = 1.368, capped at 1.0
        assert!((penalty - 1.0).abs() < 1e-9, "penalty = {penalty}");
    }

    #[test]
    fn failure_penalty_single_10min_ago() {
        let now = Utc::now();
        let failure = now - chrono::Duration::minutes(10);
        let penalty = failure_penalty(&[failure], now);
        // exp(-10/10) = exp(-1) ~ 0.3679
        assert!((penalty - (-1.0_f64).exp()).abs() < 1e-4, "penalty = {penalty}");
    }
```

- [ ] **Step 2: Run tests -- they should fail (function does not exist)**

```bash
cargo test -p nexa-core -- domain::scheduler 2>&1
```

Expected: compilation error -- `failure_penalty` not found.

- [ ] **Step 3: Implement failure_penalty**

Add this function above the `#[cfg(test)]` block in `crates/nexa-core/src/domain/scheduler.rs`:

```rust
/// Compute a failure penalty using exponential decay.
///
/// Each recent failure contributes `exp(-age_minutes / 10.0)`.
/// A failure that just happened contributes 1.0; a failure 10 minutes ago
/// contributes ~0.37; a failure 30 minutes ago contributes ~0.05.
/// The sum is capped at 1.0.
pub fn failure_penalty(failures: &[DateTime<Utc>], now: DateTime<Utc>) -> f64 {
    failures
        .iter()
        .map(|t| {
            let age_minutes = (now - *t).num_minutes() as f64;
            (-age_minutes / 10.0).exp()
        })
        .sum::<f64>()
        .min(1.0)
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p nexa-core -- domain::scheduler 2>&1
```

Expected: all 10 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-core/src/domain/scheduler.rs
git commit -m "feat(scheduler): implement failure_penalty with exponential decay"
```

---

### Task 3: Implement score_node() with tests (spread mode)

**Files:**
- Modify: `crates/nexa-core/src/domain/scheduler.rs`

- [ ] **Step 1: Write failing tests for score_node**

Add these tests to the `tests` module:

```rust
    fn make_node(
        cpu_available: f64,
        cpu_total: f64,
        mem_available: u64,
        mem_total: u64,
        running_pods: u32,
        max_pods: u32,
    ) -> NodeSnapshot {
        NodeSnapshot {
            node_id: Uuid::new_v4(),
            cpu_available,
            cpu_total,
            memory_available: mem_available,
            memory_total: mem_total,
            running_pods,
            max_pods,
            recent_failures: vec![],
        }
    }

    #[test]
    fn score_node_fully_idle_spread() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let node = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 0, 100);
        let req = PodRequest { cpu_request: 0.5, memory_request: 512_000_000 };
        let score = scheduler.score_node(&req, &node);
        // cpu_ratio  = (4.0 - 0.5) / 4.0 = 0.875
        // mem_ratio  = (8G - 512M) / 8G  = 0.9375
        // load_ratio = 0 / 100 = 0.0
        // failure    = 0.0
        // score = 0.35 * 0.875 + 0.35 * 0.9375 - 0.15 * 0.0 - 0.15 * 0.0
        //       = 0.30625 + 0.328125 = 0.634375
        assert!((score - 0.634375).abs() < 1e-6, "score = {score}");
    }

    #[test]
    fn score_node_half_loaded_spread() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let node = make_node(2.0, 4.0, 4_000_000_000, 8_000_000_000, 50, 100);
        let req = PodRequest { cpu_request: 0.0, memory_request: 0 };
        let score = scheduler.score_node(&req, &node);
        // cpu_ratio  = 2.0 / 4.0 = 0.5
        // mem_ratio  = 4G / 8G   = 0.5
        // load_ratio = 50 / 100  = 0.5
        // failure    = 0.0
        // score = 0.35 * 0.5 + 0.35 * 0.5 - 0.15 * 0.5 - 0.0
        //       = 0.175 + 0.175 - 0.075 = 0.275
        assert!((score - 0.275).abs() < 1e-6, "score = {score}");
    }

    #[test]
    fn score_node_insufficient_cpu_returns_negative_infinity() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let node = make_node(0.2, 4.0, 8_000_000_000, 8_000_000_000, 0, 100);
        let req = PodRequest { cpu_request: 1.0, memory_request: 0 };
        let score = scheduler.score_node(&req, &node);
        assert!(score == f64::NEG_INFINITY, "score = {score}");
    }

    #[test]
    fn score_node_insufficient_memory_returns_negative_infinity() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let node = make_node(4.0, 4.0, 100_000_000, 8_000_000_000, 0, 100);
        let req = PodRequest { cpu_request: 0.0, memory_request: 512_000_000 };
        let score = scheduler.score_node(&req, &node);
        assert!(score == f64::NEG_INFINITY, "score = {score}");
    }

    #[test]
    fn score_node_at_max_pods_returns_negative_infinity() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let node = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 100, 100);
        let req = PodRequest::default();
        let score = scheduler.score_node(&req, &node);
        assert!(score == f64::NEG_INFINITY, "score = {score}");
    }

    #[test]
    fn score_node_with_failures_penalized() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let now = Utc::now();
        let mut node = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 0, 100);
        node.recent_failures = vec![now]; // just failed => penalty = 1.0
        let req = PodRequest::default();
        let score = scheduler.score_node(&req, &node);
        // cpu_ratio = 4/4 = 1.0, mem_ratio = 1.0, load = 0, failure = 1.0
        // score = 0.35*1.0 + 0.35*1.0 - 0.15*0.0 - 0.15*1.0
        //       = 0.35 + 0.35 - 0.15 = 0.55
        assert!((score - 0.55).abs() < 1e-6, "score = {score}");
    }

    #[test]
    fn score_node_best_effort_pod_uses_full_capacity() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let node = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 0, 100);
        let req = PodRequest::default(); // 0 cpu, 0 memory
        let score = scheduler.score_node(&req, &node);
        // cpu_ratio = 4/4 = 1.0, mem_ratio = 8G/8G = 1.0, load = 0, failure = 0
        // score = 0.35 + 0.35 = 0.70
        assert!((score - 0.70).abs() < 1e-6, "score = {score}");
    }
```

- [ ] **Step 2: Run tests -- they should fail (WeightedScheduler does not exist)**

```bash
cargo test -p nexa-core -- domain::scheduler 2>&1
```

Expected: compilation error.

- [ ] **Step 3: Implement WeightedScheduler struct and score_node**

Add the `WeightedScheduler` struct and `score_node` method (above the `#[cfg(test)]` block):

```rust
pub struct WeightedScheduler {
    weights: SchedulerWeights,
}

impl WeightedScheduler {
    pub fn new(weights: SchedulerWeights) -> Self {
        Self { weights }
    }

    pub fn weights(&self) -> &SchedulerWeights {
        &self.weights
    }

    /// Score a single node for a given pod request.
    ///
    /// Returns `f64::NEG_INFINITY` if the node cannot fit the pod
    /// (insufficient CPU, memory, or at max pod count).
    ///
    /// The scoring formula:
    /// ```text
    /// score = w_cpu    * (cpu_after / cpu_total)
    ///       + w_memory * (memory_after / memory_total)
    ///       - w_load   * (running_pods / max_pods)
    ///       - w_fail   * failure_penalty(recent_failures)
    /// ```
    /// All resource terms reflect capacity *after* placing this pod.
    pub fn score_node(&self, request: &PodRequest, node: &NodeSnapshot) -> f64 {
        // Hard constraints: reject if pod doesn't fit
        if node.running_pods >= node.max_pods {
            return f64::NEG_INFINITY;
        }
        if request.cpu_request > 0.0 && node.cpu_available < request.cpu_request {
            return f64::NEG_INFINITY;
        }
        if request.memory_request > 0 && node.memory_available < request.memory_request {
            return f64::NEG_INFINITY;
        }

        // Compute post-placement available resources
        let cpu_after = node.cpu_available - request.cpu_request;
        let mem_after = node.memory_available.saturating_sub(request.memory_request);

        // Normalize to 0.0-1.0
        let cpu_ratio = if node.cpu_total > 0.0 {
            cpu_after / node.cpu_total
        } else {
            0.0
        };
        let mem_ratio = if node.memory_total > 0 {
            mem_after as f64 / node.memory_total as f64
        } else {
            0.0
        };
        let load_ratio = if node.max_pods > 0 {
            node.running_pods as f64 / node.max_pods as f64
        } else {
            1.0
        };

        let fail_penalty = failure_penalty(&node.recent_failures, Utc::now());

        self.weights.cpu * cpu_ratio
            + self.weights.memory * mem_ratio
            - self.weights.load * load_ratio
            - self.weights.failure * fail_penalty
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p nexa-core -- domain::scheduler 2>&1
```

Expected: all 17 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/nexa-core/src/domain/scheduler.rs
git commit -m "feat(scheduler): implement WeightedScheduler.score_node with hard constraints and weighted scoring"
```

---

### Task 4: Implement select_node() with tests (multiple nodes, edge cases)

**Files:**
- Modify: `crates/nexa-core/src/domain/scheduler.rs`

- [ ] **Step 1: Write failing tests for select_node**

Add these tests to the `tests` module:

```rust
    #[test]
    fn select_node_picks_highest_score() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let req = PodRequest { cpu_request: 0.5, memory_request: 512_000_000 };

        let idle_node = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 0, 100);
        let busy_node = make_node(1.0, 4.0, 2_000_000_000, 8_000_000_000, 80, 100);

        let idle_id = idle_node.node_id;
        let nodes = vec![busy_node, idle_node];

        let selected = scheduler.select_node(&req, &nodes).unwrap();
        assert_eq!(selected, idle_id);
    }

    #[test]
    fn select_node_skips_insufficient_nodes() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let req = PodRequest { cpu_request: 2.0, memory_request: 0 };

        let small = make_node(1.0, 2.0, 8_000_000_000, 8_000_000_000, 0, 100);
        let big = make_node(4.0, 8.0, 8_000_000_000, 8_000_000_000, 0, 100);

        let big_id = big.node_id;
        let nodes = vec![small, big];

        let selected = scheduler.select_node(&req, &nodes).unwrap();
        assert_eq!(selected, big_id);
    }

    #[test]
    fn select_node_empty_list_returns_error() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let req = PodRequest::default();
        let result = scheduler.select_node(&req, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn select_node_all_nodes_insufficient_returns_error() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let req = PodRequest { cpu_request: 8.0, memory_request: 0 };

        let n1 = make_node(2.0, 4.0, 8_000_000_000, 8_000_000_000, 0, 100);
        let n2 = make_node(1.0, 4.0, 8_000_000_000, 8_000_000_000, 0, 100);

        let result = scheduler.select_node(&req, &[n1, n2]);
        assert!(result.is_err());
    }

    #[test]
    fn select_node_prefers_node_without_failures() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let req = PodRequest::default();

        let clean = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 0, 100);
        let clean_id = clean.node_id;

        let mut failed = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 0, 100);
        failed.recent_failures = vec![Utc::now()];

        let nodes = vec![failed, clean];
        let selected = scheduler.select_node(&req, &nodes).unwrap();
        assert_eq!(selected, clean_id);
    }

    #[test]
    fn select_node_single_node_returns_it() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let req = PodRequest::default();
        let node = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 0, 100);
        let node_id = node.node_id;

        let selected = scheduler.select_node(&req, &[node]).unwrap();
        assert_eq!(selected, node_id);
    }

    #[test]
    fn select_node_three_nodes_picks_best() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::spread());
        let req = PodRequest { cpu_request: 1.0, memory_request: 1_000_000_000 };

        let n1 = make_node(2.0, 4.0, 4_000_000_000, 8_000_000_000, 50, 100);
        let n2 = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 10, 100);
        let n3 = make_node(3.0, 4.0, 6_000_000_000, 8_000_000_000, 30, 100);

        let best_id = n2.node_id;
        let nodes = vec![n1, n2, n3];

        let selected = scheduler.select_node(&req, &nodes).unwrap();
        assert_eq!(selected, best_id);
    }
```

- [ ] **Step 2: Run tests -- they should fail (select_node does not exist)**

```bash
cargo test -p nexa-core -- domain::scheduler 2>&1
```

Expected: compilation error.

- [ ] **Step 3: Implement select_node**

Add this method inside the `impl WeightedScheduler` block:

```rust
    /// Select the best node for the given pod request.
    ///
    /// Scores all candidate nodes and returns the `node_id` of the highest-scoring
    /// node. Returns `Err(NexaError::SchedulingFailed)` if no node can fit the pod
    /// (all scores are `NEG_INFINITY`) or the node list is empty.
    pub fn select_node(&self, request: &PodRequest, nodes: &[NodeSnapshot]) -> Result<Uuid> {
        if nodes.is_empty() {
            return Err(NexaError::SchedulingFailed(
                "no candidate nodes available".into(),
            ));
        }

        let mut best_id: Option<Uuid> = None;
        let mut best_score = f64::NEG_INFINITY;

        for node in nodes {
            let score = self.score_node(request, node);
            if score > best_score {
                best_score = score;
                best_id = Some(node.node_id);
            }
        }

        match best_id {
            Some(id) if best_score > f64::NEG_INFINITY => Ok(id),
            _ => Err(NexaError::SchedulingFailed(
                "no node has sufficient resources".into(),
            )),
        }
    }
```

- [ ] **Step 4: Add SchedulingFailed variant to NexaError**

In `crates/nexa-core/src/error.rs`, add a new variant to the `NexaError` enum:

```rust
    #[error("scheduling failed: {0}")]
    SchedulingFailed(String),
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test -p nexa-core -- domain::scheduler 2>&1
```

Expected: all 24 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/nexa-core/src/domain/scheduler.rs crates/nexa-core/src/error.rs
git commit -m "feat(scheduler): implement select_node with best-score selection and SchedulingFailed error"
```

---

### Task 5: Add binpack mode support with tests

**Files:**
- Modify: `crates/nexa-core/src/domain/scheduler.rs`

- [ ] **Step 1: Write failing tests for binpack behavior**

Add these tests to the `tests` module:

```rust
    #[test]
    fn binpack_prefers_busy_node() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::binpack());
        let req = PodRequest { cpu_request: 0.5, memory_request: 512_000_000 };

        let idle = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 5, 100);
        let busy = make_node(2.0, 4.0, 3_000_000_000, 8_000_000_000, 60, 100);

        let busy_id = busy.node_id;
        let nodes = vec![idle, busy];

        let selected = scheduler.select_node(&req, &nodes).unwrap();
        assert_eq!(selected, busy_id);
    }

    #[test]
    fn binpack_still_rejects_insufficient_resources() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::binpack());
        let req = PodRequest { cpu_request: 3.0, memory_request: 0 };

        let busy = make_node(1.0, 4.0, 8_000_000_000, 8_000_000_000, 60, 100);
        let idle = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 5, 100);

        let idle_id = idle.node_id;
        let nodes = vec![busy, idle];

        let selected = scheduler.select_node(&req, &nodes).unwrap();
        assert_eq!(selected, idle_id);
    }

    #[test]
    fn binpack_score_lower_for_idle_node() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::binpack());
        let req = PodRequest::default();

        let idle = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 0, 100);
        let busy = make_node(1.0, 4.0, 2_000_000_000, 8_000_000_000, 70, 100);

        let idle_score = scheduler.score_node(&req, &idle);
        let busy_score = scheduler.score_node(&req, &busy);

        assert!(
            busy_score > idle_score,
            "binpack should prefer busy: idle={idle_score}, busy={busy_score}"
        );
    }

    #[test]
    fn binpack_still_penalizes_failures() {
        let scheduler = WeightedScheduler::new(SchedulerWeights::binpack());
        let req = PodRequest::default();

        let mut clean = make_node(1.0, 4.0, 2_000_000_000, 8_000_000_000, 70, 100);
        let mut failed = make_node(1.0, 4.0, 2_000_000_000, 8_000_000_000, 70, 100);
        failed.recent_failures = vec![Utc::now()];

        let clean_id = clean.node_id;
        let nodes = vec![failed, clean];

        let selected = scheduler.select_node(&req, &nodes).unwrap();
        assert_eq!(selected, clean_id);
    }

    #[test]
    fn spread_and_binpack_pick_opposite_nodes() {
        let req = PodRequest { cpu_request: 0.5, memory_request: 512_000_000 };

        let idle = make_node(4.0, 4.0, 8_000_000_000, 8_000_000_000, 5, 100);
        let busy = make_node(2.0, 4.0, 3_000_000_000, 8_000_000_000, 60, 100);

        let idle_id = idle.node_id;
        let busy_id = busy.node_id;

        let spread = WeightedScheduler::new(SchedulerWeights::spread());
        let binpack = WeightedScheduler::new(SchedulerWeights::binpack());

        let nodes = vec![idle.clone(), busy.clone()];

        let spread_pick = spread.select_node(&req, &nodes).unwrap();
        let binpack_pick = binpack.select_node(&req, &nodes).unwrap();

        assert_eq!(spread_pick, idle_id, "spread should pick idle");
        assert_eq!(binpack_pick, busy_id, "binpack should pick busy");
    }
```

- [ ] **Step 2: Run tests to verify they pass**

The binpack logic is already handled by the negative weights in `SchedulerWeights::binpack()` combined with the existing `score_node` implementation. The negative CPU/memory weights invert the preference so that *less* available capacity = *higher* score.

```bash
cargo test -p nexa-core -- domain::scheduler 2>&1
```

Expected: all 29 tests pass. No new production code needed -- the scoring formula inherently supports binpack via negative weights.

- [ ] **Step 3: Commit**

```bash
git add crates/nexa-core/src/domain/scheduler.rs
git commit -m "test(scheduler): add binpack mode tests proving negative weight inversion"
```

---

### Task 6: Integrate scheduler into orchestrator deploy flow

**Files:**
- Modify: `crates/nexa-core/src/domain/orchestrator.rs`
- Modify: `crates/nexa-core/src/domain/models/pod.rs`

- [ ] **Step 1: Write failing tests for scheduler integration**

Add to the `tests` module in `crates/nexa-core/src/domain/orchestrator.rs`:

```rust
    use crate::domain::scheduler::{SchedulerWeights, WeightedScheduler, NodeSnapshot};

    #[tokio::test]
    async fn deploy_assigns_node_id_to_pod() {
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
            resources: None,
            secrets: vec![],
        };

        handle.deploy(spec).await.unwrap();
        let pods = handle.list_pods(Some("test".into())).await;
        assert_eq!(pods.len(), 1);
        // In single-node mode, pod should have a node_id assigned
        assert!(pods[0].node_id.is_some());
    }
```

- [ ] **Step 2: Run tests -- they should fail**

```bash
cargo test -p nexa-core -- domain::orchestrator::tests::deploy_assigns_node_id 2>&1
```

Expected: compilation error -- `node_id` does not exist on `Pod`.

- [ ] **Step 3: Add node_id field to Pod**

In `crates/nexa-core/src/domain/models/pod.rs` (or `crates/nexa-core/src/models/pod.rs` depending on Plan #1 state), add a `node_id` field to the `Pod` struct:

```rust
use uuid::Uuid;

// In the Pod struct, add:
    pub node_id: Option<Uuid>,
```

And in `Pod::new()`, initialize it:
```rust
    node_id: None,
```

- [ ] **Step 4: Add scheduler to Orchestrator**

In `crates/nexa-core/src/domain/orchestrator.rs`, modify the `Orchestrator` struct to include the scheduler and local node ID:

```rust
use crate::domain::scheduler::{
    NodeSnapshot, PodRequest, SchedulerWeights, WeightedScheduler,
};

pub struct Orchestrator {
    runtime: Arc<dyn ContainerRuntime>,
    projects: StdHashMap<String, Project>,
    deployments: StdHashMap<Uuid, Deployment>,
    pods: StdHashMap<Uuid, Pod>,
    scheduler: WeightedScheduler,
    local_node_id: Uuid,
}
```

Update `Orchestrator::spawn` to accept optional weights and create a local node ID:

```rust
    pub fn spawn(runtime: Arc<dyn ContainerRuntime>) -> OrchestratorHandle {
        Self::spawn_with_weights(runtime, SchedulerWeights::default())
    }

    pub fn spawn_with_weights(
        runtime: Arc<dyn ContainerRuntime>,
        weights: SchedulerWeights,
    ) -> OrchestratorHandle {
        let (tx, rx) = mpsc::channel(256);
        let local_node_id = Uuid::new_v4();
        tokio::spawn(async move {
            let mut orch = Self {
                runtime,
                projects: StdHashMap::new(),
                deployments: StdHashMap::new(),
                pods: StdHashMap::new(),
                scheduler: WeightedScheduler::new(weights),
                local_node_id,
            };
            orch.run(rx).await;
        });
        OrchestratorHandle { tx }
    }
```

- [ ] **Step 5: Build NodeSnapshot from local state in create_pod**

Modify `create_pod` in the orchestrator to call the scheduler before creating a pod. Add a helper method:

```rust
    /// Build a NodeSnapshot for the local node based on current reserved resources.
    fn local_node_snapshot(&self) -> NodeSnapshot {
        // Sum all reserved resources from existing pods
        // For now, single-node mode with default capacity
        let running_pods = self.pods.values().filter(|p| {
            matches!(p.status, PodStatus::Running | PodStatus::Creating)
        }).count() as u32;

        NodeSnapshot {
            node_id: self.local_node_id,
            cpu_available: 4.0,   // TODO: read from actual node registration
            cpu_total: 4.0,
            memory_available: 8_000_000_000,
            memory_total: 8_000_000_000,
            running_pods,
            max_pods: 110,
            recent_failures: vec![],
        }
    }
```

In `create_pod`, after creating the `Pod` object, assign the node_id:

```rust
    async fn create_pod(
        &mut self,
        deployment_id: Uuid,
        spec: &DeploymentSpec,
        index: u32,
    ) -> Result<()> {
        let mut pod = Pod::new(
            deployment_id,
            &spec.project,
            &spec.deployment.name,
            index,
            &spec.image,
        );

        // Schedule: determine which node this pod should run on
        let pod_request = PodRequest {
            cpu_request: spec.resources.as_ref().map(|r| r.cpu).unwrap_or(0.0),
            memory_request: spec.resources.as_ref().map(|r| r.memory_bytes()).unwrap_or(0),
        };
        let snapshot = self.local_node_snapshot();
        let node_id = self.scheduler.select_node(&pod_request, &[snapshot])?;
        pod.node_id = Some(node_id);

        // ... rest of container creation unchanged ...
    }
```

- [ ] **Step 6: Run tests to verify they pass**

```bash
cargo test -p nexa-core -- domain::orchestrator 2>&1
```

Expected: all orchestrator tests pass (including the new one).

- [ ] **Step 7: Commit**

```bash
git add crates/nexa-core/src/domain/orchestrator.rs crates/nexa-core/src/domain/models/pod.rs
git commit -m "feat(scheduler): integrate WeightedScheduler into orchestrator pod creation"
```

---

### Task 7: Add scheduler config commands (CLI + API)

**Files:**
- Modify: `crates/nexa-core/src/domain/scheduler.rs` (add SchedulerConfig, strategy parsing)
- Modify: `crates/nexa-core/src/domain/orchestrator.rs` (add GetSchedulerConfig/SetSchedulerConfig commands)
- Modify: `crates/nexad/src/api/handlers.rs` (add scheduler config endpoints)
- Modify: `crates/nexad/src/api/routes.rs` (add scheduler config routes)
- Modify: `crates/nexa-cli/src/main.rs` (add `cluster config` subcommands)
- Modify: `crates/nexa-cli/src/commands.rs` (implement config functions)

- [ ] **Step 1: Write tests for SchedulerConfig parsing**

Add to the `tests` module in `crates/nexa-core/src/domain/scheduler.rs`:

```rust
    #[test]
    fn scheduler_config_from_strategy_spread() {
        let config = SchedulerConfig::from_strategy("spread").unwrap();
        assert_eq!(config.strategy, "spread");
        assert_eq!(config.weights, SchedulerWeights::spread());
    }

    #[test]
    fn scheduler_config_from_strategy_binpack() {
        let config = SchedulerConfig::from_strategy("binpack").unwrap();
        assert_eq!(config.strategy, "binpack");
        assert_eq!(config.weights, SchedulerWeights::binpack());
    }

    #[test]
    fn scheduler_config_from_strategy_invalid() {
        let result = SchedulerConfig::from_strategy("random");
        assert!(result.is_err());
    }

    #[test]
    fn scheduler_config_with_custom_weight() {
        let mut config = SchedulerConfig::from_strategy("spread").unwrap();
        config.set_weight("cpu", 0.5).unwrap();
        assert_eq!(config.weights.cpu, 0.5);
        assert_eq!(config.strategy, "custom");
    }

    #[test]
    fn scheduler_config_set_invalid_weight_name() {
        let mut config = SchedulerConfig::from_strategy("spread").unwrap();
        let result = config.set_weight("disk", 0.5);
        assert!(result.is_err());
    }
```

- [ ] **Step 2: Implement SchedulerConfig**

Add to `crates/nexa-core/src/domain/scheduler.rs`:

```rust
/// Persisted scheduler configuration. Stored in cluster_config table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    pub strategy: String,
    pub weights: SchedulerWeights,
}

impl SchedulerConfig {
    pub fn from_strategy(strategy: &str) -> Result<Self> {
        let weights = match strategy {
            "spread" => SchedulerWeights::spread(),
            "binpack" => SchedulerWeights::binpack(),
            _ => {
                return Err(NexaError::InvalidSpec(format!(
                    "unknown scheduler strategy: '{strategy}'. Valid: spread, binpack"
                )));
            }
        };
        Ok(Self {
            strategy: strategy.to_string(),
            weights,
        })
    }

    /// Set an individual weight by name. Marks strategy as "custom".
    pub fn set_weight(&mut self, name: &str, value: f64) -> Result<()> {
        match name {
            "cpu" => self.weights.cpu = value,
            "memory" => self.weights.memory = value,
            "load" => self.weights.load = value,
            "failure" => self.weights.failure = value,
            _ => {
                return Err(NexaError::InvalidSpec(format!(
                    "unknown weight: '{name}'. Valid: cpu, memory, load, failure"
                )));
            }
        }
        self.strategy = "custom".to_string();
        Ok(())
    }
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            strategy: "spread".to_string(),
            weights: SchedulerWeights::spread(),
        }
    }
}
```

- [ ] **Step 3: Run tests to verify SchedulerConfig works**

```bash
cargo test -p nexa-core -- domain::scheduler 2>&1
```

Expected: all 34 tests pass.

- [ ] **Step 4: Add orchestrator commands for scheduler config**

In `crates/nexa-core/src/domain/orchestrator.rs`, add two new `Command` variants:

```rust
    GetSchedulerConfig {
        reply: oneshot::Sender<SchedulerConfig>,
    },
    SetSchedulerConfig {
        config: SchedulerConfig,
        reply: oneshot::Sender<Result<SchedulerConfig>>,
    },
```

Add the handler methods:

```rust
    fn handle_get_scheduler_config(&self) -> SchedulerConfig {
        SchedulerConfig {
            strategy: "spread".to_string(), // TODO: persist and track current strategy name
            weights: self.scheduler.weights().clone(),
        }
    }

    fn handle_set_scheduler_config(&mut self, config: SchedulerConfig) -> Result<SchedulerConfig> {
        self.scheduler = WeightedScheduler::new(config.weights.clone());
        Ok(config)
    }
```

Wire them in the `run` loop:

```rust
                Command::GetSchedulerConfig { reply } => {
                    let _ = reply.send(self.handle_get_scheduler_config());
                }
                Command::SetSchedulerConfig { config, reply } => {
                    let _ = reply.send(self.handle_set_scheduler_config(config));
                }
```

Add handle methods:

```rust
    pub async fn get_scheduler_config(&self) -> SchedulerConfig {
        let (reply, rx) = oneshot::channel();
        let _ = self.tx.send(Command::GetSchedulerConfig { reply }).await;
        rx.await.unwrap_or_default()
    }

    pub async fn set_scheduler_config(&self, config: SchedulerConfig) -> Result<SchedulerConfig> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Command::SetSchedulerConfig { config, reply })
            .await
            .map_err(|_| NexaError::Runtime("orchestrator stopped".into()))?;
        rx.await
            .map_err(|_| NexaError::Runtime("orchestrator dropped reply".into()))?
    }
```

- [ ] **Step 5: Add API routes for scheduler config**

In `crates/nexad/src/api/routes.rs`, add two routes:

```rust
        .route("/api/v1/cluster/scheduler", get(handlers::get_scheduler_config))
        .route("/api/v1/cluster/scheduler", post(handlers::set_scheduler_config))
```

In `crates/nexad/src/api/handlers.rs`, add the handler functions:

```rust
use nexa_core::domain::scheduler::SchedulerConfig;

pub async fn get_scheduler_config(State(handle): AppState) -> impl IntoResponse {
    Json(handle.get_scheduler_config().await)
}

#[derive(Deserialize)]
pub struct SetSchedulerRequest {
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(default)]
    pub weights: Option<SetWeightRequest>,
}

#[derive(Deserialize)]
pub struct SetWeightRequest {
    pub name: String,
    pub value: f64,
}

pub async fn set_scheduler_config(
    State(handle): AppState,
    Json(req): Json<SetSchedulerRequest>,
) -> impl IntoResponse {
    let current = handle.get_scheduler_config().await;

    let config = if let Some(strategy) = req.strategy {
        match SchedulerConfig::from_strategy(&strategy) {
            Ok(c) => c,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
        }
    } else if let Some(weight) = req.weights {
        let mut config = current;
        match config.set_weight(&weight.name, weight.value) {
            Ok(()) => config,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
        }
    } else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "provide 'strategy' or 'weights'" })),
        )
            .into_response();
    };

    match handle.set_scheduler_config(config).await {
        Ok(config) => Json(serde_json::json!(config)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
```

- [ ] **Step 6: Add CLI `cluster config` subcommands**

In `crates/nexa-cli/src/main.rs`, add a new `Cluster` subcommand with nested `Config` sub-subcommand:

```rust
    /// Manage cluster settings
    Cluster {
        #[command(subcommand)]
        command: ClusterCommands,
    },
```

```rust
#[derive(Subcommand)]
enum ClusterCommands {
    /// Manage cluster configuration
    Config {
        #[command(subcommand)]
        command: ClusterConfigCommands,
    },
}

#[derive(Subcommand)]
enum ClusterConfigCommands {
    /// Get scheduler configuration
    GetScheduler,

    /// Set scheduler strategy or individual weight
    Set {
        /// Key: "scheduler" for strategy, or "scheduler.weights.<name>" for individual weight
        key: String,
        /// Value: strategy name (spread/binpack) or weight number
        value: String,
    },
}
```

Wire it in the main match:

```rust
        Commands::Cluster { command } => match command {
            ClusterCommands::Config { command } => match command {
                ClusterConfigCommands::GetScheduler => {
                    commands::get_scheduler_config(&client).await
                }
                ClusterConfigCommands::Set { key, value } => {
                    commands::set_cluster_config(&client, &key, &value).await
                }
            },
        },
```

- [ ] **Step 7: Implement CLI config functions**

In `crates/nexa-cli/src/commands.rs`, add:

```rust
pub async fn get_scheduler_config(client: &NexaClient) -> Result<()> {
    let config: serde_json::Value = client.get("/api/v1/cluster/scheduler").await?;
    println!("Scheduler configuration:");
    println!("  Strategy: {}", config["strategy"]);
    println!("  Weights:");
    if let Some(weights) = config.get("weights") {
        println!("    cpu:     {}", weights["cpu"]);
        println!("    memory:  {}", weights["memory"]);
        println!("    load:    {}", weights["load"]);
        println!("    failure: {}", weights["failure"]);
    }
    Ok(())
}

pub async fn set_cluster_config(client: &NexaClient, key: &str, value: &str) -> Result<()> {
    let body = if key == "scheduler" {
        serde_json::json!({ "strategy": value }).to_string()
    } else if let Some(weight_name) = key.strip_prefix("scheduler.weights.") {
        let num: f64 = value
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid weight value: {value}"))?;
        serde_json::json!({ "weights": { "name": weight_name, "value": num } }).to_string()
    } else {
        anyhow::bail!("unknown config key: {key}. Supported: scheduler, scheduler.weights.<name>");
    };

    let config: serde_json::Value = client.post_json("/api/v1/cluster/scheduler", &body).await?;
    output::print_success(&format!("Scheduler config updated: {}", config["strategy"]));
    Ok(())
}
```

- [ ] **Step 8: Verify full workspace compiles**

```bash
cargo check 2>&1
```

Expected: compiles.

- [ ] **Step 9: Run all tests**

```bash
cargo test 2>&1
```

Expected: all tests pass.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "feat(scheduler): add cluster config CLI/API for scheduler strategy and weight customization"
```

---

### Task 8: Single-node mode -- scheduler returns local node

**Files:**
- Modify: `crates/nexa-core/src/domain/orchestrator.rs` (tests)

This task verifies that the existing implementation already handles single-node mode correctly. No new production code is needed -- the orchestrator already builds a single `NodeSnapshot` for the local node and the scheduler picks it.

- [ ] **Step 1: Write explicit single-node integration tests**

Add to the `tests` module in `crates/nexa-core/src/domain/orchestrator.rs`:

```rust
    #[tokio::test]
    async fn single_node_mode_assigns_all_pods_to_local() {
        let handle = spawn_test_orchestrator();

        let spec = DeploymentSpec {
            project: "test".into(),
            deployment: DeploymentMeta { name: "web".into() },
            replicas: 3,
            image: "nginx".into(),
            ports: vec![],
            env: HashMap::new(),
            volumes: vec![],
            network: None,
            healthcheck: None,
            restart: RestartPolicy::default(),
            resources: None,
            secrets: vec![],
        };

        handle.deploy(spec).await.unwrap();
        let pods = handle.list_pods(None).await;
        assert_eq!(pods.len(), 3);

        // All pods should be assigned to the same (local) node
        let node_ids: Vec<Uuid> = pods.iter().filter_map(|p| p.node_id).collect();
        assert_eq!(node_ids.len(), 3, "all pods must have node_id");
        assert!(
            node_ids.iter().all(|id| *id == node_ids[0]),
            "all pods should be on the same local node"
        );
    }

    #[tokio::test]
    async fn single_node_scheduler_works_with_binpack_weights() {
        let (tx, rx) = mpsc::channel(256);
        let local_node_id = Uuid::new_v4();
        tokio::spawn(async move {
            let mut orch = Orchestrator {
                runtime: Arc::new(MockRuntime),
                projects: StdHashMap::new(),
                deployments: StdHashMap::new(),
                pods: StdHashMap::new(),
                scheduler: WeightedScheduler::new(SchedulerWeights::binpack()),
                local_node_id,
            };
            orch.run(rx).await;
        });
        let handle = OrchestratorHandle { tx };

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
            resources: None,
            secrets: vec![],
        };

        handle.deploy(spec).await.unwrap();
        let pods = handle.list_pods(None).await;
        assert_eq!(pods.len(), 2);
        assert!(pods.iter().all(|p| p.node_id == Some(local_node_id)));
    }

    #[tokio::test]
    async fn scheduler_config_can_be_changed_at_runtime() {
        let handle = spawn_test_orchestrator();

        let config = handle.get_scheduler_config().await;
        assert_eq!(config.strategy, "spread");

        let binpack = SchedulerConfig::from_strategy("binpack").unwrap();
        let updated = handle.set_scheduler_config(binpack).await.unwrap();
        assert_eq!(updated.strategy, "binpack");

        // Verify subsequent deploys use new weights
        let config = handle.get_scheduler_config().await;
        assert_eq!(config.weights, SchedulerWeights::binpack());
    }
```

- [ ] **Step 2: Run tests to verify they pass**

```bash
cargo test -p nexa-core -- domain::orchestrator 2>&1
```

Expected: all orchestrator tests pass, including the 3 new single-node tests.

- [ ] **Step 3: Run full test suite**

```bash
cargo test 2>&1
```

Expected: all tests pass across the workspace.

- [ ] **Step 4: Commit**

```bash
git add crates/nexa-core/src/domain/orchestrator.rs
git commit -m "test(scheduler): add single-node mode and runtime config change integration tests"
```

- [ ] **Step 5: Push to remote**

```bash
git push origin main
```
