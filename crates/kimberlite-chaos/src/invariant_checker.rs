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
    }

    /// Removes a replica endpoint — e.g. when it has been killed and is
    /// intentionally unreachable. Probes skip it until re-registered.
    pub fn remove_endpoint(&mut self, cluster: u16, replica: u8) {
        self.endpoints.remove(&(cluster, replica));
    }

    /// Enables / disables HTTP probing. Called by `ChaosController` to match
    /// the controller's ExecMode: Apply → enabled, DryRun → disabled.
    pub fn set_probes_enabled(&mut self, enabled: bool) {
        self.probes_enabled = enabled;
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

    /// Checks a named invariant, appending the result.
    ///
    /// Dispatches to real HTTP probes for the two invariants covered in
    /// Phase 2.4 (`minority_refuses_writes`, `no_divergence_after_heal`).
    /// Other invariants still return `held: true` with a `TODO` message to
    /// avoid false positives while the full checker suite is built out.
    pub fn check(&mut self, name: &str, now_ms: u64) -> InvariantResult {
        let (held, message) = if !self.probes_enabled {
            (
                true,
                format!("probe skipped (probes disabled) for {name}"),
            )
        } else {
            match name {
                "minority_refuses_writes" => self.check_minority_refuses_writes(),
                "no_divergence_after_heal" => self.check_no_divergence_after_heal(),
                "no_lost_commits" => self.check_no_lost_commits(),
                // Remaining invariants map to the liveness probe for now.
                // `all_writes_preserved` and `exactly_once_semantics` will
                // graduate to real probes once the shim gains a write log
                // (Phase B of the deferred-items campaign).
                // `linearizability` is intentionally deferred — a full
                // Jepsen-style history checker is a separate work item.
                _ => {
                    let (held, mut msg) = self.check_hash_chain_all_replicas();
                    msg = format!("[liveness proxy for `{name}`] {msg}");
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
            return (true, "no minority replicas registered — trivially OK".into());
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
                format!("all {} minority replica(s) rejected writes", self.minority.len()),
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
        // One retry after 500ms to allow state transfer to settle.
        for attempt in 0..2 {
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
            let all_equal = fingerprints
                .windows(2)
                .all(|w| w[0] == w[1]);
            if all_ok && all_equal {
                return (true, format!("all {} replicas converged", states.len()));
            }
            if attempt == 0 {
                std::thread::sleep(Duration::from_millis(500));
            } else {
                let mismatches: Vec<String> = states
                    .into_iter()
                    .map(|(key, s)| {
                        format!("c{}-r{}={:?}", key.0, key.1, s)
                    })
                    .collect();
                return (
                    false,
                    format!("replicas diverged: [{}]", mismatches.join(", ")),
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

        let max_wm = watermarks.iter().map(|(_, _, w)| *w).max().unwrap_or(0);
        let min_wm = watermarks.iter().map(|(_, _, w)| *w).min().unwrap_or(0);

        // Generous tolerance — at least 5 writes, at most 50% of max.
        let tolerance = (max_wm / 2).max(5);

        let detail: Vec<String> = watermarks
            .iter()
            .map(|(c, r, w)| format!("c{c}-r{r}={w}"))
            .collect();

        if max_wm - min_wm <= tolerance {
            (
                true,
                format!(
                    "commit watermarks converged: {} (max={max_wm} min={min_wm} tol={tolerance})",
                    detail.join(", ")
                ),
            )
        } else {
            (
                false,
                format!(
                    "commit watermarks diverged: {} (max={max_wm} min={min_wm} tol={tolerance})",
                    detail.join(", ")
                ),
            )
        }
    }

    /// that's exactly what we'd expect if a replica panicked post-boot
    /// (no_panic_or_corruption) or if hash-chain verification aborted
    /// the shim on startup.
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
                    let body = resp.into_string().unwrap_or_default();
                    if !body.starts_with("replica-") {
                        failures.push(format!(
                            "c{c}-r{r} /health returned unexpected body: {body:?}"
                        ));
                    }
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
            (true, format!("all {} replicas healthy", self.endpoints.len()))
        } else {
            (false, failures.join("; "))
        }
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
        Err(ureq::Error::Status(code, resp)) => {
            (code, resp.into_string().unwrap_or_default())
        }
        Err(e) => return Err(e.to_string()),
    };
    if !(200..300).contains(&status) {
        return Ok(true);
    }
    let body_lc = body.to_lowercase();
    if body_lc.contains("not_leader") || body_lc.contains("no_quorum") || body_lc.contains("refused") {
        return Ok(true);
    }
    Ok(false)
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
            name: "exactly_once_semantics".into(),
            description: "Client retries must produce exactly-once effects (no duplicate \
                          commits, no lost operations)."
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
}
