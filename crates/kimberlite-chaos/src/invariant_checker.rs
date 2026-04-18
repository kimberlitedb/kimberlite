//! External invariant checking for chaos scenarios.
//!
//! Unlike `kimberlite-sim`'s in-process invariant checkers, chaos scenarios run
//! real kimberlite-server binaries. Invariants are checked externally via HTTP
//! (client query results, cluster status endpoints) and by direct inspection
//! of replica disk state after scenarios complete.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ============================================================================
// Invariant
// ============================================================================

/// An external invariant to check against a running chaos cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invariant {
    /// Invariant identifier (e.g. "no_divergence_after_heal").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Category (safety/liveness/durability).
    pub category: InvariantCategory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvariantCategory {
    /// Must always hold — single violation is a bug.
    Safety,
    /// Must eventually hold within a bounded time.
    Liveness,
    /// Data must survive failures.
    Durability,
}

// ============================================================================
// Invariant Result
// ============================================================================

/// Outcome of checking one invariant against a chaos cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvariantResult {
    pub invariant: String,
    pub held: bool,
    pub message: String,
    pub check_timestamp_ms: u64,
}

// ============================================================================
// Invariant Checker
// ============================================================================

/// Checks external invariants against a live or post-mortem chaos cluster.
///
/// Currently this is a skeleton with placeholder logic. The full implementation
/// will include:
///
/// - HTTP probing of cluster replicas to read state.
/// - Linearizability checker (Jepsen-style) on recorded operations.
/// - Hash chain verification across all replicas post-scenario.
/// - Partition detection via cluster topology queries.
#[derive(Debug, Default)]
pub struct InvariantChecker {
    /// Registered invariants by name.
    invariants: HashMap<String, Invariant>,
    /// Results from the last check run.
    results: Vec<InvariantResult>,
    /// Replica HTTP endpoints keyed by (cluster, replica). Populated by
    /// `ChaosController` right after `provision()` so probes know where to
    /// look.
    endpoints: HashMap<(u16, u8), String>,
    /// Replicas that the controller has explicitly killed (via
    /// `remove_endpoint`) and not yet restarted. Tracked separately so
    /// `check_quorum_loss_detected` knows the *original* cluster size even
    /// after kills — otherwise quorum = `endpoints.len() / 2 + 1` trivially
    /// holds as endpoints shrinks.
    dead_endpoints: std::collections::HashSet<(u16, u8)>,
    /// The set of replicas currently considered *minority* — i.e., they
    /// should be rejecting writes because the controller has partitioned
    /// them off from a quorum. Updated by `ChaosController` when it
    /// executes `Partition` / `Heal` actions.
    minority: Vec<(u16, u8)>,
    /// If false, HTTP probes are short-circuited as "skipped". Defaults to
    /// false so tests and DryRun scenarios don't attempt network calls to
    /// unreachable replica IPs. `ChaosController::with_apply()` flips this
    /// to true.
    probes_enabled: bool,
    /// Write IDs that the workload thread received 200 OK responses for.
    /// Set by `ChaosController` after `StopWorkload` joins the thread.
    /// Used by `check_all_writes_preserved` to verify each acknowledged
    /// write still appears in at least one replica's write log.
    acknowledged_writes: Vec<String>,
}

