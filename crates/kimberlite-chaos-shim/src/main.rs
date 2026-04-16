//! `kimberlite-chaos-shim` — tiny HTTP shim used inside chaos VMs.
//!
//! The production `kimberlite` CLI pulls in DuckDB which in turn needs a
//! C++ cross-compiler. On Ubuntu there is no prepackaged
//! `x86_64-linux-musl-g++`, so building the real CLI as a musl-static
//! binary is a rabbit hole. This shim is the minimum viable stand-in:
//! a std-only HTTP server that exposes the exact two endpoints the
//! chaos InvariantChecker probes.
//!
//! Protocol:
//!   GET  /health           -> 200 `replica-<id>`
//!   POST /kv/chaos-probe   -> 200 `ok` if we can reach ≥1 peer, else
//!                              503 `no_quorum`
//!
//! Configuration comes from env vars populated by OpenRC (see
//! `tools/chaos/init-kimberlite.sh`):
//!   KMB_REPLICA_ID   — integer 0..255
//!   KMB_BIND_ADDR    — e.g. `0.0.0.0:9000`
//!   KMB_PEERS        — comma-separated `ip:port,ip:port,...`
//!
//! Peer reachability is probed on demand (inside /kv/chaos-probe): we
//! try a TCP connect with a 500ms timeout. If all peers (excluding
//! ourselves) are unreachable the replica is isolated and we refuse the
//! write.

use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

fn main() {
    let replica_id: u8 = env::var("KMB_REPLICA_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let bind_addr = env::var("KMB_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:9000".into());
    // The shim's *own* public address in the peer list — e.g. `10.42.0.12:9000`.
    // We compare against this to filter ourselves out of the peer-reachability
    // probe (otherwise the replica would always see ≥1 peer up: itself).
    let own_advertised = env::var("KMB_OWN_ADDR").unwrap_or_default();
    let peers: Vec<String> = env::var("KMB_PEERS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().to_string())
        .filter(|s| s != &own_advertised)
        .collect();

    eprintln!(
        "kimberlite-chaos-shim replica_id={replica_id} bind={bind_addr} own={own_advertised} peers={peers:?}"
    );

    let listener = TcpListener::bind(&bind_addr)
        .unwrap_or_else(|e| panic!("bind {bind_addr} failed: {e}"));

    // Monotonically increasing count of writes acknowledged as 200 OK.
    // Exposed via GET /state/commit_watermark for the chaos invariant checker.
    let commit_count = Arc::new(AtomicU64::new(0));

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let peers = peers.clone();
                let commit_count = Arc::clone(&commit_count);
                std::thread::spawn(move || {
                    if let Err(e) = handle(stream, replica_id, &peers, &*commit_count) {
                        eprintln!("handle error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}

fn handle(
    mut stream: TcpStream,
    replica_id: u8,
    peers: &[String],
    commit_count: &AtomicU64,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return write_response(&mut stream, 400, "bad request");
    }
    let method = parts[0];
    let path = parts[1];

    // Skip remaining headers until the blank line so the socket is clean.
    let mut content_length: usize = 0;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(v) = line.trim().strip_prefix("Content-Length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
        if let Some(v) = line.trim().strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }
    if content_length > 0 {
        let mut body = vec![0u8; content_length];
        let _ = reader.read_exact(&mut body);
    }

    match (method, path) {
        ("GET", "/health") => {
            let body = format!("replica-{replica_id}");
            write_response(&mut stream, 200, &body)
        }
        ("POST", "/kv/chaos-probe") => {
            if can_reach_any_peer(peers) {
                // Increment watermark — this write was acknowledged.
                commit_count.fetch_add(1, Ordering::Relaxed);
                write_response(&mut stream, 200, "ok")
            } else {
                write_response(&mut stream, 503, "no_quorum: no peers reachable")
            }
        }
        ("GET", "/state/commit_watermark") => {
            let watermark = commit_count.load(Ordering::Relaxed);
            write_response(&mut stream, 200, &format!("{{\"watermark\":{watermark}}}"))
        }
        _ => write_response(&mut stream, 404, "not found"),
    }
}

fn write_response(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "",
    };
    let response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

fn can_reach_any_peer(peers: &[String]) -> bool {
    // `peers` is pre-filtered at startup to exclude the shim's own
    // advertised address, so every entry here is an actual remote peer.
    let deadline = Instant::now() + Duration::from_millis(2000);
    for peer in peers {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::ZERO);
        if remaining.is_zero() {
            break;
        }
        let per_peer_timeout = remaining.min(Duration::from_millis(500));
        let Ok(addr) = SocketAddr::from_str(peer) else { continue };
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
