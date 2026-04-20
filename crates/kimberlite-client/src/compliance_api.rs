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
        ConsentApi {
            client: self.client,
        }
    }

    /// Erasure sub-namespace — GDPR Article 17 right-to-erasure
    /// lifecycle.
    pub fn erasure(&mut self) -> ErasureApi<'_> {
        ErasureApi {
            client: self.client,
        }
    }

    /// Audit sub-namespace — query the compliance audit log.
    ///
    /// AUDIT-2026-04 S3.6 — vertical-helper grouping around the
    /// existing `Client::audit_query` flat method.
    pub fn audit(&mut self) -> AuditApi<'_> {
        AuditApi {
            client: self.client,
        }
    }

    /// Export sub-namespace — GDPR Article 20 portability
    /// exports and verification.
    pub fn export(&mut self) -> ExportApi<'_> {
        ExportApi {
            client: self.client,
        }
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

    pub fn check(&mut self, subject_id: &str, purpose: ConsentPurpose) -> ClientResult<bool> {
        self.client.consent_check(subject_id, purpose)
    }

    pub fn list(&mut self, subject_id: &str, valid_only: bool) -> ClientResult<Vec<ConsentRecord>> {
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

    // --- AUDIT-2026-04 S4.3 typed state-machine surface ------------------

    /// Mark an erasure request as in-progress. Mirrors the
    /// TypeScript SDK's `markProgress` — the server-side state
    /// machine requires a `Pending → InProgress` transition before
    /// per-stream marks are accepted.
    pub fn mark_progress(
        &mut self,
        request_id: &str,
        stream_ids: Vec<StreamId>,
    ) -> ClientResult<ErasureRequestInfo> {
        self.client.erasure_mark_progress(request_id, stream_ids)
    }

    /// Open a typed erasure request and return a
    /// [`ErasureRequest<Pending>`] token. The type system enforces
    /// that callers transition through [`Self::mark_progress_typed`]
    /// before recording per-stream progress.
    pub fn request_typed(&mut self, subject_id: &str) -> ClientResult<ErasureRequest<Pending>> {
        let info = self.request(subject_id)?;
        Ok(ErasureRequest::new(info))
    }

    /// Transition `Pending` → `InProgress` with the set of streams
    /// affected by this erasure.
    pub fn mark_progress_typed(
        &mut self,
        token: ErasureRequest<Pending>,
        stream_ids: Vec<StreamId>,
    ) -> ClientResult<ErasureRequest<InProgress>> {
        let info = self.mark_progress(token.info.request_id.as_str(), stream_ids)?;
        Ok(ErasureRequest::new(info))
    }

    /// Record per-stream progress — valid only in `InProgress` or
    /// `Recording`. The typed token rules out calling this on a
    /// `Pending` request.
    pub fn mark_stream_erased_typed<S: InProgressOrRecording>(
        &mut self,
        token: ErasureRequest<S>,
        stream_id: StreamId,
        records_erased: u64,
    ) -> ClientResult<ErasureRequest<Recording>> {
        let info =
            self.mark_stream_erased(token.info.request_id.as_str(), stream_id, records_erased)?;
        Ok(ErasureRequest::new(info))
    }

    /// Finalise an in-progress erasure, returning the signed audit
    /// attestation.
    pub fn complete_typed<S: InProgressOrRecording>(
        &mut self,
        token: ErasureRequest<S>,
    ) -> ClientResult<ErasureAuditInfo> {
        self.complete(token.info.request_id.as_str())
    }

    /// AUDIT-2026-04 S4.4 — one-call orchestrator that chains open →
    /// enumerate streams → mark progress → per-stream erased →
    /// complete. Mirrors the TS and Python [`erase_subject`]
    /// helpers.
    ///
    /// `on_stream` is a caller-supplied callback that performs the
    /// actual redaction for a stream and returns the records-erased
    /// count. Pass `None` to skip per-stream redaction (the server
    /// still records the transition).
    pub fn erase_subject(
        &mut self,
        subject_id: &str,
        mut on_stream: Option<Box<dyn FnMut(StreamId) -> ClientResult<u64>>>,
    ) -> ClientResult<ErasureAuditInfo> {
        let pending = self.request_typed(subject_id)?;
        let streams: Vec<StreamId> = pending.info.streams_affected.clone();
        let in_progress = self.mark_progress_typed(pending, streams.clone())?;
        let mut recording = ErasureRecordingInner::InProgress(in_progress);
        for sid in streams {
            let erased = match on_stream.as_mut() {
                Some(cb) => cb(sid)?,
                None => 0,
            };
            let next = match recording {
                ErasureRecordingInner::InProgress(t) => {
                    self.mark_stream_erased_typed(t, sid, erased)?
                }
                ErasureRecordingInner::Recording(t) => {
                    self.mark_stream_erased_typed(t, sid, erased)?
                }
            };
            recording = ErasureRecordingInner::Recording(next);
        }
        match recording {
            ErasureRecordingInner::InProgress(t) => self.complete_typed(t),
            ErasureRecordingInner::Recording(t) => self.complete_typed(t),
        }
    }
}

