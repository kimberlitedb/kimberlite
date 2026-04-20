//! Grouped compliance namespace for the Rust SDK.
//!
//! AUDIT-2026-04 S2.5 — mirrors TS `client.compliance.consent` /
//! `client.compliance.erasure` and Python's equivalent. Flat
//! `Client::consent_xxx` / `Client::erasure_xxx` methods remain
//! available for back-compat.
//!
//! # Example
//!
//! ```no_run
//! # use kimberlite_client::{Client, ClientConfig};
//! # use kimberlite_types::TenantId;
//! # fn main() -> kimberlite_client::ClientResult<()> {
//! # let mut client = Client::connect(
//! #     "127.0.0.1:5432",
//! #     TenantId::new(1),
//! #     ClientConfig::default(),
//! # )?;
//! use kimberlite_wire::ConsentPurpose;
//!
//! client.compliance().consent().grant("alice", ConsentPurpose::Marketing, None)?;
//! let req = client.compliance().erasure().request("alice")?;
//! # Ok(()) }
//! ```

use kimberlite_types::StreamId;
use kimberlite_wire::{
    AuditEventInfo, ConsentGrantResponse, ConsentPurpose, ConsentRecord, ConsentScope,
    ConsentWithdrawResponse, ErasureAuditInfo, ErasureExemptionBasis, ErasureRequestInfo,
    ExportFormat, PortabilityExportInfo, VerifyExportResponse,
};

use crate::client::Client;
use crate::error::ClientResult;

/// Top-level `client.compliance()` entry point. Owns a `&mut
/// Client` and hands out sub-namespaces that borrow further.
pub struct ComplianceApi<'a> {
    client: &'a mut Client,
}

impl<'a> ComplianceApi<'a> {
    pub(crate) fn new(client: &'a mut Client) -> Self {
        Self { client }
    }

    /// Consent sub-namespace — GDPR Article 6 consent lifecycle.
    pub fn consent(&mut self) -> ConsentApi<'_> {
        ConsentApi { client: self.client }
    }

    /// Erasure sub-namespace — GDPR Article 17 right-to-erasure
    /// lifecycle.
    pub fn erasure(&mut self) -> ErasureApi<'_> {
        ErasureApi { client: self.client }
    }

    /// Audit sub-namespace — query the compliance audit log.
    ///
    /// AUDIT-2026-04 S3.6 — vertical-helper grouping around the
    /// existing `Client::audit_query` flat method.
    pub fn audit(&mut self) -> AuditApi<'_> {
        AuditApi { client: self.client }
    }

    /// Export sub-namespace — GDPR Article 20 portability
    /// exports and verification.
    pub fn export(&mut self) -> ExportApi<'_> {
        ExportApi { client: self.client }
    }
}

/// GDPR Article 6 consent operations.
pub struct ConsentApi<'a> {
    client: &'a mut Client,
}

impl<'a> ConsentApi<'a> {
    pub fn grant(
        &mut self,
        subject_id: &str,
        purpose: ConsentPurpose,
        scope: Option<ConsentScope>,
    ) -> ClientResult<ConsentGrantResponse> {
        self.client.consent_grant(subject_id, purpose, scope)
    }

    pub fn withdraw(&mut self, consent_id: &str) -> ClientResult<ConsentWithdrawResponse> {
        self.client.consent_withdraw(consent_id)
    }

    pub fn check(
        &mut self,
        subject_id: &str,
        purpose: ConsentPurpose,
    ) -> ClientResult<bool> {
        self.client.consent_check(subject_id, purpose)
    }

    pub fn list(
        &mut self,
        subject_id: &str,
        valid_only: bool,
    ) -> ClientResult<Vec<ConsentRecord>> {
        self.client.consent_list(subject_id, valid_only)
    }
}

/// GDPR Article 17 right-to-erasure operations.
pub struct ErasureApi<'a> {
    client: &'a mut Client,
}

impl<'a> ErasureApi<'a> {
    pub fn request(&mut self, subject_id: &str) -> ClientResult<ErasureRequestInfo> {
        self.client.erasure_request(subject_id)
    }

    pub fn status(&mut self, request_id: &str) -> ClientResult<ErasureRequestInfo> {
        self.client.erasure_status(request_id)
    }