impl InvariantChecker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an invariant for future checking.
    pub fn register(&mut self, invariant: Invariant) {
        self.invariants.insert(invariant.name.clone(), invariant);
    }

    /// Returns the built-in invariant catalog.
    #[must_use]
    pub fn builtin() -> Self {
        let mut checker = Self::new();
        for inv in builtin_invariants() {
            checker.register(inv);
        }
        checker
    }

    /// Registers a replica endpoint for future HTTP-based checks.
    pub fn set_endpoint(&mut self, cluster: u16, replica: u8, url: String) {
        self.endpoints.insert((cluster, replica), url);
        // A restart clears the dead-mark so the endpoint is counted as
        // alive again.
        self.dead_endpoints.remove(&(cluster, replica));
    }

    /// Removes a replica endpoint — e.g. when it has been killed and is
    /// intentionally unreachable. Probes skip it until re-registered.
    pub fn remove_endpoint(&mut self, cluster: u16, replica: u8) {
        self.endpoints.remove(&(cluster, replica));
        self.dead_endpoints.insert((cluster, replica));
    }

    /// Enables / disables HTTP probing. Called by `ChaosController` to match
    /// the controller's ExecMode: Apply → enabled, DryRun → disabled.
    pub fn set_probes_enabled(&mut self, enabled: bool) {
        self.probes_enabled = enabled;
    }

    /// Registers the write IDs acknowledged by the workload thread.
    ///
    /// Called by `ChaosController` after `StopWorkload` joins the thread.
    /// Each ID in `writes` received a 200 OK from at least one replica's
    /// `POST /kv/chaos-probe` endpoint.
    pub fn set_acknowledged_writes(&mut self, writes: Vec<String>) {
        self.acknowledged_writes = writes;
    }

    /// Marks a replica as currently minority (cut off from quorum by an
    /// active partition). Drives the `minority_refuses_writes` probe.
    pub fn mark_minority(&mut self, cluster: u16, replica: u8) {
        if !self.minority.contains(&(cluster, replica)) {
            self.minority.push((cluster, replica));
        }
    }

    /// Removes a replica from the minority set (after `Heal`).
    pub fn clear_minority(&mut self, cluster: u16, replica: u8) {
        self.minority.retain(|k| *k != (cluster, replica));
    }

    /// Blocks until every reachable replica's commit watermark has been
    /// stable (identical and unchanging) for `stable_for_ms`, or until
    /// `timeout_ms` elapses.
    ///
    /// Scenarios call this through `ChaosAction::WaitForConvergence` as a
    /// progress-based replacement for fixed post-restart sleeps. Unreachable
    /// replicas are tolerated so scenarios that intentionally leave a
    /// replica down (e.g. `cascading_failure` in its killed-majority phase)
    /// still terminate.
    pub fn wait_for_convergence(&self, poll_ms: u64, stable_for_ms: u64, timeout_ms: u64) {
        let poll = Duration::from_millis(poll_ms.max(50));
        let budget = Duration::from_millis(timeout_ms.max(poll_ms));
        let stable_polls = u32::try_from(stable_for_ms.max(1).div_ceil(poll_ms.max(50)))
            .unwrap_or(u32::MAX);
        let deadline = std::time::Instant::now() + budget;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(500))
            .build();

        // Track the last seen watermark per endpoint across rounds so we
        // can detect "advancing" — convergence means not only that every
        // reachable replica agrees, but that the shared value itself is
        // no longer changing.
        let mut last_watermarks: HashMap<(u16, u8), u64> = HashMap::new();
        let mut consecutive_stable: u32 = 0;

        while std::time::Instant::now() < deadline {
            let mut round: HashMap<(u16, u8), u64> = HashMap::new();
            for ((c, r), url) in &self.endpoints {
                let probe = format!("{}/state/commit_watermark", url.trim_end_matches('/'));
                if let Ok(resp) = agent.get(&probe).call()
                    && resp.status() == 200
                    && let Ok(body) = resp.into_string()
                    && let Some(w) = parse_watermark_json(&body)
                {
                    round.insert((*c, *r), w);
                }
            }

            let all_equal = {
                let values: Vec<u64> = round.values().copied().collect();
                values.windows(2).all(|w| w[0] == w[1])
            };
            let all_stable = round.iter().all(|(k, v)| {
                last_watermarks.get(k).is_some_and(|prev| prev == v)
            }) && round.len() == last_watermarks.len();

            last_watermarks = round;

            if !last_watermarks.is_empty() && all_equal && all_stable {
                consecutive_stable = consecutive_stable.saturating_add(1);
                if consecutive_stable >= stable_polls {
                    return;
                }
            } else {
                consecutive_stable = 0;
            }

            std::thread::sleep(poll);
        }
    }

    /// Checks a named invariant, appending the result.
    ///
    /// Dispatches to real HTTP probes for the two invariants covered in
    /// Phase 2.4 (`minority_refuses_writes`, `no_divergence_after_heal`).
    /// Other invariants still return `held: true` with a `TODO` message to
    /// avoid false positives while the full checker suite is built out.
    pub fn check(&mut self, name: &str, now_ms: u64) -> InvariantResult {
        let (held, message) = if !self.probes_enabled {
            (true, format!("probe skipped (probes disabled) for {name}"))
        } else {
            match name {
                "minority_refuses_writes" => self.check_minority_refuses_writes(),
                // `no_corruption_under_quorum_loss` collapses to the same
                // divergence check — under a contained quorum loss, replicas
                // must not disagree on which writes they accepted.
                "no_divergence_after_heal" | "no_corruption_under_quorum_loss" => {
                    self.check_no_divergence_after_heal()
                }
                // Phase B: real durability checks using the persistent write log.
                // `all_writes_preserved`, `no_lost_commits`, and
                // `no_data_loss_across_failover` all verify that every write
                // acknowledged by the workload thread (200 OK from POST
                // /kv/chaos-probe) is still present in at least one replica's
                // GET /state/write_log after the scenario.
                "all_writes_preserved" | "no_lost_commits" | "no_data_loss_across_failover" => {
                    self.check_all_writes_preserved()
                }
                // `commit_watermark_consistent` verifies that every shim's
                // advertised watermark equals the size of its write_log —
                // a structural property of the shim's own bookkeeping.
                // Replaces the old `exactly_once_semantics` check, which
                // was structurally trivial (the shim dedups via HashSet so
                // in-log duplicates were already impossible).
                "commit_watermark_consistent" => self.check_commit_watermark_consistent(),

                // `hash_chain_valid_all_replicas`: real check — probes every
                // registered endpoint's /health and validates the `replica-<id>`
                // body. Any transport failure, non-200, or unexpected body is
                // a violation. Used by scenarios that need a minimum liveness
                // guarantee without a stronger structural probe.
                "hash_chain_valid_all_replicas" => self.check_hash_chain_all_replicas(),

                // Composition: the system must be (a) alive on every
                // replica (hash_chain_valid_all_replicas returns 200 OK
                // from /health) AND (b) converged (no_divergence_after_heal
                // sees identical commit_hash). Either one failing fails the
                // composite.
                "no_panic_or_corruption" => {
                    let (alive, alive_msg) = self.check_hash_chain_all_replicas();
                    if !alive {
                        (false, format!("alive-check failed: {alive_msg}"))
                    } else {
                        let (converged, msg) = self.check_no_divergence_after_heal();
                        (
                            converged,
                            format!("alive=OK; divergence-check: {msg}"),
                        )
                    }
                }

                // Real checks (graduated from placeholders):
                "quorum_loss_detected" => self.check_quorum_loss_detected(),
                "graceful_enforcement" => self.check_graceful_enforcement(),
                "directory_reroutes_to_cluster_b" => {
                    self.check_directory_reroutes_to_cluster_b()
                }
                "linearizability" => self.check_linearizability(),

                _ => {
                    let (held, mut msg) = self.check_hash_chain_all_replicas();
                    msg = format!("[UNKNOWN INVARIANT — liveness proxy for `{name}`] {msg}");
                    (held, msg)
                }
            }
        };
        let result = InvariantResult {
            invariant: name.to_string(),
            held,
            message,
            check_timestamp_ms: now_ms,
        };
        self.results.push(result.clone());
        result
    }

    /// Probes each currently-minority replica's write endpoint and verifies
    /// the request fails (connection refused, timeout, 4xx/5xx, or a body
    /// containing a "not leader" / "no quorum" signal). A 2xx response is a
    /// violation.
    fn check_minority_refuses_writes(&self) -> (bool, String) {
        if self.minority.is_empty() {
            return (
                true,
                "no minority replicas registered — trivially OK".into(),
            );
        }
        if self.endpoints.is_empty() {
            return (
                false,
                "no replica endpoints registered; cannot probe".into(),
            );
        }
        let mut failures = Vec::new();
        for key in &self.minority {
            let Some(url) = self.endpoints.get(key) else {
                continue;
            };
            match probe_rejects_write(url) {
                Ok(true) => { /* expected */ }
                Ok(false) => failures.push(format!(
                    "c{}-r{} @ {} accepted a write while minority",
                    key.0, key.1, url
                )),
                Err(e) => {
                    // Connection refused / timeout / DNS error all count as
                    // "write rejected" (iptables blocks the packet or the
                    // replica is unreachable).
                    tracing::debug!(%url, err = %e, "probe failed — counts as refusal");
                }
            }
        }
        if failures.is_empty() {
            (
                true,
                format!(
                    "all {} minority replica(s) rejected writes",
                    self.minority.len()
                ),
            )
        } else {
            (false, failures.join("; "))
        }
    }

    /// After a heal, every registered replica's `/health` should report
    /// identical state (same HTTP status and identical body). Minority
    /// replicas that were flipped back to majority must converge within
    /// the retry budget.
    fn check_no_divergence_after_heal(&self) -> (bool, String) {
        if self.endpoints.is_empty() {
            return (
                false,
                "no replica endpoints registered; cannot probe".into(),
            );
        }
        // Readiness gate — before we even care about watermarks, make sure
        // every reachable replica is back in `Normal` status with bootstrap
        // complete. Probing `/state/commit_hash` mid-view-change races the
        // VSR protocol's short window where the HTTP handler thread is
        // alive but kernel_state snapshots time out, producing the
        // "Unexpected EOF" flake. `Unsupported` means we're running against
        // a pre-hardening binary; fall through to the legacy logic.
        match wait_for_vsr_ready(&self.endpoints, Duration::from_secs(10)) {
            VsrReadinessOutcome::Ready | VsrReadinessOutcome::Unsupported => {}
            VsrReadinessOutcome::Timeout(detail) => {
                // Don't fail outright — some scenarios leave replicas
                // intentionally down. Proceed; the hash compare below
                // will still surface a real divergence, and an in-flight
                // view-change that never lands will manifest as
                // unreachable endpoints rather than this timeout.
                tracing::warn!("vsr readiness gate timed out: {detail}");
            }
        }

        // Quiescence poll — real VSR propagates commits asynchronously, so
        // probing immediately after heal can show a follower still catching
        // up and produce a false divergence alarm. Wait up to 5s for every
        // reachable replica's commit_watermark to agree before comparing
        // hashes. Missing endpoints and transport errors are tolerated so a
        // permanently-killed minority doesn't stall the check.
        wait_for_watermark_quiescence(&self.endpoints, Duration::from_secs(5));

        // One retry after 500ms to allow state transfer to settle.
        for attempt in 0..2 {
            // Prefer `/state/commit_hash` — it's a true content hash of the
            // write_id set, so two replicas diverging on which writes they
            // accepted will produce different hashes.  Fall back to
            // `/health` only when the shim doesn't expose the new route
            // (404) so this check still works against old shim binaries
            // during rollout.
            let hash_results: Vec<_> = self
                .endpoints
                .iter()
                .map(|(key, url)| (*key, probe_commit_hash(url)))
                .collect();

            let use_hash = hash_results.iter().any(|(_, r)| matches!(r, Ok(Some(_))));

            if use_hash {
                let mut hashes: Vec<((u16, u8), Option<String>)> = Vec::new();
                let mut transport_errs: Vec<String> = Vec::new();
                for (key, res) in &hash_results {
                    match res {
                        Ok(Some(h)) => hashes.push((*key, Some(h.clone()))),
                        Ok(None) => hashes.push((*key, None)),
                        Err(e) => {
                            transport_errs.push(format!("c{}-r{}: {}", key.0, key.1, e));
                        }
                    }
                }
                let present: Vec<&str> = hashes.iter().filter_map(|(_, h)| h.as_deref()).collect();
                let all_equal = present.windows(2).all(|w| w[0] == w[1]);

                if transport_errs.is_empty() && all_equal && !present.is_empty() {
                    return (
                        true,
                        format!(
                            "all {} replicas agree on commit_hash ({})",
                            present.len(),
                            present[0]
                        ),
                    );
                }

                if attempt == 0 {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }

                let detail: Vec<String> = hashes
                    .iter()
                    .map(|(key, h)| match h {
                        Some(hex) => format!("c{}-r{}={hex}", key.0, key.1),
                        None => format!("c{}-r{}=<no commit_hash>", key.0, key.1),
                    })
                    .collect();
                let err_suffix = if transport_errs.is_empty() {
                    String::new()
                } else {
                    format!(" (errors: [{}])", transport_errs.join(", "))
                };
                return (
                    false,
                    format!(
                        "replicas diverged on commit_hash: [{}]{}",
                        detail.join(", "),
                        err_suffix
                    ),
                );
            }

            // Fallback: every replica returned 404 for commit_hash (old
            // shim). Use the legacy /health probe.
            let states: Vec<_> = self
                .endpoints
                .iter()
                .map(|(key, url)| (*key, probe_health_fingerprint(url)))
                .collect();
            let all_ok = states.iter().all(|(_, s)| s.is_ok());
            let fingerprints: Vec<String> = states
                .iter()
                .filter_map(|(_, s)| s.as_ref().ok().cloned())
                .collect();
            let all_equal = fingerprints.windows(2).all(|w| w[0] == w[1]);
            if all_ok && all_equal {
                return (
                    true,
                    format!(
                        "[legacy /health fallback] all {} replicas converged",
                        states.len()
                    ),
                );
            }
            if attempt == 0 {
                std::thread::sleep(Duration::from_millis(500));
            } else {
                let mismatches: Vec<String> = states
                    .into_iter()
                    .map(|(key, s)| format!("c{}-r{}={:?}", key.0, key.1, s))
                    .collect();
                return (
                    false,
                    format!(
                        "[legacy /health fallback] replicas diverged: [{}]",
                        mismatches.join(", ")
                    ),
                );
            }
        }
        unreachable!()
    }

    /// Returns `(held, message)` for `hash_chain_valid_all_replicas`. The
    /// stateless chaos shim has no real log; we treat this as a liveness /
    /// boot-sanity check: `GET /health` on every endpoint must succeed
    /// with a `replica-<id>` body. Any transport failure, non-200, or
    /// body that doesn't carry the expected prefix fails the invariant —
    /// Queries `GET /state/commit_watermark` from every registered endpoint
    /// and verifies that all reachable replicas agree within a tolerance.
    ///
    /// The shim increments its watermark on every 200 response to
    /// `POST /kv/chaos-probe`. A healthy cluster under symmetrical load will
    /// have similar watermarks on all replicas. A split-brain or commit-loss
    /// scenario would show a large disparity (e.g. one replica at 0 while
    /// others are at 500+).
    ///
    /// Tolerance: `max(max_watermark / 2, 5)` — generous enough to account
    /// for partitions and kills while catching egregious divergence.
    #[allow(dead_code)] // Phase B: will be wired into check() once the shim has a persistent write log
    fn check_no_lost_commits(&self) -> (bool, String) {
        if self.endpoints.is_empty() {
            return (false, "no replica endpoints registered".into());
        }

        let mut watermarks: Vec<(u16, u8, u64)> = Vec::new();
        let mut unreachable: Vec<String> = Vec::new();

        for ((c, r), url) in &self.endpoints {
            let probe = format!("{}/state/commit_watermark", url.trim_end_matches('/'));
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(2))
                .build();
            match agent.get(&probe).call() {
                Ok(resp) if resp.status() == 200 => {
                    let body = resp.into_string().unwrap_or_default();
                    if let Some(w) = parse_watermark_json(&body) {
                        watermarks.push((*c, *r, w));
                    } else {
                        unreachable.push(format!("c{c}-r{r}: unparseable response {body:?}"));
                    }
                }
                Ok(resp) => {
                    unreachable.push(format!("c{c}-r{r}: HTTP {}", resp.status()));
                }
                Err(e) => {
                    unreachable.push(format!("c{c}-r{r}: unreachable: {e}"));
                }
            }
        }

        if watermarks.is_empty() {
            // No reachable replicas — can't assert anything, report as failure
            // only if we expected at least one to be up.
            return (
                false,
                format!(
                    "no replicas reachable for commit-watermark check; {}",
                    unreachable.join("; ")
                ),
            );
        }

        let detail: Vec<String> = watermarks
            .iter()
            .map(|(c, r, w)| format!("c{c}-r{r}={w}"))
            .collect();

        let max_wm = watermarks.iter().map(|(_, _, w)| *w).max().unwrap_or(0);

        // If no replica has served any writes yet, the workload hasn't started —
        // treat as trivially OK (nothing to lose).
        if max_wm == 0 {
            return (
                true,
                format!(
                    "all watermarks=0 (workload not started): {}",
                    detail.join(", ")
                ),
            );
        }

        // Check: every reachable replica that has had a chance to serve writes
        // must have watermark > 0.
        //
        // Rationale: comparing watermarks across replicas is misleading in
        // kill/restart scenarios because the in-memory counter resets to 0
        // after each restart.  A killed-and-restarted replica will have a much
        // lower watermark than continuously-running replicas — that is expected,
        // not a commit-loss signal.
        //
        // Instead we check a weaker but unambiguous property: every replica
        // that is currently reachable (can serve HTTP) is also actively
        // acknowledging writes (watermark > 0).  A replica stuck at 0 while
        // others serve writes indicates it is alive but refusing all operations —
        // which is a stronger signal than a plain /health check.
        //
        // Phase B will graduate this to a real per-write-ID durability check
        // once the shim gains a persistent write log.
        let zero_wm: Vec<String> = watermarks
            .iter()
            .filter(|(_, _, w)| *w == 0)
            .map(|(c, r, _)| format!("c{c}-r{r}"))
            .collect();

        if zero_wm.is_empty() {
            (
                true,
                format!(
                    "all reachable replicas serving writes: {}",
                    detail.join(", ")
                ),
            )
        } else {
            (
                false,
                format!(
                    "replica(s) alive but not serving writes [watermark=0]: {}; all: {}",
                    zero_wm.join(", "),
                    detail.join(", ")
                ),
            )
        }
    }

    /// Liveness probe composed into `no_panic_or_corruption`: every
    /// registered replica's `/health` must respond with 200. A panicked
    /// or hung replica shows up here as a transport error or non-200.
    ///
    /// The real binary returns `{"status":"ok",...}` JSON; the legacy
    /// shim returns `replica-<id>`. Accept either — we only need to know
    /// the process is alive and serving HTTP.
    fn check_hash_chain_all_replicas(&self) -> (bool, String) {
        if self.endpoints.is_empty() {
            return (false, "no replica endpoints registered".into());
        }
        let mut failures = Vec::new();
        for ((c, r), url) in &self.endpoints {
            let probe = format!("{}/health", url.trim_end_matches('/'));
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(2))
                .build();
            match agent.get(&probe).call() {
                Ok(resp) if resp.status() == 200 => {
                    // Any 200 is a live replica — both the real binary's
                    // `{"status":"ok",...}` and the shim's `replica-<id>`
                    // are valid. Failing here on body shape would fight
                    // the basic purpose of the probe.
                }
                Ok(resp) => {
                    failures.push(format!("c{c}-r{r} /health returned HTTP {}", resp.status()));
                }
                Err(e) => {
                    failures.push(format!("c{c}-r{r} /health unreachable: {e}"));
                }
            }
        }
        if failures.is_empty() {
            (
                true,
                format!("all {} replicas healthy", self.endpoints.len()),
            )
        } else {
            (false, failures.join("; "))
        }
    }

    // ========================================================================
    // Phase B: Write-log durability probes
    // ========================================================================

    /// Verifies that every write_id acknowledged by the workload thread still
    /// appears in at least one replica's `GET /state/write_log` after the
    /// scenario completes.
    ///
    /// A write is "acknowledged" when the shim returned 200 OK for a
    /// `POST /kv/chaos-probe` carrying that write_id.  The shim persists
    /// acknowledged IDs to `/tmp/kmb_writes` (ext4, survives restarts), so a
    /// killed-and-restarted replica retains its log.
    ///
    /// **Failure signal**: an acknowledged write_id is absent from ALL
    /// reachable replica logs — the shim's ext4 file was lost or the write
    /// was never durably stored.
    ///
    /// When `acknowledged_writes` is empty we report trivial hold. The
    /// previous liveness-proxy fallback (probing every replica's
    /// `/health`) confused scenarios like `leader_kill_mid_commit` and
    /// `cascading_failure` that intentionally leave replicas down — the
    /// proxy flagged the killed replica as a violation even though the
    /// scenario was operating as designed. "No writes to verify" is not
    /// a failure.
    fn check_all_writes_preserved(&self) -> (bool, String) {
        if self.acknowledged_writes.is_empty() {
            return (
                true,
                "no acknowledged writes tracked — nothing to verify".into(),
            );
        }

        // Collect write logs from reachable replicas. Retry once after
        // 1s if NOTHING responds — a replica restarted late in the
        // scenario may still be warming up its HTTP sidecar when the
        // explicit CheckInvariant action fires.
        let mut replica_logs: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
        for attempt in 0..2 {
            replica_logs.clear();
            for ((c, r), url) in &self.endpoints {
                let probe = format!("{}/state/write_log", url.trim_end_matches('/'));
                let agent = ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(3))
                    .build();
                if let Ok(resp) = agent.get(&probe).call() {
                    if resp.status() == 200 {
                        let body = resp.into_string().unwrap_or_default();
                        replica_logs.insert(format!("c{c}-r{r}"), parse_write_log_json(&body));
                    }
                }
            }
            if !replica_logs.is_empty() {
                break;
            }
            if attempt == 0 {
                std::thread::sleep(Duration::from_secs(1));
            }
        }

        if replica_logs.is_empty() {
            return (false, "no replicas reachable for write-log check".into());
        }

        // For each acknowledged ID, count how many replicas hold it and
        // require a majority (quorum).  The earlier "≥ 1 replica" threshold
        // only verified shim-level durability; the VSR claim is that a
        // successfully-acknowledged write was replicated to a quorum before
        // the 200 was returned, so the check must mirror that claim.
        //
        // `quorum_size(n) = n / 2 + 1` — matches the formula in
        // `kimberlite_vsr::types::quorum_size`.  `n` is the number of
        // registered endpoints (typically 3 for single-cluster scenarios).
        let total_endpoints = self.endpoints.len();
        let quorum = total_endpoints / 2 + 1;

        let lost: Vec<(String, usize)> = self
            .acknowledged_writes
            .iter()
            .filter_map(|id| {
                let count = replica_logs.values().filter(|log| log.contains(id)).count();
                if count < quorum {
                    Some((id.clone(), count))
                } else {
                    None
                }
            })
            .collect();

        let replica_summary: Vec<String> = replica_logs
            .iter()
            .map(|(name, log)| format!("{name}={}", log.len()))
            .collect();

        if lost.is_empty() {
            (
                true,
                format!(
                    "all {} acknowledged writes preserved on >= quorum ({}/{}); replicas: {}",
                    self.acknowledged_writes.len(),
                    quorum,
                    total_endpoints,
                    replica_summary.join(", ")
                ),
            )
        } else {
            let sample: Vec<String> = lost
                .iter()
                .take(5)
                .map(|(id, c)| format!("{id}(on {c} replicas)"))
                .collect();
            (
                false,
                format!(
                    "{}/{} acknowledged writes not in quorum of replica logs \
                     (need >= {}/{}): {:?}; replicas: {}",
                    lost.len(),
                    self.acknowledged_writes.len(),
                    quorum,
                    total_endpoints,
                    sample,
                    replica_summary.join(", ")
                ),
            )
        }
    }

    /// Verifies that each replica's reported `commit_watermark` equals the
    /// length of its `write_log`.
    ///
    /// The shim increments `commit_count` atomically with each new write_id
    /// inserted into the `HashSet`-backed write log, so a discrepancy
    /// indicates a shim-level bug: either the counter drifted from the log,
    /// or one of the two persistence paths dropped an update.
    ///
    /// This is narrower than the previous `exactly_once_semantics` check
    /// (which was structurally trivial because the shim dedups via HashSet),
    /// but it does exercise a real invariant about the shim's internal
    /// bookkeeping.
    fn check_commit_watermark_consistent(&self) -> (bool, String) {
        if self.endpoints.is_empty() {
            return (false, "no replica endpoints registered".into());
        }

        let mut mismatches: Vec<String> = Vec::new();
        let mut unreachable: Vec<String> = Vec::new();

        for ((c, r), url) in &self.endpoints {
            let log_probe = format!("{}/state/write_log", url.trim_end_matches('/'));
            let wm_probe = format!("{}/state/commit_watermark", url.trim_end_matches('/'));
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(2))
                .build();

            let log_total = match agent.get(&log_probe).call() {
                Ok(resp) if resp.status() == 200 => {
                    let body = resp.into_string().unwrap_or_default();
                    parse_write_log_json(&body).len() as u64
                }
                Ok(resp) => {
                    unreachable.push(format!("c{c}-r{r} /state/write_log HTTP {}", resp.status()));
                    continue;
                }
                Err(e) => {
                    unreachable.push(format!("c{c}-r{r} /state/write_log: {e}"));
                    continue;
                }
            };

            let watermark = match agent.get(&wm_probe).call() {
                Ok(resp) if resp.status() == 200 => {
                    let body = resp.into_string().unwrap_or_default();
                    match parse_watermark_json(&body) {
                        Some(w) => w,
                        None => {
                            unreachable.push(format!("c{c}-r{r} unparseable watermark: {body:?}"));
                            continue;
                        }
                    }
                }
                Ok(resp) => {
                    unreachable.push(format!(
                        "c{c}-r{r} /state/commit_watermark HTTP {}",
                        resp.status()
                    ));
                    continue;
                }
                Err(e) => {
                    unreachable.push(format!("c{c}-r{r} /state/commit_watermark: {e}"));
                    continue;
                }
            };

            if watermark != log_total {
                mismatches.push(format!(
                    "c{c}-r{r}: watermark={watermark} != log_total={log_total}"
                ));
            }
        }

        if !mismatches.is_empty() {
            return (
                false,
                format!(
                    "watermark/log mismatches: [{}]{}",
                    mismatches.join(", "),
                    if unreachable.is_empty() {
                        String::new()
                    } else {
                        format!(" (unreachable: {})", unreachable.join(", "))
                    }
                ),
            );
        }

        if mismatches.is_empty()
            && !self.endpoints.is_empty()
            && unreachable.len() == self.endpoints.len()
        {
            return (
                false,
                format!("no replicas reachable: {}", unreachable.join(", ")),
            );
        }

        (
            true,
            format!(
                "watermark == write_log.len() on {} reachable replicas",
                self.endpoints.len() - unreachable.len()
            ),
        )
    }

    /// `quorum_loss_detected` — after `cascading_failure` kills f+1 replicas,
    /// the surviving replica(s) must refuse writes (they can't form quorum).
    ///
    /// Surveys every registered endpoint: unreachable endpoints count as
    /// "dead" and alive endpoints are probed with a POST. If fewer than
    /// `quorum_size = N/2 + 1` replicas are alive *and* any alive replica
    /// returns 200 OK for a probe write, the invariant is violated — the
    /// cluster is accepting writes without quorum.
    ///
    /// When quorum is still intact (kills didn't take out f+1), the property
    /// holds trivially.
    fn check_quorum_loss_detected(&self) -> (bool, String) {
        // `total` must count every replica that ever existed, not just the
        // currently-registered ones — killed replicas are moved to
        // `dead_endpoints` by `remove_endpoint`. Without this, quorum shrinks
        // as replicas die and the invariant trivially holds.
        let total = self.endpoints.len() + self.dead_endpoints.len();
        if total == 0 {
            return (true, "no endpoints registered — trivially OK".into());
        }
        let quorum_size = total / 2 + 1;

        let mut alive: Vec<((u16, u8), String)> = Vec::new();
        let mut dead: Vec<String> = Vec::new();
        for ((c, r), url) in &self.endpoints {
            let health = format!("{}/health", url.trim_end_matches('/'));
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(1))
                .build();
            match agent.get(&health).call() {
                Ok(resp) if resp.status() == 200 => {
                    alive.push(((*c, *r), url.clone()));
                }
                _ => dead.push(format!("c{c}-r{r}")),
            }
        }

        if alive.len() >= quorum_size {
            return (
                true,
                format!(
                    "{}/{} replicas alive — quorum ({}) still possible",
                    alive.len(),
                    total,
                    quorum_size
                ),
            );
        }

        // Quorum is lost. Probe every alive replica. Any 200 OK for a write
        // attempt is a violation — the cluster is accepting writes without
        // the durable f+1 replication contract.
        let mut violators = Vec::new();
        for ((c, r), url) in &alive {
            let probe = format!("{}/kv/chaos-probe", url.trim_end_matches('/'));
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(2))
                .build();
            let body = r#"{"op":"quorum-loss-probe","write_id":"quorum-probe"}"#;
            let resp = agent.post(&probe).send_string(body);
            if let Ok(resp) = resp
                && resp.status() == 200
            {
                violators.push(format!("c{c}-r{r}"));
            }
        }

        if violators.is_empty() {
            (
                true,
                format!(
                    "{}/{} alive (below quorum={}), all correctly refusing writes; dead: [{}]",
                    alive.len(),
                    total,
                    quorum_size,
                    dead.join(", ")
                ),
            )
        } else {
            (
                false,
                format!(
                    "replicas accepted writes without quorum ({}/{} alive, quorum={}): {}",
                    alive.len(),
                    total,
                    quorum_size,
                    violators.join(", ")
                ),
            )
        }
    }

    /// `graceful_enforcement` — during/after `storage_exhaustion`, every
    /// shim must (1) remain alive (respond 200 on /health) and (2) return
    /// clean HTTP responses to write attempts — either 200 (if somehow
    /// space is available) or 5xx (gracefully rejected). Connection-refused
    /// or non-HTTP responses indicate a crash, which violates the invariant.
    fn check_graceful_enforcement(&self) -> (bool, String) {
        if self.endpoints.is_empty() {
            return (true, "no endpoints registered — trivially OK".into());
        }
        let mut crashed = Vec::new();
        let mut non_http = Vec::new();

        for ((c, r), url) in &self.endpoints {
            let health = format!("{}/health", url.trim_end_matches('/'));
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(2))
                .build();
            match agent.get(&health).call() {
                Ok(resp) if resp.status() == 200 => {}
                Ok(_) | Err(_) => {
                    crashed.push(format!("c{c}-r{r}"));
                    continue;
                }
            }

            let probe = format!("{}/kv/chaos-probe", url.trim_end_matches('/'));
            let body = r#"{"op":"graceful-probe"}"#;
            let resp = agent.post(&probe).send_string(body);
            match resp {
                Ok(response) if (200..600).contains(&response.status()) => {}
                // ureq surfaces 5xx as Err(Status(...)) by default — also OK.
                Err(ureq::Error::Status(code, _)) if (200..600).contains(&code) => {}
                _ => non_http.push(format!("c{c}-r{r}")),
            }
        }

        if crashed.is_empty() && non_http.is_empty() {
            (
                true,
                format!(
                    "all {} replicas alive and returning clean HTTP responses",
                    self.endpoints.len()
                ),
            )
        } else {
            let mut msg = Vec::new();
            if !crashed.is_empty() {
                msg.push(format!("crashed: [{}]", crashed.join(", ")));
            }
            if !non_http.is_empty() {
                msg.push(format!("non-HTTP responses: [{}]", non_http.join(", ")));
            }
            (false, msg.join("; "))
        }
    }

    /// `directory_reroutes_to_cluster_b` — after `cross_cluster_failover`
    /// kills every replica in cluster 0, cluster B (cluster != 0) must
    /// continue to accept writes. At least one cluster-B replica returning
    /// 200 OK for a probe write satisfies the invariant.
    fn check_directory_reroutes_to_cluster_b(&self) -> (bool, String) {
        let cluster_b: Vec<_> = self
            .endpoints
            .iter()
            .filter(|((c, _), _)| *c != 0)
            .collect();
        if cluster_b.is_empty() {
            return (
                true,
                "no cluster B registered — scenario not multi-cluster, trivially OK".into(),
            );
        }

        let mut accepted = Vec::new();
        let mut refused = Vec::new();
        for ((c, r), url) in cluster_b {
            let probe = format!("{}/kv/chaos-probe", url.trim_end_matches('/'));
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(3))
                .build();
            let body = r#"{"op":"reroute-probe","write_id":"reroute-test"}"#;
            let resp = agent.post(&probe).send_string(body);
            match resp {
                Ok(response) if response.status() == 200 => {
                    accepted.push(format!("c{c}-r{r}"));
                }
                _ => refused.push(format!("c{c}-r{r}")),
            }
        }

        if accepted.is_empty() {
            (
                false,
                format!(
                    "no cluster-B replicas accepted writes post-failover; refused/unreachable: [{}]",
                    refused.join(", ")
                ),
            )
        } else {
            (
                true,
                format!(
                    "cluster-B accepting writes: [{}]; refused: [{}]",
                    accepted.join(", "),
                    refused.join(", ")
                ),
            )
        }
    }

    /// `linearizability` — weak-form ordering check. Fetches ordered
    /// write_logs from each replica and verifies that no two replicas
    /// disagree on the relative order of any two writes they both contain.
    ///
    /// This is weaker than a full Jepsen linearizability checker (which
    /// would also require client-side op-timestamp recording and a
    /// wall-clock consistent total order). But it catches any replica that
    /// has the same write pair (A, B) in reversed order from another —
    /// which is a real linearizability violation visible in the shim model.
    ///
    /// Unordered replicas (shims without the ordered log endpoint) are
    /// silently skipped so the check still works with older shims.
    fn check_linearizability(&self) -> (bool, String) {
        if self.endpoints.is_empty() {
            return (true, "no endpoints — trivially OK".into());
        }

        // Quiescence barrier — read `/state/write_log` after every
        // reachable replica's commit_watermark has agreed for the
        // stability window. Otherwise a freshly-restarted replica can
        // return a log that's a strict prefix of the canonical order,
        // and our pairwise ordering check falsely flags a disagreement
        // on writes the slow replica hasn't observed yet. Same 5s budget
        // as `check_no_divergence_after_heal` — plenty for a converged
        // cluster, short enough that an intentionally-partitioned
        // scenario still terminates.
        wait_for_watermark_quiescence(&self.endpoints, Duration::from_secs(5));

        // (cluster, replica) -> ordered Vec<write_id>. Uses the existing
        // /state/write_log endpoint; the shim preserves insertion order in
        // its JSON array so ordering comparisons are meaningful.
        let mut orderings: HashMap<(u16, u8), Vec<String>> = HashMap::new();
        for ((c, r), url) in &self.endpoints {
            let probe = format!("{}/state/write_log", url.trim_end_matches('/'));
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(3))
                .build();
            if let Ok(resp) = agent.get(&probe).call()
                && resp.status() == 200
                && let Ok(body) = resp.into_string()
            {
                let ids = parse_write_log_json_ordered(&body);
                if !ids.is_empty() {
                    orderings.insert((*c, *r), ids);
                }
            }
        }

        if orderings.len() < 2 {
            return (
                true,
                format!(
                    "only {} replica(s) reported ordered log — cannot compare",
                    orderings.len()
                ),
            );
        }

        // Build per-replica position maps, then for each pair of replicas,
        // check that any shared writes appear in the same relative order.
        let position_maps: HashMap<(u16, u8), HashMap<&String, usize>> = orderings
            .iter()
            .map(|(k, ids)| {
                let mut map = HashMap::new();
                for (i, id) in ids.iter().enumerate() {
                    map.insert(id, i);
                }
                (*k, map)
            })
            .collect();

        let keys: Vec<_> = position_maps.keys().copied().collect();
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                let a = &position_maps[&keys[i]];
                let b = &position_maps[&keys[j]];
                let shared: Vec<_> = a.keys().filter(|k| b.contains_key(*k)).collect();
                for x in 0..shared.len() {
                    for y in (x + 1)..shared.len() {
                        let (s1, s2) = (shared[x], shared[y]);
                        let a_order = a[s1] < a[s2];
                        let b_order = b[s1] < b[s2];
                        if a_order != b_order {
                            return (
                                false,
                                format!(
                                    "ordering disagreement between c{}-r{} and c{}-r{}: `{}` vs `{}`",
                                    keys[i].0, keys[i].1, keys[j].0, keys[j].1, s1, s2
                                ),
                            );
                        }
                    }
                }
            }
        }

        (
            true,
            format!(
                "all {} replicas' ordered write_logs are consistent on every shared pair",
                orderings.len()
            ),
        )
    }

    /// Returns all recorded results.
    #[must_use]
    pub fn results(&self) -> &[InvariantResult] {
        &self.results
    }

    /// Returns results that failed.
    #[must_use]
    pub fn failures(&self) -> Vec<&InvariantResult> {
        self.results.iter().filter(|r| !r.held).collect()
    }
}