/// Sealed phantom-state marker for [`ErasureRequest<S>`]. One of
/// [`Pending`], [`InProgress`], [`Recording`].
pub trait ErasureState: sealed::Sealed {}

/// Phantom states that accept `mark_stream_erased_typed` /
/// `complete_typed` — namely [`InProgress`] and [`Recording`].
pub trait InProgressOrRecording: ErasureState {}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Pending {}
    impl Sealed for super::InProgress {}
    impl Sealed for super::Recording {}
}

/// Erasure request just opened — must call
/// [`ErasureApi::mark_progress_typed`] before anything else.
pub enum Pending {}

/// Erasure request moved to `InProgress` — ready for per-stream
/// progress marks.
pub enum InProgress {}

/// Erasure request with at least one stream marked erased.
pub enum Recording {}

impl ErasureState for Pending {}
impl ErasureState for InProgress {}
impl ErasureState for Recording {}
impl InProgressOrRecording for InProgress {}
impl InProgressOrRecording for Recording {}

/// Typed wrapper around [`ErasureRequestInfo`] that enforces the
/// state-machine transitions at compile time. The `S` phantom
/// parameter is one of [`Pending`], [`InProgress`], [`Recording`].
pub struct ErasureRequest<S: ErasureState> {
    /// The underlying wire record, including request id, subject,
    /// streams affected, etc.
    pub info: ErasureRequestInfo,
    _state: std::marker::PhantomData<S>,
}

impl<S: ErasureState> ErasureRequest<S> {
    fn new(info: ErasureRequestInfo) -> Self {
        Self {
            info,
            _state: std::marker::PhantomData,
        }
    }
}

enum ErasureRecordingInner {
    InProgress(ErasureRequest<InProgress>),
    Recording(ErasureRequest<Recording>),
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
    pub fn query_with(&mut self, filter: AuditQueryFilter) -> ClientResult<Vec<AuditEventInfo>> {
        self.query(
            filter.subject_id,
            filter.action_type,
            filter.time_from_nanos,
            filter.time_to_nanos,
            filter.actor,
            filter.limit,
        )
    }

