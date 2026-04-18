//! End-to-end clinic-management walkthrough — Rust SDK.
//!
//! Mirror of `examples/healthcare/clinic.ts` and `clinic.py`. Uses `Pool` +
//! direct client methods. `PooledClient` derefs to `Client`, so every public
//! client method (`list_tables`, `consent_grant`, `erasure_request`, …) is
//! reachable through the pooled handle.
//!
//! # Running
//!
//! 1. Start a server loaded with the clinic schema:
//!
//!    ```bash
//!    examples/healthcare/00-setup.sh
//!    ```
//!
//! 2. Run the walkthrough:
//!
//!    ```bash
//!    cd examples/rust
//!    cargo run --example clinic
//!    ```
//!
//! Override the server address with `KIMBERLITE_ADDR=host:port`.

use anyhow::{Context, Result};
use kimberlite_client::{ConsentPurpose, Pool, PoolConfig, QueryParam, QueryResponse, QueryValue};
use kimberlite_types::TenantId;
use std::env;

#[derive(Debug)]
struct Patient {
    id: i64,
    mrn: String,
    name: String,
    _dob: String,
    primary_provider_id: i64,
}

fn main() -> Result<()> {
    let addr = env::var("KIMBERLITE_ADDR").unwrap_or_else(|_| "127.0.0.1:5432".to_string());

    let pool = Pool::new(
        addr.as_str(),
        TenantId::new(1),
        PoolConfig {
            max_size: 8,
            ..Default::default()
        },
    )
    .context("pool create")?;
    println!("✓ pool created");

    // 1. Admin — list tables.
    {
        let mut client = pool.acquire()?;
        let tables = client.list_tables().context("list_tables")?;
        let names: Vec<_> = tables.iter().map(|t| t.name.as_str()).collect();
        println!(
            "✓ list_tables → {} tables: {}",
            tables.len(),
            names.join(", ")
        );
    }

    // 2. Typed row mapping via direct SQL.
    let patients: Vec<Patient> = {
        let mut client = pool.acquire()?;
        let response = client.query(
            "SELECT id, medical_record_number, first_name, last_name, \
             date_of_birth, primary_provider_id \
             FROM patients WHERE active = $1 ORDER BY id",
            &[QueryParam::Boolean(true)],
        )?;
        map_patients(&response)
    };
    println!("✓ typed query → {} active patients", patients.len());
    for p in &patients {
        println!(
            "  · #{} {} (MRN {}) → provider {}",
            p.id, p.name, p.mrn, p.primary_provider_id
        );
    }

    // 3. Consent — grant research consent for patient 1.
    let subject_id = "patient:1";
    {
        let mut client = pool.acquire()?;
        let granted = client
            .consent_grant(subject_id, ConsentPurpose::Research, None)
            .context("consent_grant")?;
        println!(
            "✓ consent_grant → consent_id={}",
            granted.consent_id
        );

        let ok = client
            .consent_check(subject_id, ConsentPurpose::Research)
            .context("consent_check")?;
        println!("  · consent_check({subject_id}, Research) → {ok}");
    }

    // 4. Erasure — GDPR Article 17.
    {
        let mut client = pool.acquire()?;
        let req = client
            .erasure_request(subject_id)
            .context("erasure_request")?;
        println!(
            "✓ erasure_request → request_id={} status={:?}",
            req.request_id, req.status
        );
        let stream_count = req.streams_affected.len();
        if stream_count > 0 {
            client
                .erasure_mark_progress(&req.request_id, req.streams_affected)
                .context("erasure_mark_progress")?;
            println!("  · mark_progress for {} stream(s)", stream_count);
        }
        println!("  · erasure_complete() skipped in demo — see docs/concepts/data-portability.md");
    }

    // 5. Pool stats.
    let stats = pool.stats();
    println!(
        "✓ pool.stats → open={} in_use={} idle={}",
        stats.open, stats.in_use, stats.idle
    );

    println!("\n✅ clinic walkthrough complete");

    pool.shutdown();
    Ok(())
}

fn map_patients(response: &QueryResponse) -> Vec<Patient> {
    let col = |name: &str| {
        response
            .columns
            .iter()
            .position(|c| c.as_str() == name)
            .unwrap_or_else(|| panic!("column '{name}' missing from response"))
    };
    let id_ix = col("id");
    let mrn_ix = col("medical_record_number");
    let fn_ix = col("first_name");
    let ln_ix = col("last_name");
    let dob_ix = col("date_of_birth");
    let ppid_ix = col("primary_provider_id");

    response
        .rows
        .iter()
        .map(|row| Patient {
            id: as_bigint(&row[id_ix]),
            mrn: as_text(&row[mrn_ix]),
            name: format!("{} {}", as_text(&row[fn_ix]), as_text(&row[ln_ix])),
            _dob: as_text(&row[dob_ix]),
            primary_provider_id: as_bigint(&row[ppid_ix]),
        })
        .collect()
}

fn as_bigint(v: &QueryValue) -> i64 {
    match v {
        QueryValue::BigInt(n) => *n,
        other => panic!("expected BIGINT, got {other:?}"),
    }
}

fn as_text(v: &QueryValue) -> String {
    match v {
        QueryValue::Text(s) => s.clone(),
        QueryValue::Null => String::new(),
        QueryValue::Timestamp(t) => t.to_string(),
        other => panic!("expected TEXT, got {other:?}"),
    }
}
