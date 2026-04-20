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
    ConsentGrantResponse, ConsentPurpose, ConsentRecord, ConsentScope, ConsentWithdrawResponse,
    ErasureAuditInfo, ErasureExemptionBasis, ErasureRequestInfo,
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
}
