//! `kimberlite-chaos-shim` — tiny HTTP shim used inside chaos VMs.
//!
//! The production `kimberlite` CLI pulls in DuckDB which in turn needs a
//! C++ cross-compiler. On Ubuntu there is no prepackaged
//! `x86_64-unknown-linux-musl-g++`, so building the real CLI as a musl-static
//! binary is a rabbit hole. This shim is the minimum viable stand-in:
//! a std-only HTTP server that exposes the endpoints the chaos InvariantChecker
//! probes.
//!
//! ## Protocol
//!
//!   GET  /health                 -> 200 `replica-<id>`
//!   POST /kv/chaos-probe         -> 200 `ok` if ≥1 peer reachable, else 503
//!   GET  /state/commit_watermark -> 200 `{"watermark":N}` (monotone write count)
//!   GET  /state/write_log        -> 200 `{"write_ids":["id1",...],"total":N}`
//!
//! `POST /kv/chaos-probe` accepts an optional `write_id` in the JSON body:
//!   `{"op":"workload","write_id":"42"}` — if present the ID is recorded in the
//!   write log so post-scenario invariant checkers can verify durability.
//!
//! ## Write log persistence
//!
//! Each acknowledged write_id is appended (one per line) to `KMB_WRITE_LOG_PATH`
//! (default `/tmp/kmb_writes`). Because the Alpine VM uses an ext4 root volume
//! that persists across reboots, the log survives kill+restart scenarios. Write
//! errors (e.g. in `storage_exhaustion`) are silently ignored — the in-memory
//! HashSet still tracks the IDs for the current session.
//!
//! ## Configuration (env vars)
//!
//!   KMB_REPLICA_ID      — integer 0..255
//!   KMB_BIND_ADDR       — e.g. `0.0.0.0:9000`
//!   KMB_PEERS           — comma-separated `ip:port,...`
//!   KMB_OWN_ADDR        — shim's own public address (filtered from peer list)
//!   KMB_WRITE_LOG_PATH  — path for persistent write log (default /tmp/kmb_writes)

use std::collections::HashSet;
use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ============================================================================
// Shared state across connection threads
// ============================================================================

/// Holds the shim's write log. `order` preserves insertion order (used by
/// the linearizability check) and `seen` provides O(1) dedup. Kept in lockstep
/// by `insert()`.
struct WriteLog {
    order: Vec<String>,
    seen: HashSet<String>,
}

impl WriteLog {
    fn new() -> Self {
        Self {
            order: Vec::new(),
            seen: HashSet::new(),
        }
    }

    fn insert(&mut self, id: String) -> bool {
        if self.seen.insert(id.clone()) {
            self.order.push(id);
            true
        } else {
            false
        }
    }

    fn len(&self) -> usize {
        self.order.len()
    }
}

struct ShimState {
    /// Monotone count of acknowledged writes (increments on each new write_id).
    commit_count: AtomicU64,
    /// Ordered dedup log of acknowledged write_ids.
    write_log: Mutex<WriteLog>,
    /// Filesystem path for persistent write log.
    write_log_path: String,
}

impl ShimState {
    fn new(path: &str) -> Self {
        let loaded_order = load_write_log(path);
        let mut log = WriteLog::new();
        for id in loaded_order {
            log.insert(id);
        }
        let count = log.len() as u64;
        Self {
            commit_count: AtomicU64::new(count),
            write_log: Mutex::new(log),
            write_log_path: path.to_string(),
        }
    }

    /// Records a new write_id (idempotent — duplicate IDs are ignored).
    fn record_write(&self, write_id: &str) {
        let is_new = {
            let mut log = self.write_log.lock().expect("write_log poisoned");
            log.insert(write_id.to_string())
        };
        if is_new {
            self.commit_count.fetch_add(1, Ordering::Relaxed);
            append_write_log(&self.write_log_path, write_id);
        }
    }

    fn watermark(&self) -> u64 {
        self.commit_count.load(Ordering::Relaxed)
    }