    pub fn mark_stream_erased(
        &mut self,
        request_id: &str,
        stream_id: StreamId,
        records_erased: u64,
    ) -> ClientResult<ErasureRequestInfo> {
        self.client
            .erasure_mark_stream_erased(request_id, stream_id, records_erased)
    }

    pub fn complete(&mut self, request_id: &str) -> ClientResult<ErasureAuditInfo> {
        self.client.erasure_complete(request_id)
    }

    pub fn exempt(
        &mut self,
        request_id: &str,
        basis: ErasureExemptionBasis,
    ) -> ClientResult<ErasureRequestInfo> {
        self.client.erasure_exempt(request_id, basis)
    }

    pub fn list(&mut self) -> ClientResult<Vec<ErasureAuditInfo>> {
        self.client.erasure_list()
    }
}

/// Compliance audit-log query operations.
pub struct AuditApi<'a> {
    client: &'a mut Client,
}

impl<'a> AuditApi<'a> {
    /// Query the audit log with optional filters. Unset fields
    /// do not constrain the result set.
    ///
    /// See [`AuditQueryFilter`] for a builder-style constructor
    /// that reads cleanly at call sites.
    #[allow(clippy::too_many_arguments)]
    pub fn query(
        &mut self,
        subject_id: Option<String>,
        action_type: Option<String>,
        time_from_nanos: Option<u64>,
        time_to_nanos: Option<u64>,
        actor: Option<String>,
        limit: Option<u32>,
    ) -> ClientResult<Vec<AuditEventInfo>> {
        self.client.audit_query(
            subject_id,
            action_type,
            time_from_nanos,
            time_to_nanos,
            actor,
            limit,
        )
    }

    /// Query convenience — accepts a [`AuditQueryFilter`] builder
    /// value for clearer call sites.
    pub fn query_with(
        &mut self,
        filter: AuditQueryFilter,
    ) -> ClientResult<Vec<AuditEventInfo>> {
        self.query(
            filter.subject_id,
            filter.action_type,
            filter.time_from_nanos,
            filter.time_to_nanos,
            filter.actor,
            filter.limit,
        )
    }
}

/// Builder for audit-log query filters. All fields optional.
///
/// AUDIT-2026-04 S3.6 — clearer call sites than the 6-arg
/// `audit().query(Some("alice"), None, None, None, None, Some(100))`:
///
/// ```no_run
/// # use kimberlite_client::{Client, ClientConfig};
/// # use kimberlite_client::compliance_api::AuditQueryFilter;
/// # use kimberlite_types::TenantId;
/// # fn main() -> kimberlite_client::ClientResult<()> {
/// # let mut client = Client::connect("127.0.0.1:5432", TenantId::new(1), ClientConfig::default())?;
/// let events = client.compliance().audit().query_with(
///     AuditQueryFilter::new()
///         .subject("alice")
///         .limit(100),
/// )?;
/// # Ok(()) }
/// ```
#[derive(Debug, Default, Clone)]
pub struct AuditQueryFilter {
    pub subject_id: Option<String>,
    pub action_type: Option<String>,
    pub time_from_nanos: Option<u64>,
    pub time_to_nanos: Option<u64>,
    pub actor: Option<String>,
    pub limit: Option<u32>,
}

impl AuditQueryFilter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn subject(mut self, s: impl Into<String>) -> Self {
        self.subject_id = Some(s.into());
        self
    }

    #[must_use]
    pub fn action_type(mut self, t: impl Into<String>) -> Self {
        self.action_type = Some(t.into());
        self
    }

    #[must_use]
    pub fn time_range(mut self, from_nanos: u64, to_nanos: u64) -> Self {
        self.time_from_nanos = Some(from_nanos);
        self.time_to_nanos = Some(to_nanos);
        self
    }

    #[must_use]
    pub fn actor(mut self, a: impl Into<String>) -> Self {
        self.actor = Some(a.into());
        self
    }

    #[must_use]
    pub fn limit(mut self, n: u32) -> Self {
        self.limit = Some(n);
        self
    }
}

/// GDPR Article 20 data-portability export operations.
pub struct ExportApi<'a> {
    client: &'a mut Client,
}

