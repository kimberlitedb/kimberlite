//! v0.6.0 Tier 1 #2 — wire protocol v4 end-to-end round-trip test
//! for the GDPR Article 6(1) lawful basis threaded through
//! `consent.grant` → `consent.list`.
//!
//! Spins up an in-process server via `kimberlite-test-harness`,
//! grants two consents (one with `basis = Some`, one without), then
//! asserts that `consent_list` returns both records with `basis`
//! round-tripped intact. This is the Rust half of the cross-SDK
//! parity gate (mirror tests live in `sdks/typescript/tests/` and
//! `sdks/python/tests/`).

use std::net::SocketAddr;
use std::time::Duration;

use kimberlite_client::{Client, ClientConfig};
use kimberlite_test_harness::TestKimberlite;
use kimberlite_types::TenantId;
use kimberlite_wire::{ConsentBasis, ConsentPurpose, GdprArticle};

const BASIS_TENANT: u64 = 606;
const SUBJECT_WITH_BASIS: &str = "subject:with-basis";
const SUBJECT_WITHOUT_BASIS: &str = "subject:without-basis";

struct TestServer {
    addr: SocketAddr,
    _harness: TestKimberlite,
}

impl TestServer {
    fn start() -> Self {
        let harness = TestKimberlite::builder()
            .tenant(BASIS_TENANT)
            .build()
            .expect("harness build");
        Self {
            addr: harness.addr(),
            _harness: harness,
        }
    }
}

#[tokio::test]
async fn consent_basis_roundtrips_through_grant_and_list() {
    let server = TestServer::start();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let tenant = TenantId::new(BASIS_TENANT);
    let mut client =
        Client::connect(server.addr, tenant, ClientConfig::default()).expect("sync connect");

    client
        .tenant_create(tenant, Some("basis-test-tenant".into()))
        .expect("tenant_create");

    // Grant with an explicit GDPR Article 6(1)(a) basis + free-form
    // justification. This is the regulated-industry shape.
    let basis = ConsentBasis {
        article: GdprArticle::Consent,
        justification: Some("patient opt-in at the front desk".into()),
    };
    let with_basis = client
        .consent_grant(
            SUBJECT_WITH_BASIS,
            ConsentPurpose::Research,
            None,
            Some(basis.clone()),
        )
        .expect("consent_grant with basis");
    assert!(!with_basis.consent_id.is_empty());

    // Grant without a basis — must remain backwards-compatible.
    let without_basis = client
        .consent_grant(SUBJECT_WITHOUT_BASIS, ConsentPurpose::Analytics, None, None)
        .expect("consent_grant without basis");
    assert!(!without_basis.consent_id.is_empty());

    // List for the `with_basis` subject and assert the basis
    // round-tripped bytewise.
    let records_with = client
        .consent_list(SUBJECT_WITH_BASIS, false)
        .expect("consent_list with-basis");
    assert_eq!(records_with.len(), 1, "exactly one consent granted");
    let recorded_basis = records_with[0]
        .basis
        .as_ref()
        .expect("basis must persist on record");
    assert_eq!(recorded_basis.article, GdprArticle::Consent);
    assert_eq!(
        recorded_basis.justification.as_deref(),
        Some("patient opt-in at the front desk"),
        "justification string must round-trip verbatim"
    );

    // List for the other subject and assert `basis = None`
    // stays `None` — no accidental defaulting.
    let records_without = client
        .consent_list(SUBJECT_WITHOUT_BASIS, false)
        .expect("consent_list without-basis");
    assert_eq!(records_without.len(), 1);
    assert!(
        records_without[0].basis.is_none(),
        "basis must remain None when not supplied on grant"
    );
}

#[tokio::test]
async fn consent_basis_with_null_justification_roundtrips() {
    // Regression guard: `basis.justification = None` is a valid
    // shape (the lettered article alone carries legal weight for
    // Article 6(1)(c/d/e) — legal obligation, vital interests, public
    // task). The wire encoding of `Option<Option<String>>` must not
    // collapse `Some(None)` into `None`.
    let server = TestServer::start();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let tenant = TenantId::new(BASIS_TENANT);
    let mut client =
        Client::connect(server.addr, tenant, ClientConfig::default()).expect("sync connect");
    client
        .tenant_create(tenant, Some("basis-test-null-just".into()))
        .expect("tenant_create");

    let basis = ConsentBasis {
        article: GdprArticle::LegalObligation,
        justification: None,
    };
    client
        .consent_grant(
            "subject:legal-obligation",
            ConsentPurpose::Security,
            None,
            Some(basis),
        )
        .expect("consent_grant");

    let records = client
        .consent_list("subject:legal-obligation", false)
        .expect("consent_list");
    let basis = records[0]
        .basis
        .as_ref()
        .expect("basis is Some even when justification is None");
    assert_eq!(basis.article, GdprArticle::LegalObligation);
    assert!(basis.justification.is_none());
}
