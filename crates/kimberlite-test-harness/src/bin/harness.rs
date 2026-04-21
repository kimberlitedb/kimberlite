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

use std::io::{self, BufRead, Write};

use kimberlite_test_harness::TestKimberlite;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build with defaults — tenant id defaulting keeps the protocol
    // forward-compatible with future `--tenant=<n>` args.
    let tenant = parse_tenant_arg().unwrap_or(1_000_000);
    let harness = TestKimberlite::builder().tenant(tenant).build()?;
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
