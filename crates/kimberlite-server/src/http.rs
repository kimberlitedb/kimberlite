//! Lightweight HTTP sidecar for metrics, health, and readiness endpoints.
//!
//! Runs on a separate port (default 9090) alongside the main binary protocol server.
//! Provides minimal HTTP/1.1 parsing for three endpoints:
//! - `GET /metrics` — Prometheus text format
//! - `GET /health` — Liveness check (always 200 if process is running)
//! - `GET /ready` — Readiness check (503 if unhealthy)

use std::io::{Read, Write};
use std::net::SocketAddr;

use mio::net::TcpListener;
use mio::{Interest, Poll, Token};
use tracing::{debug, error, warn};

use crate::health::HealthChecker;
use crate::metrics::Metrics;

/// Token offset for the HTTP listener in the mio event loop.
/// Must not conflict with LISTENER_TOKEN (0) or SIGNAL_TOKEN (1).
pub const HTTP_LISTENER_TOKEN: Token = Token(1_000_000);

/// HTTP sidecar that serves observability endpoints.
pub struct HttpSidecar {
    listener: TcpListener,
}

impl HttpSidecar {
    /// Binds the HTTP sidecar listener and registers it with the poll.
    pub fn bind(addr: SocketAddr, poll: &Poll) -> std::io::Result<Self> {
        let mut listener = TcpListener::bind(addr)?;
        poll.registry()
            .register(&mut listener, HTTP_LISTENER_TOKEN, Interest::READABLE)?;
        tracing::info!("HTTP sidecar listening on {addr}");
        Ok(Self { listener })
    }

    /// Handles incoming HTTP connections (non-blocking, call from event loop).
    ///
    /// Accepts a connection, reads the request, dispatches to the appropriate
    /// handler, and writes the response. Connections are not kept alive.
    pub fn handle_accept(&self, health_checker: &HealthChecker) {
        loop {
            match self.listener.accept() {
                Ok((mut stream, addr)) => {
                    debug!("HTTP connection from {addr}");

                    // Read request (small buffer — these are simple GET requests)
                    let mut buf = [0u8; 1024];
                    let n = match stream.read(&mut buf) {
                        Ok(0) => continue,
                        Ok(n) => n,
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(e) => {
                            warn!("HTTP read error from {addr}: {e}");
                            continue;
                        }
                    };

                    let request = String::from_utf8_lossy(&buf[..n]);

                    // Parse first line: "GET /path HTTP/1.1"
                    let response = if let Some(path) = parse_request_path(&request) {
                        dispatch(path, health_checker)
                    } else {
                        http_response(400, "text/plain", "Bad Request")
                    };

                    if let Err(e) = stream.write_all(response.as_bytes()) {
                        debug!("HTTP write error to {addr}: {e}");
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    error!("HTTP accept error: {e}");
                    break;
                }
            }
        }
    }
}

/// Parse the request path from an HTTP request line.
fn parse_request_path(request: &str) -> Option<&str> {
    let first_line = request.lines().next()?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next()?;
    let path = parts.next()?;

    if method != "GET" {
        return None;
    }

    Some(path)
}

/// Dispatch a request to the appropriate handler.
fn dispatch(path: &str, health_checker: &HealthChecker) -> String {
    match path {
        "/metrics" => {
            let body = Metrics::global().render();
            http_response(
                200,
                "text/plain; version=0.0.4; charset=utf-8",
                &body,
            )
        }
        "/health" => {
            let response = health_checker.liveness_check();
            let body = response.to_json();
            http_response(200, "application/json", &body)
        }
        "/ready" => {
            let response = health_checker.readiness_check();
            let status_code = if response.status.is_healthy() {
                200
            } else {
                503
            };
            let body = response.to_json();
            http_response(status_code, "application/json", &body)
        }
        _ => http_response(404, "text/plain", "Not Found"),
    }
}

/// Build a minimal HTTP/1.1 response.
fn http_response(status: u16, content_type: &str, body: &str) -> String {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        503 => "Service Unavailable",
        _ => "Unknown",
    };

    format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_request_path_get() {
        assert_eq!(
            parse_request_path("GET /metrics HTTP/1.1\r\nHost: localhost\r\n"),
            Some("/metrics")
        );
    }

    #[test]
    fn test_parse_request_path_health() {
        assert_eq!(
            parse_request_path("GET /health HTTP/1.1\r\n"),
            Some("/health")
        );
    }

    #[test]
    fn test_parse_request_path_post_rejected() {
        assert_eq!(
            parse_request_path("POST /metrics HTTP/1.1\r\n"),
            None
        );
    }

    #[test]
    fn test_http_response_format() {
        let resp = http_response(200, "text/plain", "OK");
        assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(resp.contains("Content-Length: 2\r\n"));
        assert!(resp.ends_with("OK"));
    }

    #[test]
    fn test_dispatch_metrics() {
        let checker = HealthChecker::new("/tmp");
        let resp = dispatch("/metrics", &checker);
        assert!(resp.contains("200 OK"));
        assert!(resp.contains("text/plain"));
    }

    #[test]
    fn test_dispatch_health() {
        let checker = HealthChecker::new("/tmp");
        let resp = dispatch("/health", &checker);
        assert!(resp.contains("200 OK"));
        assert!(resp.contains("application/json"));
    }

    #[test]
    fn test_dispatch_not_found() {
        let checker = HealthChecker::new("/tmp");
        let resp = dispatch("/nonexistent", &checker);
        assert!(resp.contains("404 Not Found"));
    }
}
