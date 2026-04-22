//! v0.6.0 Tier 2 #7 — end-to-end Rust SDK integration test for the
//! masking-policy CRUD surface. Spins up an in-process server via
//! `kimberlite-test-harness`, runs CREATE / ATTACH / LIST / DETACH /
//! DROP against `client.admin().masking_policy()`, and asserts the
//! full round-trip matches what the kernel state holds.
//!
//! Pair test across SDKs: mirror tests live in `sdks/typescript/tests/`
//! and `sdks/python/tests/` (Stage 0.1b + 0.1c).

use std::net::SocketAddr;
use std::time::Duration;

use kimberlite_client::{Client, ClientConfig, MaskingStrategySpec};
use kimberlite_test_harness::TestKimberlite;
use kimberlite_types::TenantId;
use kimberlite_wire::MaskingStrategyWire;

const MASKING_TENANT: u64 = 607;

struct TestServer {
    addr: SocketAddr,
    _harness: TestKimberlite,
}

impl TestServer {
    fn start() -> Self {
        let harness = TestKimberlite::builder()
            .tenant(MASKING_TENANT)
            .build()
            .expect("harness build");
        Self {
            addr: harness.addr(),
            _harness: harness,
        }
    }
}

/// Full CRUD round-trip exercising every step in the chain:
/// parser → executor → kernel command → state → wire read path.
#[tokio::test]
async fn masking_policy_crud_end_to_end() {
    let server = TestServer::start();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let tenant = TenantId::new(MASKING_TENANT);
    let mut client =
        Client::connect(server.addr, tenant, ClientConfig::default()).expect("sync connect");

    client
        .tenant_create(tenant, Some("masking-test".into()))
        .expect("tenant_create");

    client
        .execute(
            "CREATE TABLE patients (id BIGINT PRIMARY KEY, medicare_number TEXT)",
            &[],
        )
        .expect("create table");

    // 1. CREATE — exercise each strategy shape.
    client
        .admin()
        .masking_policy()
        .create(
            "ssn_policy",
            MaskingStrategySpec::RedactSsn,
            &["clinician", "billing"],
        )
        .expect("create REDACT_SSN policy");
    client
        .admin()
        .masking_policy()
        .create(
            "trunc_policy",
            MaskingStrategySpec::Truncate { max_chars: 4 },
            &["admin"],
        )
        .expect("create TRUNCATE policy");
    client
        .admin()
        .masking_policy()
        .create(
            "custom_policy",
            MaskingStrategySpec::RedactCustom {
                replacement: "***".into(),
            },
            &["auditor"],
        )
        .expect("create REDACT_CUSTOM policy");

    // 2. LIST — three policies, no attachments yet.
    let listing = client
        .admin()
        .masking_policy()
        .list(false)
        .expect("list policies");
    assert_eq!(listing.policies.len(), 3);
    assert!(listing.attachments.is_empty());
    // Lookups carry the strategy payload intact.
    let ssn = listing
        .policies
        .iter()
        .find(|p| p.name == "ssn_policy")
        .expect("ssn_policy present");
    match &ssn.strategy {
        MaskingStrategyWire::Redact {
            pattern,
            replacement,
        } => {
            assert_eq!(pattern, "SSN");
            assert!(replacement.is_none());
        }
        other => panic!("expected Redact strategy, got {other:?}"),
    }
    assert_eq!(ssn.exempt_roles, vec!["clinician", "billing"]);
    assert_eq!(ssn.attachment_count, 0);

    let custom = listing
        .policies
        .iter()
        .find(|p| p.name == "custom_policy")
        .unwrap();
    match &custom.strategy {
        MaskingStrategyWire::Redact {
            pattern,
            replacement,
        } => {
            assert_eq!(pattern, "CUSTOM");
            assert_eq!(replacement.as_deref(), Some("***"));
        }
        other => panic!("expected Redact Custom, got {other:?}"),
    }

    // 3. ATTACH — attachments show up with include_attachments = true.
    client
        .admin()
        .masking_policy()
        .attach("patients", "medicare_number", "ssn_policy")
        .expect("attach");

    let listing_with_atts = client
        .admin()
        .masking_policy()
        .list(true)
        .expect("list with attachments");
    let ssn = listing_with_atts
        .policies
        .iter()
        .find(|p| p.name == "ssn_policy")
        .unwrap();
    assert_eq!(ssn.attachment_count, 1);
    assert_eq!(listing_with_atts.attachments.len(), 1);
    let att = &listing_with_atts.attachments[0];
    assert_eq!(att.table_name, "patients");
    assert_eq!(att.column_name, "medicare_number");
    assert_eq!(att.policy_name, "ssn_policy");

    // 4. DROP while attached — rejected (PG-style dependency guard).
    let drop_err = client.admin().masking_policy().drop("ssn_policy");
    assert!(drop_err.is_err(), "DROP must reject while attached");

    // 5. DETACH → DROP succeeds.
    client
        .admin()
        .masking_policy()
        .detach("patients", "medicare_number")
        .expect("detach");
    client
        .admin()
        .masking_policy()
        .drop("ssn_policy")
        .expect("drop after detach");

    // 6. Final state: two policies remain, no attachments.
    let final_listing = client
        .admin()
        .masking_policy()
        .list(true)
        .expect("final list");
    assert_eq!(final_listing.policies.len(), 2);
    assert!(
        final_listing
            .policies
            .iter()
            .all(|p| p.name != "ssn_policy")
    );
    assert!(final_listing.attachments.is_empty());
}

/// Identifier validation trips at the client boundary, not the server.
/// This matters for compliance: we don't want SQL-injection-shaped
/// attempts to reach the parser where the error surface is less
/// precise.
#[tokio::test]
async fn masking_policy_create_rejects_injection_shaped_names() {
    let server = TestServer::start();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let tenant = TenantId::new(MASKING_TENANT);
    let mut client =
        Client::connect(server.addr, tenant, ClientConfig::default()).expect("sync connect");
    client
        .tenant_create(tenant, Some("masking-inj-test".into()))
        .expect("tenant_create");

    // Space in name — not a valid identifier.
    let err =
        client
            .admin()
            .masking_policy()
            .create("bad name", MaskingStrategySpec::Hash, &["admin"]);
    assert!(err.is_err());

    // Single quote in role name — rejected before hitting the server.
    let err = client.admin().masking_policy().create(
        "good_name",
        MaskingStrategySpec::Hash,
        &["admin'; DROP TABLE users; --"],
    );
    assert!(err.is_err());

    // Empty role list — rejected client-side.
    let err = client
        .admin()
        .masking_policy()
        .create("empty_roles", MaskingStrategySpec::Hash, &[]);
    assert!(err.is_err());
}
