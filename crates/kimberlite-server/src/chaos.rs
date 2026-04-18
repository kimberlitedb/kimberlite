//! Chaos HTTP probe surface for fault-injection testing.
//!
//! The `kimberlite-chaos` harness drives VSR clusters under adversarial
//! conditions (partitions, kills, disk-fills) and polls a fixed HTTP
//! contract on every replica to verify consensus invariants. The full
//! contract is:
//!
//! - `POST /kv/chaos-probe`         — submit a write (body `{"write_id":"<s>"}`).
//!                                    200 on commit, 503 on lost quorum,
//!                                    4xx on not-leader / refused.
//! - `GET  /state/commit_watermark` — `{"watermark":N}`, the chaos stream's
//!                                    committed offset on this replica.
//! - `GET  /state/write_log`        — `{"write_ids":[...],"total":N}`,
//!                                    in commit order.
//! - `GET  /state/commit_hash`      — `{"commit_hash":"<16-hex>"}`, an
//!                                    ordering-independent fingerprint of
//!                                    the write-id set. Replicas with the
//!                                    same committed set produce the same
//!                                    hash.
//!
//! # Architecture
//!
//! Two threads sit behind the handle:
//!
//! 1. **apply observer** — subscribes to [`kimberlite_vsr::AppliedCommit`]
//!    events from the VSR event loop. Every commit on this replica (leader
//!    AND follower) fires a fanout; the observer updates the shared
//!    snapshot. This bypasses the `Kimberlite` projection, which is only
//!    kept in sync on the leader today.
//! 2. **job worker** — single thread, consumes [`ChaosJob`]s from the HTTP
//!    sidecar. POST probes submit an `AppendBatch` to the chaos stream via
//!    [`CommandSubmitter::submit_with_timeout`] and map VSR errors to
//!    probe-contract responses.
//!
//! Read endpoints (`/state/*`) dispatch directly off the `Arc<RwLock<_>>`
//! snapshot without hitting the worker — they stay responsive even while a
//! probe commit is in flight.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use bytes::Bytes;
use kimberlite_kernel::{Command, Effect};
use kimberlite_types::{DataClass, Offset, Placement, StreamId, StreamName};
use kimberlite_vsr::AppliedCommit;
use tracing::{info, warn};

use crate::error::ServerError;
use crate::replication::CommandSubmitter;

/// Reserved stream name for chaos probes. Leading underscore marks it as
/// system-reserved so user code has no reason to collide.
pub const CHAOS_STREAM_NAME: &str = "_chaos_probe";

/// File name (under the server's `data_dir`) holding the persistent
/// chaos write log. One write_id per line, appended + fsync'd on each
/// new observation so a VM restart doesn't lose durable state.
const CHAOS_LOG_FILENAME: &str = "chaos-write-log";

/// Wall-clock budget for a single chaos probe. Long enough to cover a
/// view-change round-trip (worst-case ~3s for 3-node localhost), short
/// enough that a lost-quorum probe fails fast.
const CHAOS_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Shared, read-mostly snapshot of the chaos write log on this replica.
///
/// Updated only by the apply-observer thread. Read concurrently by the
/// HTTP sidecar for `/state/*` responses.
#[derive(Debug, Default, Clone)]
pub struct ChaosSnapshot {
    /// Write IDs in commit order (insertion-sorted by the observer).
    pub write_ids: Vec<String>,
    /// Chaos stream's committed offset. Advances monotonically.
    pub watermark: u64,
    /// FNV-1a over sorted write IDs. Cached so `/state/commit_hash` is O(1).
    pub commit_hash: String,
    /// Chaos stream ID once we've observed its `StreamMetadataWrite`.
    /// `None` until the first `CreateStream` commits.
    pub stream_id: Option<StreamId>,
}

/// Result of a `POST /kv/chaos-probe` request.
#[derive(Debug, Clone)]
pub enum ProbeResult {
    /// Commit succeeded; the write is durable.
    Ok,
    /// No quorum (timeout) — translates to HTTP 503.
    NoQuorum(String),
    /// Not the leader — translates to HTTP 421 with a leader hint.
    NotLeader {
        view: u64,
        leader_hint: Option<String>,
    },
    /// Any other error — translates to HTTP 500.
    InternalError(String),
}