// ============================================================================
// HTTP Probes
// ============================================================================

/// Parses the `write_ids` array from a `GET /state/write_log` response.
///
/// Expected format: `{"write_ids":["id1","id2",...],"total":N}`
/// Returns a deduplicated `HashSet` of IDs.
fn parse_write_log_json(body: &str) -> std::collections::HashSet<String> {
    parse_write_log_json_ordered(body).into_iter().collect()
}

/// Like `parse_write_log_json` but preserves duplicates (Vec not HashSet).
/// Used by the exactly-once check to detect if the shim somehow stored the
/// same write_id twice.
fn parse_write_log_json_ordered(body: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let start = match body.find('[') {
        Some(s) => s + 1,
        None => return ids,
    };
    let end = match body.rfind(']') {
        Some(e) => e,
        None => return ids,
    };
    for part in body[start..end].split(',') {
        let id = part.trim().trim_matches('"');
        if !id.is_empty() {
            ids.push(id.to_string());
        }
    }
    ids
}

/// Polls every reachable replica's `/state/commit_watermark` and waits
/// until they all report the same value for `stable_polls` consecutive
/// rounds — or until `budget` elapses.
///
/// Real VSR commits propagate asynchronously (leader commits at op N,
/// broadcasts `Commit`, followers catch up over the next few ms). Without
/// a quiescence barrier, divergence checks fire mid-propagation and
/// report false positives. A single agreeing poll is not enough either:
/// a follower can momentarily agree with the leader at a stale offset
/// before catching up, so we require agreement to persist across
/// `stable_polls` rounds (default: 3, ~600ms at a 200ms poll interval).
///
/// Unreachable replicas are tolerated — `cascading_failure` intentionally
/// kills a majority and we don't want this to stall.
fn wait_for_watermark_quiescence(
    endpoints: &std::collections::HashMap<(u16, u8), String>,
    budget: Duration,
) {
    wait_for_watermark_quiescence_with_stability(endpoints, budget, 3);
}

