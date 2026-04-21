//! Thin IPC launcher for the `kimberlite-test-harness` crate.
//!
//! Launched by the TS `@kimberlitedb/testing` subpackage and the
//! Python `kimberlite.testing` module. Both SDKs spawn this binary,
//! read the first non-comment line from stdout
//! (`ADDR=127.0.0.1:<port>`), connect a normal SDK client to that
//! address, and run their test workload. On test teardown the SDK
//! wrapper closes stdin (or sends SIGTERM), at which point this
//! process shuts the in-process server down and exits with status 0.
//!
//! Protocol:
//!
//!   stdout line 1: `ADDR=<host>:<port>`          (machine-readable)
//!   stdout line 2: `TENANT=<tenant id>`           (machine-readable)
//!   stdout line 3+: human-readable log lines      (tracing output)
//!
//! stdin: closed by the parent = exit gracefully. Sending a line
//! containing literal `"shutdown\n"` triggers the same path — useful
//! for parents that need to hold stdin open to receive logs.
//!
//! Flags:
//!
//!   `--tenant=<N>`          override default tenant id (1_000_000).
//!   `--backend=tempdir`     use the on-disk event log (default).
//!   `--backend=memory`      use `MemoryStorage` (v0.6.0, pure in-RAM).
//!
//! Unknown flags are silently ignored so that newer SDK wrappers can
//! pass future flags to older CLI binaries without breaking the
//! protocol.

use std::io::{self, BufRead, Write};

use kimberlite_test_harness::{Backend, TestKimberlite};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build with defaults — tenant id defaulting keeps the protocol
    // forward-compatible with future `--tenant=<n>` args.
    let tenant = parse_tenant_arg().unwrap_or(1_000_000);
    let backend = parse_backend_arg().unwrap_or(Backend::TempDir);

    let harness = TestKimberlite::builder()
        .tenant(tenant)
        .backend(backend)
        .build()?;
    let addr = harness.addr();

    {
        let mut out = io::stdout().lock();
        writeln!(out, "ADDR={addr}")?;
        writeln!(out, "TENANT={}", u64::from(harness.tenant()))?;
        out.flush()?;
    }

    // Block on stdin until the parent closes it or sends "shutdown".
    // `stdin.lock().lines()` returns None when stdin is closed.
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        match line {
            Ok(s) if s.trim() == "shutdown" => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }

    // Dropping `harness` runs the deterministic shutdown.
    drop(harness);
    Ok(())
}

/// Parse `--tenant=<N>` from CLI args. Any other argument is ignored
/// — keeps the surface minimal so the IPC contract with both SDK
/// wrappers is a single well-known form.
fn parse_tenant_arg() -> Option<u64> {
    for arg in std::env::args().skip(1) {
        if let Some(v) = arg.strip_prefix("--tenant=") {
            return v.parse().ok();
        }
    }
    None
}

/// Parse `--backend=<tempdir|memory>`. Unknown values fall back to
/// `TempDir` (the default) so older CLI binaries don't crash when
/// newer SDKs pass unfamiliar values.
fn parse_backend_arg() -> Option<Backend> {
    for arg in std::env::args().skip(1) {
        if let Some(v) = arg.strip_prefix("--backend=") {
            return match v {
                "tempdir" => Some(Backend::TempDir),
                "memory" => Some(Backend::InMemory),
                _ => None,
            };
        }
    }
    None
}
