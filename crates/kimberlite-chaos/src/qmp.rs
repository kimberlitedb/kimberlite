//! Minimal QMP (QEMU Machine Protocol) client over a UNIX socket.
//!
//! QMP is line-delimited JSON. The protocol is:
//!
//!   1. QEMU writes a greeting line on connect:
//!        `{"QMP": {"version": {...}, "capabilities": [...]}}`
//!   2. The client writes `{"execute": "qmp_capabilities"}` to enter
//!      command mode; QEMU responds `{"return": {}}`.
//!   3. The client issues one or more commands, each of the form
//!        `{"execute": "<name>"}`
//!      with an optional `arguments` object.
//!
//! This module implements just enough of that protocol for chaos scenarios:
//! graceful shutdown via `system_powerdown` plus `quit` as a fallback. No
//! async, no crates beyond workspace `serde_json`.
//!
//! ## Error handling
//!
//! All I/O errors collapse to [`QmpError::Io`]. Protocol errors (missing
//! greeting, malformed response, server error objects) collapse to
//! [`QmpError::Protocol`]. Callers are expected to log and fall back to
//! hard-kill semantics on any error — graceful shutdown is a best-effort
//! optimization.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum QmpError {
    #[error("QMP I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("QMP protocol error: {0}")]
    Protocol(String),
}

/// Reads and parses one newline-terminated JSON object from the stream.
fn read_line<R: BufRead>(reader: &mut R) -> Result<serde_json::Value, QmpError> {
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Err(QmpError::Protocol("unexpected EOF".into()));
    }
    serde_json::from_str::<serde_json::Value>(line.trim())
        .map_err(|e| QmpError::Protocol(format!("malformed JSON: {e}: {line}")))
}

/// A connected QMP client with capabilities negotiated.
pub struct QmpClient {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl QmpClient {
    /// Connects to the QMP socket and performs the capability handshake.
    pub fn connect(path: &Path) -> Result<Self, QmpError> {
        Self::connect_with_timeout(path, Duration::from_secs(5))
    }

    /// Connects with a custom I/O timeout applied to both reads and writes.
    pub fn connect_with_timeout(path: &Path, timeout: Duration) -> Result<Self, QmpError> {
        let stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let writer = stream.try_clone()?;
        let mut reader = BufReader::new(stream);

        // 1. Read the greeting.
        let greeting = read_line(&mut reader)?;
        if greeting.get("QMP").is_none() {
            return Err(QmpError::Protocol(format!(
                "expected QMP greeting, got: {greeting}"
            )));
        }

        let mut client = Self { reader, writer };

        // 2. Negotiate capabilities.
        let reply = client.send_command("qmp_capabilities", None)?;
        if reply.get("return").is_none() {
            return Err(QmpError::Protocol(format!(
                "qmp_capabilities response missing 'return': {reply}"
            )));
        }

        Ok(client)
    }

    /// Sends a QMP command and returns the matching response line.
    ///
    /// Asynchronous `event` messages sent before the response are discarded.
    pub fn send_command(
        &mut self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, QmpError> {
        let cmd = if let Some(args) = arguments {
            serde_json::json!({"execute": name, "arguments": args})
        } else {
            serde_json::json!({"execute": name})
        };

        let mut line = serde_json::to_string(&cmd)
            .map_err(|e| QmpError::Protocol(format!("cannot serialize command: {e}")))?;
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;
        self.writer.flush()?;

        loop {
            let reply = read_line(&mut self.reader)?;
            // Events have an "event" key — skip them, wait for return/error.
            if reply.get("event").is_some() {
                continue;
            }
            if let Some(err) = reply.get("error") {
                return Err(QmpError::Protocol(format!("QMP error: {err}")));
            }
            return Ok(reply);
        }
    }

    /// Requests an ACPI powerdown.
    pub fn system_powerdown(&mut self) -> Result<(), QmpError> {
        self.send_command("system_powerdown", None).map(|_| ())
    }

    /// Terminates the QEMU process unconditionally via the `quit` command.
    pub fn quit(&mut self) -> Result<(), QmpError> {
        self.send_command("quit", None).map(|_| ())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::thread;
    use tempfile::tempdir;

    /// Spawns a fake QMP server on `path`. It sends the greeting, then echoes
    /// a `{"return": {}}` reply for every command that arrives until the
    /// client disconnects. Captures every received line into `received`.
    fn spawn_fake_server(
        path: std::path::PathBuf,
        received: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) -> thread::JoinHandle<()> {
        let listener = UnixListener::bind(&path).expect("bind QMP listener");
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let greeting =
                r#"{"QMP":{"version":{"qemu":{"major":9,"minor":0,"micro":0}},"capabilities":[]}}"#;
            writeln!(stream, "{greeting}").unwrap();

            let mut buf = [0u8; 4096];
            let mut acc = String::new();
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                acc.push_str(&String::from_utf8_lossy(&buf[..n]));
                while let Some(newline) = acc.find('\n') {
                    let line: String = acc.drain(..=newline).collect();
                    received.lock().unwrap().push(line.trim().to_string());
                    // Reply with a bare return object for every command.
                    if writeln!(stream, "{}", r#"{"return":{}}"#).is_err() {
                        return;
                    }
                }
            }
        })
    }

    #[test]
    fn handshake_and_commands_roundtrip() {
        let dir = tempdir().unwrap();
        let socket = dir.path().join("qmp.sock");
        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let handle = spawn_fake_server(socket.clone(), received.clone());
        // Give the listener a moment to be ready.
        std::thread::sleep(Duration::from_millis(50));

        let mut client = QmpClient::connect(&socket).expect("connect + handshake");
        client.system_powerdown().expect("system_powerdown reply");
        drop(client);
        let _ = handle.join();

        let got = received.lock().unwrap().clone();
        assert!(
            got.iter().any(|s| s.contains("qmp_capabilities")),
            "expected qmp_capabilities to be sent; got: {got:?}"
        );
        assert!(
            got.iter().any(|s| s.contains("system_powerdown")),
            "expected system_powerdown to be sent; got: {got:?}"
        );
    }
}