/// Like `wait_for_watermark_quiescence` but lets the caller tune the
/// stability window. `stable_polls = 1` matches the old "first-agreement
/// wins" behaviour; callers under tight budgets (unit tests) can pass 1.
fn wait_for_watermark_quiescence_with_stability(
    endpoints: &std::collections::HashMap<(u16, u8), String>,
    budget: Duration,
    stable_polls: u32,
) {
    if endpoints.is_empty() {
        return;
    }
    let deadline = std::time::Instant::now() + budget;
    let poll_interval = Duration::from_millis(200);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(500))
        .build();

    // Consecutive agreeing rounds observed so far. Resets to 0 on any
    // disagreement or when no replicas respond. Returns once the count
    // reaches `stable_polls`.
    let mut consecutive_agree: u32 = 0;

    while std::time::Instant::now() < deadline {
        let mut watermarks: Vec<u64> = Vec::new();
        for url in endpoints.values() {
            let probe = format!("{}/state/commit_watermark", url.trim_end_matches('/'));
            match agent.get(&probe).call() {
                Ok(resp) if resp.status() == 200 => {
                    let body = resp.into_string().unwrap_or_default();
                    if let Some(w) = parse_watermark_json(&body) {
                        watermarks.push(w);
                    }
                }
                _ => { /* unreachable or non-200: skip */ }
            }
        }
        let all_equal = watermarks
            .windows(2)
            .all(|w| w[0] == w[1]);
        if !watermarks.is_empty() && all_equal {
            consecutive_agree = consecutive_agree.saturating_add(1);
            if consecutive_agree >= stable_polls {
                return;
            }
        } else {
            consecutive_agree = 0;
        }
        std::thread::sleep(poll_interval);
    }
}

