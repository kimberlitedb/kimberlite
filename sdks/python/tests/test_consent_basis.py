"""v0.6.0 Tier 1 #2 — ConsentBasis round-trip tests for the Python SDK.

Exercises :class:`kimberlite.compliance.ConsentBasis` and
:func:`kimberlite.compliance._parse_consent_record` without a live
server. The full cross-SDK parity — that a grant with `basis` travels
over the wire and shows up on `list` — is covered by the Rust-side
`consent_basis.rs` integration test; here we prove the Python SDK's
serde layer correctly encodes/decodes the field.
"""

from kimberlite.compliance import (
    ConsentBasis,
    ConsentRecord,
    _parse_consent_record,
)


def test_consent_basis_is_exported():
    """The top-level package re-export matters for consumer ergonomics."""
    import kimberlite

    assert kimberlite.ConsentBasis is ConsentBasis


def test_consent_basis_constructor_accepts_article_and_justification():
    basis = ConsentBasis(article="Consent", justification="opt-in at signup")
    assert basis.article == "Consent"
    assert basis.justification == "opt-in at signup"


def test_consent_basis_justification_defaults_to_none():
    basis = ConsentBasis(article="LegalObligation")
    assert basis.article == "LegalObligation"
    assert basis.justification is None


def test_parse_consent_record_with_basis_populates_field():
    raw = {
        "consent_id": "consent-uuid-1",
        "subject_id": "alice",
        "purpose": "Research",
        "scope": "AllData",
        "granted_at_nanos": 1_700_000_000_000_000_000,
        "withdrawn_at_nanos": None,
        "expires_at_nanos": None,
        "notes": None,
        "basis": {
            "article": "Consent",
            "justification": "clinical research opt-in",
        },
    }
    rec = _parse_consent_record(raw)
    assert isinstance(rec, ConsentRecord)
    assert rec.basis is not None
    assert rec.basis.article == "Consent"
    assert rec.basis.justification == "clinical research opt-in"


def test_parse_consent_record_without_basis_yields_none():
    # Pre-v4 server payload: no `basis` key at all.
    raw = {
        "consent_id": "consent-uuid-2",
        "subject_id": "bob",
        "purpose": "Marketing",
        "scope": "AllData",
        "granted_at_nanos": 1_700_000_000_000_000_000,
        "withdrawn_at_nanos": None,
        "expires_at_nanos": None,
        "notes": None,
    }
    rec = _parse_consent_record(raw)
    assert rec.basis is None


def test_parse_consent_record_with_null_basis_yields_none():
    # v4 server explicitly returning `"basis": null` must also map to None.
    raw = {
        "consent_id": "consent-uuid-3",
        "subject_id": "carol",
        "purpose": "Analytics",
        "scope": "AllData",
        "granted_at_nanos": 1_700_000_000_000_000_000,
        "withdrawn_at_nanos": None,
        "expires_at_nanos": None,
        "notes": None,
        "basis": None,
    }
    rec = _parse_consent_record(raw)
    assert rec.basis is None


def test_parse_consent_record_basis_with_null_justification():
    # Article 6(1)(c) / (d) / (e) can be justified by the lettered
    # basis alone — `justification` may legitimately be null.
    raw = {
        "consent_id": "consent-uuid-4",
        "subject_id": "dave",
        "purpose": "Security",
        "scope": "AllData",
        "granted_at_nanos": 1_700_000_000_000_000_000,
        "withdrawn_at_nanos": None,
        "expires_at_nanos": None,
        "notes": None,
        "basis": {"article": "LegalObligation", "justification": None},
    }
    rec = _parse_consent_record(raw)
    assert rec.basis is not None
    assert rec.basis.article == "LegalObligation"
    assert rec.basis.justification is None


# --- v0.6.2 — terms_version + accepted ------------------------------


def test_parse_consent_record_with_terms_version_and_accepted():
    raw = {
        "consent_id": "consent-uuid-5",
        "subject_id": "erin",
        "purpose": "Marketing",
        "scope": "AllData",
        "granted_at_nanos": 1_700_000_000_000_000_000,
        "withdrawn_at_nanos": None,
        "expires_at_nanos": None,
        "notes": None,
        "basis": None,
        "terms_version": "2026-04-tos",
        "accepted": True,
    }
    rec = _parse_consent_record(raw)
    assert rec.terms_version == "2026-04-tos"
    assert rec.accepted is True


def test_parse_consent_record_with_explicit_decline():
    """An explicit decline (`accepted=False`) must round-trip — it is
    itself a compliance event, not just a missing acceptance."""
    raw = {
        "consent_id": "consent-uuid-6",
        "subject_id": "frank",
        "purpose": "Analytics",
        "scope": "AllData",
        "granted_at_nanos": 1_700_000_000_000_000_000,
        "withdrawn_at_nanos": None,
        "expires_at_nanos": None,
        "notes": None,
        "basis": None,
        "terms_version": "v3",
        "accepted": False,
    }
    rec = _parse_consent_record(raw)
    assert rec.terms_version == "v3"
    assert rec.accepted is False


def test_parse_consent_record_pre_v062_payload_defaults_to_accepted_true():
    """Pre-v0.6.2 servers don't emit `terms_version` / `accepted` keys.
    The dataclass defaults preserve v0.6.1 acceptance-only semantics —
    parse must report `terms_version=None` and `accepted=True`."""
    raw = {
        "consent_id": "consent-uuid-7",
        "subject_id": "gail",
        "purpose": "Marketing",
        "scope": "AllData",
        "granted_at_nanos": 1_700_000_000_000_000_000,
        "withdrawn_at_nanos": None,
        "expires_at_nanos": None,
        "notes": None,
        # no basis, no terms_version, no accepted
    }
    rec = _parse_consent_record(raw)
    assert rec.terms_version is None
    assert rec.accepted is True


def test_consent_record_dataclass_default_for_accepted_is_true():
    """The dataclass default for `accepted` matches the parser default —
    constructing a record without the field yields an acceptance."""
    rec = ConsentRecord(
        consent_id="x",
        subject_id="y",
        purpose="Marketing",
        scope="AllData",
        granted_at_nanos=0,
        withdrawn_at_nanos=None,
        expires_at_nanos=None,
        notes=None,
    )
    assert rec.terms_version is None
    assert rec.accepted is True