/// Work delivered from the HTTP sidecar to the chaos worker thread.
#[derive(Debug)]
pub enum ChaosJob {
    Probe {
        write_id: Option<String>,
        respond: SyncSender<ProbeResult>,
    },
}

/// Handle the HTTP sidecar holds onto. Cheaply cloneable.
#[derive(Clone)]
pub struct ChaosHandle {
    snapshot: Arc<RwLock<ChaosSnapshot>>,
    job_tx: SyncSender<ChaosJob>,
}

impl ChaosHandle {
    /// Spawns the apply-observer and job-worker threads. Returns a handle
    /// that the HTTP sidecar uses to dispatch chaos requests.
    ///
    /// `data_dir` points to the server's persistent directory; the chaos
    /// log file lives at `<data_dir>/chaos-write-log` so observer state
    /// (write_ids, watermark, commit_hash) survives a VM restart. VSR
    /// doesn't replay its log on boot, and the chaos observer can't
    /// rebuild its state from the kernel_state snapshot (stream metadata
    /// only — event payloads are elsewhere), so the persistence here is
    /// the only way restart-heavy scenarios (`rolling_restart_under_load`)
    /// can satisfy durability invariants against the real binary.
    pub fn spawn(
        submitter: Arc<CommandSubmitter>,
        applied_rx: Receiver<AppliedCommit>,
        data_dir: PathBuf,
    ) -> Self {
        let snapshot = Arc::new(RwLock::new(ChaosSnapshot::default()));
        let (job_tx, job_rx) = mpsc::sync_channel::<ChaosJob>(256);

        // Best-effort load of any prior run's write log. Errors are
        // logged and ignored — a missing/corrupt file just means we
        // start fresh.
        let log_path = data_dir.join(CHAOS_LOG_FILENAME);
        if let Err(e) = load_persisted_write_log(&log_path, &snapshot) {
            warn!(error = %e, path = %log_path.display(), "failed to load persisted chaos log");
        }

        // Open the log file for append. Shared between the observer
        // thread (writes new entries) and loaders above (read-only at
        // startup). `Mutex<File>` rather than `File::try_clone` keeps
        // the lock explicit — only one fsync at a time anyway.
        let log_file: Arc<Mutex<Option<File>>> = Arc::new(Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .map_err(|e| {
                    warn!(error = %e, path = %log_path.display(), "could not open chaos log for append");
                    e
                })
                .ok(),
        ));

        {
            let snapshot = Arc::clone(&snapshot);
            let log_file = Arc::clone(&log_file);
            thread::Builder::new()
                .name("chaos-apply-observer".into())
                .spawn(move || run_apply_observer(applied_rx, snapshot, log_file))
                .expect("spawn chaos-apply-observer");
        }

        {
            let submitter = Arc::clone(&submitter);
            let snapshot = Arc::clone(&snapshot);
            thread::Builder::new()
                .name("chaos-worker".into())
                .spawn(move || run_worker(job_rx, submitter, snapshot))
                .expect("spawn chaos-worker");
        }

        Self { snapshot, job_tx }
    }

    /// Returns the current write-log snapshot. Cloned so callers can
    /// render JSON without holding the RwLock.
    pub fn snapshot(&self) -> ChaosSnapshot {
        self.snapshot
            .read()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Enqueues a probe job. Blocks up to 100ms on backpressure; on
    /// channel-full returns `Err`.
    pub fn submit_probe(
        &self,
        write_id: Option<String>,
    ) -> Result<Receiver<ProbeResult>, mpsc::TrySendError<ChaosJob>> {
        let (respond, rx) = mpsc::sync_channel(1);
        self.job_tx.try_send(ChaosJob::Probe { write_id, respond })?;
        Ok(rx)
    }
}