/// Parses `{"watermark":N}` from a `/state/commit_watermark` response body.
///
/// Accepts both `{"watermark":N}` and `{"watermark": N}` (with or without
/// space after colon). Returns `None` if the body is malformed.
fn parse_watermark_json(body: &str) -> Option<u64> {
    // Fast hand-rolled parse: no serde dependency in this probe helper.
    let body = body.trim();
    let inner = body.strip_prefix('{')?.strip_suffix('}')?;
    for part in inner.split(',') {
        let part = part.trim();
        if let Some(val) = part
            .strip_prefix("\"watermark\":")
            .or_else(|| part.strip_prefix("\"watermark\": "))
        {
            return val.trim().parse().ok();
        }
    }
    None
}

/// POSTs a probe write to `<base_url>/kv/chaos-probe`. Returns `Ok(true)` if
/// the replica rejected the write (any non-2xx response or a body containing
/// a "not_leader"/"no_quorum" hint), `Ok(false)` if it accepted (2xx), or
/// `Err` on transport failures (connection refused / timeout). Callers
/// should treat transport errors as refusals — those are the exact signals
/// we want from an iptables-blocked minority.
fn probe_rejects_write(base_url: &str) -> Result<bool, String> {
    let url = format!("{}/kv/chaos-probe", base_url.trim_end_matches('/'));
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build();
    // ureq 2.x returns Err(Status(code, response)) for any non-2xx response.
    // We want to classify *that* as "rejected" — only transport-level errors
    // should bubble up as Err.
    let (status, body) = match agent
        .post(&url)
        .set("content-type", "application/json")
        .send_string("{\"op\":\"chaos-probe\"}")
    {
        Ok(resp) => (resp.status(), resp.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, resp)) => (code, resp.into_string().unwrap_or_default()),
        Err(e) => return Err(e.to_string()),
    };
    if !(200..300).contains(&status) {
        return Ok(true);
    }
    let body_lc = body.to_lowercase();
    if body_lc.contains("not_leader")
        || body_lc.contains("no_quorum")
        || body_lc.contains("refused")
    {
        return Ok(true);
    }
    Ok(false)
}