impl<'a> ExportApi<'a> {
    /// Produce a signed portability export for a subject.
    ///
    /// Empty `stream_ids` means "every stream the caller can
    /// see". `max_records_per_stream` of 0 means unbounded
    /// (server-side caps still apply).
    pub fn for_subject(
        &mut self,
        subject_id: impl Into<String>,
        requester_id: impl Into<String>,
        format: ExportFormat,
        stream_ids: Vec<StreamId>,
        max_records_per_stream: u64,
    ) -> ClientResult<PortabilityExportInfo> {
        self.client.export_subject(
            subject_id,
            requester_id,
            format,
            stream_ids,
            max_records_per_stream,
        )
    }

    /// Verify the cryptographic integrity of a prior export.
    pub fn verify(
        &mut self,
        export_id: &str,
        body_base64: &str,
    ) -> ClientResult<VerifyExportResponse> {
        self.client.verify_export(export_id, body_base64)
    }
}

#[cfg(test)]
mod tests {
    //! Signature smoke test — grouped namespace exposes every
    //! method the flat API does with matching types. Never runs.

    use super::*;

    #[allow(dead_code)]
    fn _signature_trybuild(client: &mut Client) {
        // consent sub-namespace
        let mut c = client.compliance();
        let mut consent = c.consent();
        let _: ClientResult<ConsentGrantResponse> =
            consent.grant("alice", ConsentPurpose::Marketing, None);
        let _: ClientResult<ConsentWithdrawResponse> = consent.withdraw("consent-id");
        let _: ClientResult<bool> = consent.check("alice", ConsentPurpose::Marketing);
        let _: ClientResult<Vec<ConsentRecord>> = consent.list("alice", true);
    }

    #[allow(dead_code)]
    fn _signature_trybuild_erasure(client: &mut Client) {
        let mut c = client.compliance();
        let mut erasure = c.erasure();
        let _: ClientResult<ErasureRequestInfo> = erasure.request("alice");
        let _: ClientResult<ErasureRequestInfo> = erasure.status("id");
        let _: ClientResult<ErasureRequestInfo> =
            erasure.mark_stream_erased("id", StreamId::new(1), 10);
        let _: ClientResult<ErasureAuditInfo> = erasure.complete("id");
        let _: ClientResult<ErasureRequestInfo> =
            erasure.exempt("id", ErasureExemptionBasis::LegalObligation);
        let _: ClientResult<Vec<ErasureAuditInfo>> = erasure.list();
    }

    #[allow(dead_code)]
    fn _signature_trybuild_audit(client: &mut Client) {
        let mut c = client.compliance();
        let mut audit = c.audit();
        let _: ClientResult<Vec<AuditEventInfo>> =
            audit.query(Some("alice".into()), None, None, None, None, Some(100));
        let _: ClientResult<Vec<AuditEventInfo>> = audit.query_with(
            AuditQueryFilter::new()
                .subject("alice")
                .actor("bob")
                .time_range(0, u64::MAX)
                .action_type("Erasure")
                .limit(100),
        );
    }

    #[allow(dead_code)]
    fn _signature_trybuild_export(client: &mut Client) {
        let mut c = client.compliance();
        let mut export = c.export();
        let _: ClientResult<PortabilityExportInfo> = export.for_subject(
            "alice",
            "requester",
            ExportFormat::Json,
            vec![],
            0,
        );
        let _: ClientResult<VerifyExportResponse> = export.verify("export-id", "body-b64");
    }

    #[test]
    fn audit_query_filter_builder_populates_all_fields() {
        let f = AuditQueryFilter::new()
            .subject("alice")
            .action_type("Erasure")
            .time_range(100, 200)
            .actor("bob")
            .limit(50);
        assert_eq!(f.subject_id.as_deref(), Some("alice"));
        assert_eq!(f.action_type.as_deref(), Some("Erasure"));
        assert_eq!(f.time_from_nanos, Some(100));
        assert_eq!(f.time_to_nanos, Some(200));
        assert_eq!(f.actor.as_deref(), Some("bob"));
        assert_eq!(f.limit, Some(50));
    }

    #[test]
    fn audit_query_filter_default_is_all_none() {
        let f = AuditQueryFilter::default();
        assert!(f.subject_id.is_none());
        assert!(f.action_type.is_none());
        assert!(f.time_from_nanos.is_none());
        assert!(f.time_to_nanos.is_none());
        assert!(f.actor.is_none());
        assert!(f.limit.is_none());
    }
}