fn run_apply_observer(
    rx: Receiver<AppliedCommit>,
    snapshot: Arc<RwLock<ChaosSnapshot>>,
    log_file: Arc<Mutex<Option<File>>>,
) {
    // Seed the dedup set from any write_ids already loaded off disk.
    // Otherwise a commit that happens to replay a previously-persisted
    // write_id (e.g. via VSR log replay after reboot — not implemented
    // today, but may land) would double-append.
    let mut seen: HashSet<String> = snapshot
        .read()
        .map(|s| s.write_ids.iter().cloned().collect())
        .unwrap_or_default();
    while let Ok(commit) = rx.recv() {
        let mut new_ids: Vec<String> = Vec::new();
        let mut new_watermark: Option<u64> = None;
        let mut learned_stream_id: Option<StreamId> = None;

        let current_stream_id = snapshot
            .read()
            .ok()
            .and_then(|s| s.stream_id);

        for effect in &commit.effects {
            match effect {
                Effect::StreamMetadataWrite(meta)
                    if meta.stream_name.as_str() == CHAOS_STREAM_NAME =>
                {
                    learned_stream_id = Some(meta.stream_id);
                    info!(
                        stream_id = %meta.stream_id,
                        op = %commit.op,
                        "chaos stream registered via apply observer",
                    );
                }
                Effect::StorageAppend {
                    stream_id,
                    base_offset,
                    events,
                } if current_stream_id == Some(*stream_id)
                    || learned_stream_id == Some(*stream_id) =>
                {
                    for (i, event) in events.iter().enumerate() {
                        let write_id = match std::str::from_utf8(event.as_ref()) {
                            Ok(s) if !s.is_empty() => s.to_string(),
                            _ => continue,
                        };
                        if seen.insert(write_id.clone()) {
                            new_ids.push(write_id);
                        }
                        // Watermark = last committed offset + 1.
                        new_watermark = Some(base_offset.as_u64() + i as u64 + 1);
                    }
                }
                _ => {}
            }
        }

        if learned_stream_id.is_some() || !new_ids.is_empty() || new_watermark.is_some() {
            // Persist new write_ids before updating the in-memory
            // snapshot so a crash between the two never leaves
            // `/state/write_log` reporting a write_id that isn't yet
            // durable on disk. Fsync on each append: low volume (one
            // probe every 10-20ms under chaos workload) and the
            // durability guarantee is load-bearing for the invariant
            // checker.
            if !new_ids.is_empty() {
                if let Err(e) = append_write_ids(&log_file, &new_ids) {
                    warn!(error = %e, "failed to append chaos write-ids to disk");
                }
            }

            match snapshot.write() {
                Ok(mut state) => {
                    if let Some(id) = learned_stream_id {
                        state.stream_id = Some(id);
                    }
                    let appended = new_ids.len();
                    for id in new_ids {
                        state.write_ids.push(id);
                    }
                    if let Some(w) = new_watermark {
                        state.watermark = w;
                    }
                    state.commit_hash = compute_commit_hash(&state.write_ids);
                    info!(
                        op = %commit.op,
                        watermark = state.watermark,
                        total = state.write_ids.len(),
                        appended,
                        "chaos snapshot updated",
                    );
                }
                Err(e) => warn!(error = %e, "chaos snapshot lock poisoned"),
            }
        }
    }

    info!("chaos apply-observer shutting down (channel closed)");
}

fn run_worker(
    rx: Receiver<ChaosJob>,
    submitter: Arc<CommandSubmitter>,
    snapshot: Arc<RwLock<ChaosSnapshot>>,
) {
    while let Ok(job) = rx.recv() {
        match job {
            ChaosJob::Probe { write_id, respond } => {
                let result = handle_probe(&submitter, &snapshot, write_id);
                let _ = respond.send(result);
            }
        }
    }
    info!("chaos job-worker shutting down (channel closed)");
}

fn handle_probe(
    submitter: &Arc<CommandSubmitter>,
    snapshot: &Arc<RwLock<ChaosSnapshot>>,
    write_id: Option<String>,
) -> ProbeResult {
    let stream_id = match ensure_chaos_stream(submitter, snapshot) {
        Ok(id) => id,
        Err(r) => return r,
    };

    let next_offset = submitter
        .kernel_state_snapshot(Duration::from_secs(2))
        .ok()
        .and_then(|s| s.get_stream(&stream_id).map(|m| m.current_offset))
        .unwrap_or(Offset::ZERO);

    let payload = write_id.as_deref().unwrap_or("").as_bytes().to_vec();
    let cmd = Command::append_batch(stream_id, vec![Bytes::from(payload)], next_offset);

    match submitter.submit_with_timeout(cmd, CHAOS_PROBE_TIMEOUT) {
        Ok(_) => ProbeResult::Ok,
        Err(ServerError::NotLeader { view, leader_hint }) => ProbeResult::NotLeader {
            view,
            leader_hint: leader_hint.map(|a| a.to_string()),
        },
        Err(ServerError::CommitTimeout { timeout_ms }) => {
            ProbeResult::NoQuorum(format!("commit timed out after {timeout_ms}ms"))
        }
        Err(ServerError::ServerBusy) => {
            ProbeResult::NoQuorum("server busy (backpressure)".to_string())
        }
        Err(e) => ProbeResult::InternalError(e.to_string()),
    }
}