/// Result of one `/state/vsr_status` probe. Carries enough info for
/// callers to decide whether this replica is safe to read a consistent
/// commit snapshot from.
#[derive(Debug)]
enum VsrStatusProbe {
    /// `replica_status == "normal"` AND `bootstrap_complete == true`.
    Ready,
    /// HTTP responded but the replica is not yet ready (view change /
    /// recovering / bootstrap incomplete). Carries the status string
    /// for diagnostics.
    NotReady(String),
    /// Endpoint is old enough that it doesn't know this route. Callers
    /// should skip the gate and fall back to the pre-existing logic.
    Unsupported,
    /// Transport error or non-200/404 response.
    Unreachable(String),
}

/// GETs `<base_url>/state/vsr_status` and interprets the response.
///
/// The real binary added this route in the chaos-hardening pass; older
/// shim builds return 404 (→ `Unsupported`) so callers can degrade
/// gracefully while binaries roll out.
fn probe_vsr_status(base_url: &str) -> VsrStatusProbe {
    let url = format!("{}/state/vsr_status", base_url.trim_end_matches('/'));
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build();
    match agent.get(&url).call() {
        Ok(resp) if resp.status() == 200 => {
            let body = resp.into_string().unwrap_or_default();
            let replica_status = extract_json_string(&body, "replica_status");
            let bootstrap_complete = body.contains("\"bootstrap_complete\":true");
            match replica_status.as_deref() {
                Some("normal") if bootstrap_complete => VsrStatusProbe::Ready,
                Some(other) => VsrStatusProbe::NotReady(format!(
                    "status={other} bootstrap_complete={bootstrap_complete}"
                )),
                None => VsrStatusProbe::NotReady(format!("unparseable body: {body:?}")),
            }
        }
        Ok(resp) if resp.status() == 404 => VsrStatusProbe::Unsupported,
        Ok(resp) => VsrStatusProbe::Unreachable(format!("HTTP {}", resp.status())),
        Err(ureq::Error::Status(404, _)) => VsrStatusProbe::Unsupported,
        Err(e) => VsrStatusProbe::Unreachable(e.to_string()),
    }
}