    /// AUDIT-2026-04 S3.6 — generate a structured compliance
    /// report from the audit log.
    ///
    /// Wraps [`Self::query_with`] and pre-aggregates counts by
    /// action kind and actor — the shape a HIPAA/GDPR auditor
    /// wants at a glance. The raw events are preserved on
    /// [`AuditReport::events`] for detail rendering. See
    /// [`AuditReport::to_markdown`] for a regulator-friendly
    /// string renderer.
    pub fn generate_report(
        &mut self,
        from_nanos: u64,
        to_nanos: u64,
        subject_id: Option<String>,
    ) -> ClientResult<AuditReport> {
        let mut filter = AuditQueryFilter::new().time_range(from_nanos, to_nanos);
        if let Some(s) = subject_id.clone() {
            filter = filter.subject(s);
        }
        let events = self.query_with(filter)?;
        let mut by_action_kind: std::collections::BTreeMap<String, usize> = Default::default();
        let mut by_actor: std::collections::BTreeMap<String, usize> = Default::default();
        for e in &events {
            *by_action_kind.entry(e.action_kind.clone()).or_default() += 1;
            if let Some(a) = &e.actor {
                *by_actor.entry(a.clone()).or_default() += 1;
            }
        }
        Ok(AuditReport {
            from_nanos,
            to_nanos,
            subject_id,
            total_events: events.len(),
            by_action_kind,
            by_actor,
            events,
        })
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

/// Structured audit-report summary — produced by
/// `AuditApi::generate_report` from a set of audit events.
///
/// AUDIT-2026-04 S3.6 — gives compliance teams a single-shot
/// call that returns a regulator-ready summary rather than a raw
/// event list. The report can be rendered to Markdown / JSON /
/// PDF at the call site.
#[derive(Debug, Clone)]
pub struct AuditReport {
    /// Inclusive start of the reporting window (Unix ns).
    pub from_nanos: u64,
    /// Inclusive end of the reporting window (Unix ns).
    pub to_nanos: u64,
    /// Subject filter (None = all subjects in the window).
    pub subject_id: Option<String>,
    /// Total event count in the window.
    pub total_events: usize,
    /// Events grouped by `action_kind` (e.g. "ConsentGranted",
    /// "ErasureCompleted"). Value is the count.
    pub by_action_kind: std::collections::BTreeMap<String, usize>,
    /// Events grouped by actor (e.g. user email). Value is count.
    pub by_actor: std::collections::BTreeMap<String, usize>,
    /// The underlying events, untouched, for rendering at the
    /// call site.
    pub events: Vec<AuditEventInfo>,
}

impl AuditReport {
    /// Render the report as a regulator-friendly Markdown string.
    pub fn to_markdown(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = writeln!(out, "# Compliance Audit Report");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "- Window: `{}` → `{}` (Unix ns)",
            self.from_nanos, self.to_nanos,
        );
        if let Some(s) = &self.subject_id {
            let _ = writeln!(out, "- Subject: `{s}`");
        }
        let _ = writeln!(out, "- Total events: **{}**", self.total_events);
        let _ = writeln!(out);

        let _ = writeln!(out, "## Events by action kind");
        for (kind, count) in &self.by_action_kind {
            let _ = writeln!(out, "- `{kind}`: {count}");
        }
        let _ = writeln!(out);

        let _ = writeln!(out, "## Events by actor");
        for (actor, count) in &self.by_actor {
            let _ = writeln!(out, "- `{actor}`: {count}");
        }
        out
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
        let _: ClientResult<PortabilityExportInfo> =
            export.for_subject("alice", "requester", ExportFormat::Json, vec![], 0);
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
    fn audit_report_markdown_renders_sections() {
        use std::collections::BTreeMap;
        let r = AuditReport {
            from_nanos: 100,
            to_nanos: 200,
            subject_id: Some("alice".into()),
            total_events: 5,
            by_action_kind: BTreeMap::from([
                ("ConsentGranted".to_string(), 3),
                ("ErasureCompleted".to_string(), 2),
            ]),
            by_actor: BTreeMap::from([
                ("admin@example.com".to_string(), 4),
                ("system".to_string(), 1),
            ]),
            events: Vec::new(),
        };
        let md = r.to_markdown();
        assert!(md.contains("# Compliance Audit Report"));
        assert!(md.contains("Total events: **5**"));
        assert!(md.contains("ConsentGranted"));
        assert!(md.contains("ErasureCompleted"));
        assert!(md.contains("admin@example.com"));
        assert!(md.contains("100"));
        assert!(md.contains("200"));
    }

    #[test]
    fn audit_report_handles_empty_event_list() {
        use std::collections::BTreeMap;
        let r = AuditReport {
            from_nanos: 0,
            to_nanos: 0,
            subject_id: None,
            total_events: 0,
            by_action_kind: BTreeMap::new(),
            by_actor: BTreeMap::new(),
            events: Vec::new(),
        };
        let md = r.to_markdown();
        assert!(md.contains("Total events: **0**"));
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
