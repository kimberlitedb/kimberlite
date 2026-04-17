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
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

// ============================================================================
// Shared state across connection threads
// ============================================================================

struct ShimState {
    /// Monotone count of acknowledged writes (increments on each new write_id).
    commit_count: AtomicU64,
    /// In-memory set of all acknowledged write_ids (deduplicated).
    write_log: Mutex<HashSet<String>>,
    /// Filesystem path for persistent write log.
    write_log_path: String,
}

impl ShimState {
    fn new(path: &str) -> Self {
        let loaded = load_write_log(path);
        let count = loaded.len() as u64;
        Self {
            commit_count: AtomicU64::new(count),
            write_log: Mutex::new(loaded),
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

    fn write_log_json(&self) -> String {
        let log = self.write_log.lock().expect("write_log poisoned");
        let ids: String = log
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
    /// Sorting makes the result independent of HashSet iteration order, so
    /// two replicas that acknowledged the same writes produce the same
    /// hash regardless of insertion order.
    ///
    /// FNV-1a is not cryptographic, but the adversary here is not crafting
    /// write_ids to collide — this is for divergence detection, not
    /// integrity. The shim binary is deliberately std-only (musl-static)
    /// so we avoid bringing in SHA-2 / BLAKE3 C dependencies.
    fn commit_hash(&self) -> String {
        let log = self.write_log.lock().expect("write_log poisoned");
        let mut ids: Vec<&String> = log.iter().collect();
        ids.sort_unstable();

        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;
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

fn load_write_log(path: &str) -> HashSet<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

fn append_write_log(path: &str, write_id: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
    {
        if writeln!(f, "{}", write_id).is_ok() {
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
    let write_log_path = env::var("KMB_WRITE_LOG_PATH")
        .unwrap_or_else(|_| "/var/lib/kimberlite/writes".into());

    eprintln!(
        "kimberlite-chaos-shim replica_id={replica_id} bind={bind_addr} \
         own={own_advertised} peers={peers:?} write_log={write_log_path}"
    );

    let listener = TcpListener::bind(&bind_addr)
        .unwrap_or_else(|e| panic!("bind {bind_addr} failed: {e}"));

    let state = Arc::new(ShimState::new(&write_log_path));

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
        ("GET", "/health") => {
            write_response(&mut stream, 200, &format!("replica-{replica_id}"))
        }

        ("POST", "/kv/chaos-probe") => {
            if can_reach_any_peer(peers) {
                let body = String::from_utf8_lossy(&body_bytes);
                if let Some(write_id) = extract_write_id(&body) {
                    state.record_write(write_id);
                } else {
                    // No write_id in body — still count as an acknowledged write.
                    state.commit_count.fetch_add(1, Ordering::Relaxed);
                }
                write_response(&mut stream, 200, "ok")
            } else {
                write_response(&mut stream, 503, "no_quorum: no peers reachable")
            }
        }

        ("GET", "/state/commit_watermark") => {
            let w = state.watermark();
            write_response(&mut stream, 200, &format!("{{\"watermark\":{w}}}"))
        }

        ("GET", "/state/write_log") => {
            write_response(&mut stream, 200, &state.write_log_json())
        }

        ("GET", "/state/commit_hash") => {
            // Ordering-independent content hash of the write_id set.  Used
            // by `check_no_divergence_after_heal` to catch cases where two
            // replicas are both alive but hold different committed sets.
            let hash = state.commit_hash();
            write_response(
                &mut stream,
                200,
                &format!("{{\"commit_hash\":\"{hash}\"}}"),
            )
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

fn can_reach_any_peer(peers: &[String]) -> bool {
    let deadline = Instant::now() + Duration::from_millis(2000);
    for peer in peers {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            break;
        }
        let per_peer_timeout = remaining.min(Duration::from_millis(500));
        let Ok(addr) = SocketAddr::from_str(peer) else {
            continue;
        };
        match TcpStream::connect_timeout(&addr, per_peer_timeout) {
            Ok(s) => {
                let _ = s.shutdown(std::net::Shutdown::Both);
                return true;
            }
            Err(_) => continue,
        }
    }
    false
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
    fn commit_hash_empty_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("writes").display().to_string();
        let state = ShimState::new(&path);
        // Empty state produces the FNV-1a offset-basis fingerprint.
        assert_eq!(state.commit_hash(), format!("{:016x}", 0xcbf29ce484222325u64));
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