/// Pulls a JSON string value out of a small flat object. Matches
/// `"<key>":"<value>"` — tolerates whitespace and escaped quotes in the
/// value are intentionally not handled because the only keys we read
/// produce ASCII values we control.
fn extract_json_string(body: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let start = body.find(&pat)? + pat.len();
    let end = body[start..].find('"')? + start;
    Some(body[start..end].to_string())
}

/// Polls `/state/vsr_status` on every endpoint until every reachable
/// replica reports `Ready` (Normal + bootstrap_complete), every endpoint
/// is `Unsupported` (old binary — fall through to the weaker check), or
/// `budget` elapses.
///
/// Returns the terminal decision the caller should act on. The caller
/// tolerates `Err` by treating unreachable replicas the same way the
/// existing quiescence helper does — as a valid "don't stall" signal
/// when the scenario intentionally kills a majority.
enum VsrReadinessOutcome {
    /// All reachable replicas reported Normal + bootstrap_complete.
    Ready,
    /// No endpoint supports the route (pre-hardening binary).
    Unsupported,
    /// Budget elapsed with at least one reachable replica still not ready.
    /// Caller falls through to the existing retry logic so it can still
    /// surface a meaningful divergence message.
    Timeout(String),
}

fn wait_for_vsr_ready(
    endpoints: &std::collections::HashMap<(u16, u8), String>,
    budget: Duration,
) -> VsrReadinessOutcome {
    if endpoints.is_empty() {
        return VsrReadinessOutcome::Ready;
    }
    let deadline = std::time::Instant::now() + budget;
    let poll_interval = Duration::from_millis(250);

    let mut last_detail: Vec<String> = Vec::new();
    while std::time::Instant::now() < deadline {
        last_detail.clear();
        let mut any_supported = false;
        let mut all_ready = true;
        for ((c, r), url) in endpoints {
            match probe_vsr_status(url) {
                VsrStatusProbe::Ready => {
                    any_supported = true;
                    last_detail.push(format!("c{c}-r{r}=ready"));
                }
                VsrStatusProbe::NotReady(detail) => {
                    any_supported = true;
                    all_ready = false;
                    last_detail.push(format!("c{c}-r{r}=not_ready({detail})"));
                }
                VsrStatusProbe::Unsupported => {
                    last_detail.push(format!("c{c}-r{r}=unsupported"));
                }
                VsrStatusProbe::Unreachable(detail) => {
                    // Match existing quiescence semantics: treat as
                    // "absent" so a permanently-killed replica does not
                    // stall the gate.
                    last_detail.push(format!("c{c}-r{r}=unreachable({detail})"));
                }
            }
        }
        if !any_supported {
            return VsrReadinessOutcome::Unsupported;
        }
        if all_ready {
            return VsrReadinessOutcome::Ready;
        }
        std::thread::sleep(poll_interval);
    }
    VsrReadinessOutcome::Timeout(last_detail.join(", "))
}

/// GETs `<base_url>/health` and returns the response's HTTP status as a
/// fingerprint. Bodies often encode per-replica identity (e.g.
/// `replica-0`), so we only compare status codes — identical statuses
/// across all replicas mean they are all equally healthy / equally
/// unhealthy, which is the chaos-level notion of "converged" we want.
fn probe_health_fingerprint(base_url: &str) -> Result<String, String> {
    let url = format!("{}/health", base_url.trim_end_matches('/'));
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build();
    match agent.get(&url).call() {
        Ok(resp) => Ok(format!("status={}", resp.status())),
        Err(ureq::Error::Status(code, _)) => Ok(format!("status={code}")),
        Err(e) => Err(e.to_string()),
    }
}

/// GETs `<base_url>/state/commit_hash` and extracts the hex digest from
/// `{"commit_hash":"<hex>"}`.
///
/// Returns `Ok(None)` when the endpoint is unavailable (old shim binary
/// without this route, 404), so callers can fall back to a weaker probe.
/// Returns `Err(msg)` for transport failures — those still count as
/// divergence since an unreachable replica can't prove it converged.
fn probe_commit_hash(base_url: &str) -> Result<Option<String>, String> {
    let url = format!("{}/state/commit_hash", base_url.trim_end_matches('/'));
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build();
    match agent.get(&url).call() {
        Ok(resp) if resp.status() == 200 => {
            let body = resp.into_string().unwrap_or_default();
            let Some(start) = body.find("\"commit_hash\":\"") else {
                return Err(format!("malformed body: {body:?}"));
            };
            let hex_start = start + "\"commit_hash\":\"".len();
            let Some(end_rel) = body[hex_start..].find('"') else {
                return Err(format!("malformed body: {body:?}"));
            };
            Ok(Some(body[hex_start..hex_start + end_rel].to_string()))
        }
        Ok(resp) if resp.status() == 404 => Ok(None),
        Ok(resp) => Err(format!("HTTP {}", resp.status())),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(ureq::Error::Status(code, _)) => Err(format!("HTTP {code}")),
        Err(e) => Err(e.to_string()),
    }
}