/// Ensures the chaos stream exists and returns its `StreamId`. Caches the
/// result in the snapshot so subsequent probes skip the lookup.
///
/// On follower replicas the create path returns `NotLeader`, which bubbles
/// up as the probe's `NotLeader` response — the desired chaos semantics.
fn ensure_chaos_stream(
    submitter: &Arc<CommandSubmitter>,
    snapshot: &Arc<RwLock<ChaosSnapshot>>,
) -> Result<StreamId, ProbeResult> {
    if let Ok(s) = snapshot.read() {
        if let Some(id) = s.stream_id {
            return Ok(id);
        }
    }

    let state = submitter
        .kernel_state_snapshot(Duration::from_secs(2))
        .map_err(|e| ProbeResult::InternalError(e.to_string()))?;

    if let Some(meta) = state
        .streams()
        .values()
        .find(|m| m.stream_name.as_str() == CHAOS_STREAM_NAME)
    {
        if let Ok(mut s) = snapshot.write() {
            s.stream_id = Some(meta.stream_id);
        }
        return Ok(meta.stream_id);
    }

    // Stream missing — try to create. Only leader succeeds.
    let cmd = Command::create_stream_with_auto_id(
        StreamName::new(CHAOS_STREAM_NAME),
        DataClass::Public,
        Placement::Global,
    );
    match submitter.submit_with_timeout(cmd, CHAOS_PROBE_TIMEOUT) {
        Ok(_) => {}
        Err(ServerError::NotLeader { view, leader_hint }) => {
            return Err(ProbeResult::NotLeader {
                view,
                leader_hint: leader_hint.map(|a| a.to_string()),
            });
        }
        Err(ServerError::CommitTimeout { timeout_ms }) => {
            return Err(ProbeResult::NoQuorum(format!(
                "create chaos stream: commit timed out after {timeout_ms}ms"
            )));
        }
        Err(e) => return Err(ProbeResult::InternalError(e.to_string())),
    }

    // Re-snapshot to learn the assigned StreamId. The apply-observer will
    // also populate it shortly, but re-reading kernel_state is deterministic.
    let state = submitter
        .kernel_state_snapshot(Duration::from_secs(2))
        .map_err(|e| ProbeResult::InternalError(e.to_string()))?;

    let meta = state
        .streams()
        .values()
        .find(|m| m.stream_name.as_str() == CHAOS_STREAM_NAME)
        .ok_or_else(|| {
            ProbeResult::InternalError(
                "chaos stream vanished after create committed".into(),
            )
        })?;

    if let Ok(mut s) = snapshot.write() {
        s.stream_id = Some(meta.stream_id);
    }
    Ok(meta.stream_id)
}

/// Reads any previously-persisted write-log into the snapshot. Returns
/// `Ok(())` if the file doesn't exist (fresh install). Any parse error
/// stops the load and leaves the snapshot in whatever state it reached —
/// partial recovery is better than crashing on boot.
fn load_persisted_write_log(
    path: &Path,
    snapshot: &Arc<RwLock<ChaosSnapshot>>,
) -> std::io::Result<()> {
    let file = match OpenOptions::new().read(true).open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let reader = BufReader::new(file);
    let mut loaded = Vec::new();
    let mut seen_local: HashSet<String> = HashSet::new();
    for line in reader.lines() {
        let id = line?;
        if id.is_empty() {
            continue;
        }
        if seen_local.insert(id.clone()) {
            loaded.push(id);
        }
    }
    let count = loaded.len();
    if let Ok(mut state) = snapshot.write() {
        state.write_ids = loaded;
        state.watermark = count as u64;
        state.commit_hash = compute_commit_hash(&state.write_ids);
    }
    info!(count, path = %path.display(), "loaded persisted chaos write-log");
    Ok(())
}