    /// Serialises the ordered write_log as `{"write_ids":[...],"total":N}`.
    /// Order is insertion order, which is what the linearizability check
    /// relies on — two replicas with the same `(A before B)` insertion order
    /// agree on ordering; a flipped pair is a violation.
    fn write_log_json(&self) -> String {
        let log = self.write_log.lock().expect("write_log poisoned");
        let ids: String = log
            .order
            .iter()
            .map(|id| format!("\"{}\"", id.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"write_ids\":[{}],\"total\":{}}}", ids, log.len())
    }

    /// Returns a deterministic, ordering-independent fingerprint of the
    /// acknowledged write_id set.
    ///
    /// The hash is FNV-1a 64 over sorted write_ids joined by `'\n'`.
    /// Sorting makes the result independent of insertion order, so two
    /// replicas that acknowledged the same writes produce the same hash
    /// regardless of how they learned about them.
    ///
    /// FNV-1a is not cryptographic, but the adversary here is not crafting
    /// write_ids to collide — this is for divergence detection, not
    /// integrity. The shim binary is deliberately std-only (musl-static)
    /// so we avoid bringing in SHA-2 / BLAKE3 C dependencies.
    fn commit_hash(&self) -> String {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0100_0000_01b3;

        let log = self.write_log.lock().expect("write_log poisoned");
        let mut ids: Vec<&String> = log.seen.iter().collect();
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
}

// ============================================================================
// Persistence helpers
// ============================================================================

/// Returns the persisted write_ids in insertion order (one per line).
/// Duplicates are preserved here — `WriteLog::insert` will dedup on load.
fn load_write_log(path: &str) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(std::string::ToString::to_string)
        .collect()
}

fn append_write_log(path: &str, write_id: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
    {
        if writeln!(f, "{write_id}").is_ok() {
            // fsync ensures the write reaches the virtual disk before we
            // return 200 OK to the client. Combined with QEMU's
            // cache=writethrough, this guarantees durability across
            // kill+restart scenarios.
            let _ = f.sync_all();
        }
    }
    // Silently ignore errors (storage_exhaustion scenario fills the disk).
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let replica_id: u8 = env::var("KMB_REPLICA_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let bind_addr = env::var("KMB_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:9000".into());
    let own_advertised = env::var("KMB_OWN_ADDR").unwrap_or_default();
    let peers: Vec<String> = env::var("KMB_PEERS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().to_string())
        .filter(|s| s != &own_advertised)
        .collect();
    // Default to a path on the ext4 root volume that survives VM restarts.
    // /tmp on Alpine is typically tmpfs and is cleared on reboot.
    let write_log_path =
        env::var("KMB_WRITE_LOG_PATH").unwrap_or_else(|_| "/var/lib/kimberlite/writes".into());

    eprintln!(
        "kimberlite-chaos-shim replica_id={replica_id} bind={bind_addr} \
         own={own_advertised} peers={peers:?} write_log={write_log_path}"
    );

    let listener =
        TcpListener::bind(&bind_addr).unwrap_or_else(|e| panic!("bind {bind_addr} failed: {e}"));

    let state = Arc::new(ShimState::new(&write_log_path));

    // Spawn a gossip thread. Every GOSSIP_INTERVAL_MS, pull each reachable
    // peer's /state/write_log and merge into the local log. During a network
    // partition peers are unreachable so no gossip happens — the shim
    // correctly preserves split-brain semantics. After heal, gossip resumes
    // and the cluster converges within a few hundred ms (well inside the
    // InvariantChecker's 500ms retry window on `no_divergence_after_heal`).
    //
    // This is deliberately simple: last-write-wins union of write_id sets,
    // no versioning, no causality tracking. Real Kimberlite uses VSR
    // consensus; the shim is only modelling *convergence*, not ordering.
    {
        let peers = peers.clone();
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            gossip_loop(&peers, &state);
        });
    }

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let peers = peers.clone();
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    if let Err(e) = handle(stream, replica_id, &peers, &state) {
                        eprintln!("handle error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}

// ============================================================================
// Request handler
// ============================================================================

fn handle(
    mut stream: TcpStream,
    replica_id: u8,
    peers: &[String],
    state: &ShimState,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);

    // Parse request line.
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return write_response(&mut stream, 400, "bad request");
    }
    let method = parts[0];
    let path = parts[1];

    // Read headers and body.
    let mut content_length: usize = 0;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(v) = line
            .trim()
            .strip_prefix("Content-Length:")
            .or_else(|| line.trim().strip_prefix("content-length:"))
        {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body_bytes = vec![0u8; content_length.min(4096)]; // cap at 4KB
    if content_length > 0 {
        let _ = reader.read_exact(&mut body_bytes);
    }

    match (method, path) {
        ("GET", "/health") => write_response(&mut stream, 200, &format!("replica-{replica_id}")),

        ("POST", "/kv/chaos-probe") => {
            let body = String::from_utf8_lossy(&body_bytes);
            let write_id = extract_write_id(&body).map(String::from);

            // Synchronously replicate to peers BEFORE acking. This models
            // VSR-style prepare-ok quorum: a write is durable only when
            // ≥ f+1 replicas have recorded it, so that any single node
            // loss (kill, restart, partition) still leaves a quorum that
            // holds the write.
            //
            // For N=3, f=1, so we need 1 peer ack in addition to the
            // local record. Record locally first, then push to peers —
            // if ≥1 peer acks, return 200; otherwise 503 "no_quorum".
            // Record locally first (fsync before returning).
            match &write_id {
                Some(id) => state.record_write(id),
                None => {
                    state.commit_count.fetch_add(1, Ordering::Relaxed);
                }
            }

            // Fan out to peers and require ≥ 1 ack (for N=3 → f+1 = 2 total).
            let peer_acks = push_write_to_peers(peers, write_id.as_deref());
            if peer_acks >= 1 {
                write_response(&mut stream, 200, "ok")
            } else {
                write_response(
                    &mut stream,
                    503,
                    "no_quorum: insufficient peer acks for durable ack",
                )
            }
        }

        // Internal replication push from a peer shim. Accepts a JSON
        // array of write_ids and records them locally without fan-out,
        // preventing infinite recursion. Idempotent (record_write dedups).
        ("POST", "/internal/replicate") => {
            let body = String::from_utf8_lossy(&body_bytes);
            if let Some(ids) = parse_write_ids(&body) {
                for id in ids {
                    state.record_write(&id);
                }
            }
            write_response(&mut stream, 200, "ok")
        }

        ("GET", "/state/commit_watermark") => {
            let w = state.watermark();
            write_response(&mut stream, 200, &format!("{{\"watermark\":{w}}}"))
        }

        ("GET", "/state/write_log") => write_response(&mut stream, 200, &state.write_log_json()),

        ("GET", "/state/commit_hash") => {
            // Ordering-independent content hash of the write_id set.  Used
            // by `check_no_divergence_after_heal` to catch cases where two
            // replicas are both alive but hold different committed sets.
            let hash = state.commit_hash();
            write_response(&mut stream, 200, &format!("{{\"commit_hash\":\"{hash}\"}}"))
        }

        _ => write_response(&mut stream, 404, "not found"),
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn write_response(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "",
    };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\n\
         Content-Length: {}\r\n\
         Content-Type: text/plain\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

/// Extracts the `write_id` value from a JSON body like `{"write_id":"42"}`.
/// Uses a simple string search — no serde dependency.
fn extract_write_id(body: &str) -> Option<&str> {
    const KEY: &str = "\"write_id\":\"";
    let start = body.find(KEY)? + KEY.len();
    let end = body[start..].find('"')? + start;
    let id = &body[start..end];
    if id.is_empty() { None } else { Some(id) }
}

/// How often the gossip loop pulls from peers. 250ms converges comfortably
/// within the 500ms retry window `check_no_divergence_after_heal` uses.
const GOSSIP_INTERVAL_MS: u64 = 250;

/// Background convergence thread. Every `GOSSIP_INTERVAL_MS`, pulls each
/// reachable peer's `/state/write_log` and merges unknown write_ids into
/// our local log. Unreachable peers are silently skipped — during a network
/// partition this is exactly the behaviour we want.
///
/// Blocking TCP + a hand-rolled HTTP parse keeps the shim std-only (no
/// ureq / reqwest / tokio) so it stays musl-static with zero deps.
fn gossip_loop(peers: &[String], state: &ShimState) {
    loop {
        std::thread::sleep(Duration::from_millis(GOSSIP_INTERVAL_MS));
        for peer in peers {
            if let Some(ids) = fetch_peer_write_log(peer) {
                for id in ids {
                    state.record_write(&id);
                }
            }
        }
    }
}

/// Fetches `http://<peer>/state/write_log` and returns the list of write_ids
/// it reports. Returns `None` on any transport, HTTP, or parse failure —
/// callers treat that as "peer unreachable, skip".
fn fetch_peer_write_log(peer: &str) -> Option<Vec<String>> {
    let addr = SocketAddr::from_str(peer).ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(200)).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    stream.set_write_timeout(Some(Duration::from_millis(200))).ok()?;

    let req = format!(
        "GET /state/write_log HTTP/1.1\r\nHost: {peer}\r\nConnection: close\r\n\r\n",
    );
    stream.write_all(req.as_bytes()).ok()?;

    let mut buf = Vec::with_capacity(4096);
    std::io::copy(&mut stream.take(65_536), &mut buf).ok()?;
    let response = String::from_utf8_lossy(&buf);

    // Body starts after the empty CRLF line.
    let body_start = response.find("\r\n\r\n")? + 4;
    if !response.contains(" 200 ") {
        return None;
    }
    let body = &response[body_start..];
    parse_write_ids(body)
}

/// Extracts the `"write_ids":[...]` array from the shim's JSON response.
/// Returns the list of ids, or `None` on malformed input. Avoids serde to
/// keep the shim std-only.
fn parse_write_ids(body: &str) -> Option<Vec<String>> {
    const KEY: &str = "\"write_ids\":[";
    let start = body.find(KEY)? + KEY.len();
    let end = body[start..].find(']')? + start;
    let inner = &body[start..end];
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }
    let mut ids = Vec::new();
    for chunk in inner.split(',') {
        let trimmed = chunk.trim();
        if trimmed.len() < 2 || !trimmed.starts_with('"') || !trimmed.ends_with('"') {
            continue;
        }
        ids.push(trimmed[1..trimmed.len() - 1].to_string());
    }
    Some(ids)
}

/// Fan out a write_id to every peer's `/internal/replicate` and return the
/// number of peers that acknowledged with 200 OK. Unreachable peers count
/// as 0. Used by the `/kv/chaos-probe` handler to enforce a quorum-replication
/// pre-condition before acking writes to the client.
///
/// A write_id of `None` fans out an empty replicate (no-op at the peer),
/// which still lets the handler count reachability. Peers are contacted
/// sequentially; fastest-first would require threads, not worth it given
/// the modest N.
fn push_write_to_peers(peers: &[String], write_id: Option<&str>) -> usize {
    let body = match write_id {
        Some(id) => {
            let escaped = id.replace('\\', "\\\\").replace('"', "\\\"");
            format!("{{\"write_ids\":[\"{escaped}\"]}}")
        }
        None => String::from(r#"{"write_ids":[]}"#),
    };

    let mut acks = 0;
    for peer in peers {
        if push_to_peer(peer, &body) {
            acks += 1;
        }
    }
    acks
}

/// POST to `http://<peer>/internal/replicate`. Returns true on 200 OK,
/// false on any transport/HTTP failure. Hard-coded timeouts keep a slow
/// peer from stalling the client-facing ack path.
fn push_to_peer(peer: &str, body: &str) -> bool {
    let Ok(addr) = SocketAddr::from_str(peer) else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(300)) else {
        return false;
    };
    if stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .is_err()
        || stream
            .set_write_timeout(Some(Duration::from_millis(300)))
            .is_err()
    {
        return false;
    }

    let req = format!(
        "POST /internal/replicate HTTP/1.1\r\nHost: {peer}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = Vec::with_capacity(256);
    if std::io::copy(&mut stream.take(1024), &mut buf).is_err() {
        return false;
    }
    let resp = String::from_utf8_lossy(&buf);
    resp.contains(" 200 ")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_write_id_basic() {
        assert_eq!(
            extract_write_id(r#"{"op":"workload","write_id":"42"}"#),
            Some("42")
        );
    }

    #[test]
    fn extract_write_id_no_field() {
        assert_eq!(extract_write_id(r#"{"op":"workload"}"#), None);
    }

    #[test]
    fn extract_write_id_empty_value() {
        assert_eq!(extract_write_id(r#"{"write_id":""}"#), None);
    }

    #[test]
    fn shim_state_idempotent_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("writes").display().to_string();
        let state = ShimState::new(&path);

        state.record_write("abc");
        state.record_write("abc"); // duplicate — must not increment twice
        state.record_write("def");

        assert_eq!(state.watermark(), 2);
        let json = state.write_log_json();
        assert!(json.contains("\"abc\""));
        assert!(json.contains("\"def\""));
        assert!(json.contains("\"total\":2"));
    }

    #[test]
    fn shim_state_persists_and_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("writes").display().to_string();

        {
            let state = ShimState::new(&path);
            state.record_write("x1");
            state.record_write("x2");
        }

        // Reload — simulates a restart.
        let state2 = ShimState::new(&path);
        assert_eq!(state2.watermark(), 2);
        let json = state2.write_log_json();
        assert!(json.contains("\"x1\""));
        assert!(json.contains("\"x2\""));
    }

    #[test]
    fn load_write_log_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent").display().to_string();
        let log = load_write_log(&path);
        assert!(log.is_empty());
    }

    #[test]
    fn write_log_preserves_insertion_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("writes").display().to_string();
        let state = ShimState::new(&path);

        for id in ["w1", "w2", "w3", "w4", "w5"] {
            state.record_write(id);
        }

        let body = state.write_log_json();
        let p1 = body.find("\"w1\"").unwrap();
        let p2 = body.find("\"w2\"").unwrap();
        let p3 = body.find("\"w3\"").unwrap();
        assert!(p1 < p2 && p2 < p3, "JSON lost insertion order: {body}");

        // Reload — insertion order must survive restart.
        let state2 = ShimState::new(&path);
        let body2 = state2.write_log_json();
        let p1b = body2.find("\"w1\"").unwrap();
        let p5b = body2.find("\"w5\"").unwrap();
        assert!(p1b < p5b, "order not preserved across reload: {body2}");
    }

    #[test]
    fn commit_hash_empty_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("writes").display().to_string();
        let state = ShimState::new(&path);
        // Empty state produces the FNV-1a offset-basis fingerprint.
        assert_eq!(
            state.commit_hash(),
            format!("{:016x}", 0xcbf2_9ce4_8422_2325_u64)
        );
    }

    #[test]
    fn commit_hash_is_ordering_independent() {
        let dir = tempfile::tempdir().unwrap();
        let path_a = dir.path().join("writes_a").display().to_string();
        let path_b = dir.path().join("writes_b").display().to_string();

        let state_a = ShimState::new(&path_a);
        state_a.record_write("alpha");
        state_a.record_write("bravo");
        state_a.record_write("charlie");

        let state_b = ShimState::new(&path_b);
        // Insert in a different order.
        state_b.record_write("charlie");
        state_b.record_write("alpha");
        state_b.record_write("bravo");

        assert_eq!(
            state_a.commit_hash(),
            state_b.commit_hash(),
            "same write_id set → same commit_hash regardless of insertion order"
        );
    }

    #[test]
    fn parse_write_ids_basic() {
        let body = r#"{"write_ids":["a","b","c"],"total":3}"#;
        assert_eq!(
            parse_write_ids(body),
            Some(vec!["a".into(), "b".into(), "c".into()])
        );
    }

    #[test]
    fn parse_write_ids_empty() {
        let body = r#"{"write_ids":[],"total":0}"#;
        assert_eq!(parse_write_ids(body), Some(Vec::new()));
    }

    #[test]
    fn parse_write_ids_single() {
        let body = r#"{"write_ids":["only"],"total":1}"#;
        assert_eq!(parse_write_ids(body), Some(vec!["only".into()]));
    }

    #[test]
    fn parse_write_ids_no_field() {
        assert_eq!(parse_write_ids(r#"{"total":5}"#), None);
    }

    #[test]
    fn fetch_peer_write_log_merges_via_record_write() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = format!("127.0.0.1:{port}");

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let body = r#"{"write_ids":["peer-write-1","peer-write-2"],"total":2}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\
                     Content-Type: application/json\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });

        let ids = fetch_peer_write_log(&peer).expect("peer reachable");
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"peer-write-1".to_string()));
        assert!(ids.contains(&"peer-write-2".to_string()));
    }

    #[test]
    fn push_to_peer_returns_true_on_200() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = format!("127.0.0.1:{port}");

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\
                      Content-Type: text/plain\r\nConnection: close\r\n\r\nok",
                );
            }
        });

        assert!(push_to_peer(&peer, r#"{"write_ids":["x"]}"#));
    }

    #[test]
    fn push_to_peer_returns_false_on_unreachable() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(!push_to_peer(&format!("127.0.0.1:{port}"), r#"{"write_ids":[]}"#));
    }

    #[test]
    fn fetch_peer_write_log_returns_none_on_unreachable() {
        // Bind to find a free port then drop — connect-refused path.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(fetch_peer_write_log(&format!("127.0.0.1:{port}")).is_none());
    }

    #[test]
    fn commit_hash_differs_on_divergent_sets() {
        let dir = tempfile::tempdir().unwrap();
        let path_a = dir.path().join("a").display().to_string();
        let path_b = dir.path().join("b").display().to_string();

        let state_a = ShimState::new(&path_a);
        state_a.record_write("w1");
        state_a.record_write("w2");

        let state_b = ShimState::new(&path_b);
        state_b.record_write("w1");
        state_b.record_write("w3"); // different id

        assert_ne!(state_a.commit_hash(), state_b.commit_hash());
    }
}