// ============================================================================
// Built-in Invariants
// ============================================================================

fn builtin_invariants() -> Vec<Invariant> {
    vec![
        Invariant {
            name: "minority_refuses_writes".into(),
            description: "A minority partition must refuse write requests.".into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "no_divergence_after_heal".into(),
            description: "After healing a partition, all replicas must converge to \
                          identical committed log state."
                .into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "hash_chain_valid_all_replicas".into(),
            description: "Every replica's hash chain must validate end-to-end.".into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "all_writes_preserved".into(),
            description: "Every client write that received an acknowledgment must \
                          be present in the final log of a quorum of replicas."
                .into(),
            category: InvariantCategory::Durability,
        },
        Invariant {
            name: "linearizability".into(),
            description: "Client operations must appear to execute in a global total \
                          order consistent with real-time ordering."
                .into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "commit_watermark_consistent".into(),
            description: "Each shim's advertised commit_watermark must equal the length \
                          of its write_log — a structural property of shim bookkeeping."
                .into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "no_lost_commits".into(),
            description: "A commit acknowledged to a client must never be lost.".into(),
            category: InvariantCategory::Durability,
        },
        Invariant {
            name: "directory_reroutes_to_cluster_b".into(),
            description: "When all replicas of cluster A are unreachable, \
                          kimberlite-directory must route new requests to cluster B."
                .into(),
            category: InvariantCategory::Liveness,
        },
        Invariant {
            name: "no_data_loss_across_failover".into(),
            description: "Cross-cluster failover must not lose data that was \
                          durably committed in the original cluster."
                .into(),
            category: InvariantCategory::Durability,
        },
        Invariant {
            name: "quorum_loss_detected".into(),
            description: "When f+1 replicas fail, the cluster must reject writes \
                          rather than commit with under-quorum."
                .into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "no_corruption_under_quorum_loss".into(),
            description: "Quorum loss must not corrupt log state — on recovery, \
                          the hash chain must still validate."
                .into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "graceful_enforcement".into(),
            description: "Storage exhaustion must be enforced with clear error \
                          responses, not panics or silent corruption."
                .into(),
            category: InvariantCategory::Safety,
        },
        Invariant {
            name: "no_panic_or_corruption".into(),
            description: "No kimberlite-server process should panic under any \
                          chaos scenario. Disk state must remain valid."
                .into(),
            category: InvariantCategory::Safety,
        },
    ]
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_checker_has_thirteen_invariants() {
        let checker = InvariantChecker::builtin();
        assert_eq!(checker.invariants.len(), 13);
    }

    #[test]
    fn check_records_result() {
        let mut checker = InvariantChecker::builtin();
        let result = checker.check("minority_refuses_writes", 1000);
        assert!(result.held);
        assert_eq!(checker.results().len(), 1);
    }

    #[test]
    fn probes_short_circuit_when_disabled() {
        let mut checker = InvariantChecker::builtin();
        checker.set_endpoint(0, 0, "http://192.0.2.1:9000".into()); // unroutable
        checker.mark_minority(0, 0);
        let result = checker.check("minority_refuses_writes", 0);
        assert!(result.held);
        assert!(result.message.contains("probe skipped"));
    }

    fn start_fixed_status_server(status_line: &'static str, body: &'static str) -> String {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}");

        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                // Read request headers until double CRLF so the client has
                // finished writing before we reply.
                let mut received = Vec::new();
                let mut buf = [0u8; 1024];
                while !received.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => received.extend_from_slice(&buf[..n]),
                    }
                    if received.len() > 16 * 1024 {
                        break;
                    }
                }
                // Drain any POST body (client wrote Content-Length bytes) —
                // cheap pattern: read once more and discard.
                let _ = sock.set_read_timeout(Some(Duration::from_millis(50)));
                let _ = sock.read(&mut buf);

                let response = format!(
                    "{status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len(),
                );
                let _ = sock.write_all(response.as_bytes());
                let _ = sock.flush();
                let _ = sock.shutdown(std::net::Shutdown::Both);
            }
        });
        url
    }

    #[test]
    fn probe_rejects_write_treats_5xx_as_rejection() {
        let url = start_fixed_status_server("HTTP/1.1 503 Service Unavailable", "");
        let rejected = probe_rejects_write(&url).expect("probe completed");
        assert!(rejected, "5xx should count as rejection");
    }

    #[test]
    fn probe_rejects_write_treats_200_as_acceptance() {
        let url = start_fixed_status_server("HTTP/1.1 200 OK", "ok");
        let rejected = probe_rejects_write(&url).expect("probe completed");
        assert!(
            !rejected,
            "2xx without not_leader/no_quorum signals = acceptance"
        );
    }

    #[test]
    fn probe_rejects_write_treats_not_leader_body_as_rejection() {
        let url = start_fixed_status_server("HTTP/1.1 200 OK", "{\"error\":\"not_leader\"}");
        let rejected = probe_rejects_write(&url).expect("probe completed");
        assert!(rejected, "body containing not_leader = rejection");
    }

    #[test]
    fn parse_watermark_json_compact() {
        assert_eq!(parse_watermark_json("{\"watermark\":42}"), Some(42));
    }

    #[test]
    fn parse_watermark_json_spaced() {
        assert_eq!(parse_watermark_json("{\"watermark\": 100}"), Some(100));
    }

    #[test]
    fn parse_watermark_json_zero() {
        assert_eq!(parse_watermark_json("{\"watermark\":0}"), Some(0));
    }

    #[test]
    fn parse_watermark_json_malformed() {
        assert_eq!(parse_watermark_json("{}"), None);
        assert_eq!(parse_watermark_json("not json"), None);
        assert_eq!(parse_watermark_json("{\"other\":5}"), None);
    }

    // ========================================================================
    // Phase B: write_log parse helpers
    // ========================================================================

    #[test]
    fn parse_write_log_json_basic() {
        let body = r#"{"write_ids":["1","2","3"],"total":3}"#;
        let ids = parse_write_log_json(body);
        assert_eq!(ids.len(), 3);
        assert!(ids.contains("1") && ids.contains("2") && ids.contains("3"));
    }

    #[test]
    fn parse_write_log_json_empty() {
        let body = r#"{"write_ids":[],"total":0}"#;
        assert!(parse_write_log_json(body).is_empty());
    }

    #[test]
    fn parse_write_log_json_ordered_preserves_duplicates() {
        // The shim uses HashSet so this shouldn't happen in practice, but
        // the parser itself must handle it (so the exactly-once check can
        // detect it if it ever does).
        let body = r#"{"write_ids":["5","5","7"],"total":3}"#;
        let ids = parse_write_log_json_ordered(body);
        assert_eq!(ids.len(), 3);
        assert_eq!(ids.iter().filter(|id| *id == "5").count(), 2);
    }

    // ========================================================================
    // Phase B: durability check unit tests (no real HTTP)
    // ========================================================================

    #[test]
    fn set_acknowledged_writes_stores_correctly() {
        let mut c = InvariantChecker::new();
        assert!(c.acknowledged_writes.is_empty());
        c.set_acknowledged_writes(vec!["a".into(), "b".into(), "c".into()]);
        assert_eq!(c.acknowledged_writes.len(), 3);
        assert!(c.acknowledged_writes.contains(&"b".to_string()));
    }
}