/// Appends new write-ids to the chaos log file, one per line, and
/// fsyncs so the durability guarantee holds across crash/restart.
fn append_write_ids(
    log_file: &Arc<Mutex<Option<File>>>,
    ids: &[String],
) -> std::io::Result<()> {
    let mut guard = log_file
        .lock()
        .map_err(|_| std::io::Error::other("chaos log mutex poisoned"))?;
    let Some(ref mut file) = *guard else {
        return Err(std::io::Error::other("chaos log file not open"));
    };
    for id in ids {
        writeln!(file, "{id}")?;
    }
    file.sync_all()?;
    Ok(())
}

/// FNV-1a 64-bit over sorted write_ids joined by `'\n'`. Ordering-independent
/// on the SET — two replicas with the same committed write-id set produce
/// the same hash regardless of commit order. Matches the shim's protocol.
fn compute_commit_hash(write_ids: &[String]) -> String {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01b3;

    let mut ids: Vec<&str> = write_ids.iter().map(String::as_str).collect();
    ids.sort_unstable();

    let mut hash: u64 = FNV_OFFSET;
    for id in ids {
        for byte in id.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash ^= u64::from(b'\n');
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use kimberlite_types::{Offset, StreamMetadata, StreamName};
    use kimberlite_vsr::types::OpNumber;
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn commit_hash_stable_empty() {
        assert_eq!(compute_commit_hash(&[]), format!("{:016x}", 0xcbf2_9ce4_8422_2325_u64));
    }

    #[test]
    fn commit_hash_ordering_independent() {
        let a = vec!["alpha".to_string(), "bravo".to_string(), "charlie".to_string()];
        let b = vec!["charlie".to_string(), "alpha".to_string(), "bravo".to_string()];
        assert_eq!(compute_commit_hash(&a), compute_commit_hash(&b));
    }

    #[test]
    fn commit_hash_differs_on_divergent_sets() {
        let a = vec!["w1".to_string(), "w2".to_string()];
        let b = vec!["w1".to_string(), "w3".to_string()];
        assert_ne!(compute_commit_hash(&a), compute_commit_hash(&b));
    }

    fn wait_for<F: Fn(&ChaosSnapshot) -> bool>(snapshot: &Arc<RwLock<ChaosSnapshot>>, f: F) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(s) = snapshot.read() {
                if f(&s) {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("snapshot did not reach expected state in time");
    }

    #[test]
    fn observer_learns_stream_id_from_metadata_write() {
        // Feed the observer a StreamMetadataWrite with the chaos name and
        // verify it caches the StreamId in the snapshot.
        let (tx, rx) = mpsc::sync_channel::<AppliedCommit>(16);
        let snapshot = Arc::new(RwLock::new(ChaosSnapshot::default()));
        let snap_clone = Arc::clone(&snapshot);
        std::thread::spawn(move || run_apply_observer(rx, snap_clone, Arc::new(Mutex::new(None))));

        let meta = StreamMetadata::new(
            StreamId::new(7),
            StreamName::new(CHAOS_STREAM_NAME),
            DataClass::Public,
            Placement::Global,
        );
        tx.send(AppliedCommit {
            op: OpNumber::new(1),
            effects: vec![Effect::StreamMetadataWrite(meta)],
        })
        .unwrap();

        wait_for(&snapshot, |s| s.stream_id == Some(StreamId::new(7)));
        drop(tx);
    }

    #[test]
    fn observer_records_write_ids_from_storage_append() {
        let (tx, rx) = mpsc::sync_channel::<AppliedCommit>(16);
        let snapshot = Arc::new(RwLock::new(ChaosSnapshot::default()));
        let snap_clone = Arc::clone(&snapshot);
        std::thread::spawn(move || run_apply_observer(rx, snap_clone, Arc::new(Mutex::new(None))));

        // First commit registers the chaos stream.
        let meta = StreamMetadata::new(
            StreamId::new(42),
            StreamName::new(CHAOS_STREAM_NAME),
            DataClass::Public,
            Placement::Global,
        );
        tx.send(AppliedCommit {
            op: OpNumber::new(1),
            effects: vec![Effect::StreamMetadataWrite(meta)],
        })
        .unwrap();
        wait_for(&snapshot, |s| s.stream_id.is_some());

        // Subsequent commits append events to the chaos stream. Observer
        // should record each event as a write_id in commit order and
        // update the watermark.
        tx.send(AppliedCommit {
            op: OpNumber::new(2),
            effects: vec![Effect::StorageAppend {
                stream_id: StreamId::new(42),
                base_offset: Offset::new(0),
                events: vec![Bytes::from_static(b"w1"), Bytes::from_static(b"w2")],
            }],
        })
        .unwrap();

        wait_for(&snapshot, |s| s.write_ids.len() == 2 && s.watermark == 2);
        let snap = snapshot.read().unwrap();
        assert_eq!(snap.write_ids, vec!["w1".to_string(), "w2".to_string()]);
        assert_eq!(snap.watermark, 2);

        // Hash is set and stable vs recomputing from the ordered list.
        assert_eq!(snap.commit_hash, compute_commit_hash(&snap.write_ids));
    }

    #[test]
    fn observer_ignores_storage_append_on_unrelated_streams() {
        let (tx, rx) = mpsc::sync_channel::<AppliedCommit>(16);
        let snapshot = Arc::new(RwLock::new(ChaosSnapshot::default()));
        let snap_clone = Arc::clone(&snapshot);
        std::thread::spawn(move || run_apply_observer(rx, snap_clone, Arc::new(Mutex::new(None))));

        // Chaos stream registers at id=10.
        let meta = StreamMetadata::new(
            StreamId::new(10),
            StreamName::new(CHAOS_STREAM_NAME),
            DataClass::Public,
            Placement::Global,
        );
        tx.send(AppliedCommit {
            op: OpNumber::new(1),
            effects: vec![Effect::StreamMetadataWrite(meta)],
        })
        .unwrap();
        wait_for(&snapshot, |s| s.stream_id == Some(StreamId::new(10)));

        // Append to a DIFFERENT stream — observer must ignore.
        tx.send(AppliedCommit {
            op: OpNumber::new(2),
            effects: vec![Effect::StorageAppend {
                stream_id: StreamId::new(99),
                base_offset: Offset::new(0),
                events: vec![Bytes::from_static(b"noise")],
            }],
        })
        .unwrap();

        // Give the observer a moment and confirm it did NOT record anything.
        std::thread::sleep(Duration::from_millis(100));
        let snap = snapshot.read().unwrap();
        assert_eq!(snap.write_ids.len(), 0);
        assert_eq!(snap.watermark, 0);
    }

    #[test]
    fn observer_deduplicates_write_ids() {
        let (tx, rx) = mpsc::sync_channel::<AppliedCommit>(16);
        let snapshot = Arc::new(RwLock::new(ChaosSnapshot::default()));
        let snap_clone = Arc::clone(&snapshot);
        std::thread::spawn(move || run_apply_observer(rx, snap_clone, Arc::new(Mutex::new(None))));

        let meta = StreamMetadata::new(
            StreamId::new(1),
            StreamName::new(CHAOS_STREAM_NAME),
            DataClass::Public,
            Placement::Global,
        );
        tx.send(AppliedCommit {
            op: OpNumber::new(1),
            effects: vec![Effect::StreamMetadataWrite(meta)],
        })
        .unwrap();
        wait_for(&snapshot, |s| s.stream_id.is_some());

        // Send the same write_id twice via two commits. Observer dedups.
        for op in [2u64, 3u64] {
            tx.send(AppliedCommit {
                op: OpNumber::new(op),
                effects: vec![Effect::StorageAppend {
                    stream_id: StreamId::new(1),
                    base_offset: Offset::new(op - 2),
                    events: vec![Bytes::from_static(b"same")],
                }],
            })
            .unwrap();
        }

        wait_for(&snapshot, |s| s.watermark >= 1);
        std::thread::sleep(Duration::from_millis(50));
        let snap = snapshot.read().unwrap();
        assert_eq!(snap.write_ids, vec!["same".to_string()], "dedup failed");
    }
}
