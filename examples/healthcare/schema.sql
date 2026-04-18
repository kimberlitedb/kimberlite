-- Healthcare / HIPAA clinic-management schema.
--
-- Five tables. Everything here stays in the current-state projection. The
-- append-only log retains the full history, so `AS OF TIMESTAMP` and
-- `AT OFFSET` queries reconstruct any prior point in time.
--
-- Apply via examples/healthcare/00-setup.sh, or paste each statement
-- below into `kimberlite repl --tenant 1`.

-- ============================================================================
-- providers — clinicians who read/write patient data.
-- Data classification: Non-PHI (professional registry).
-- ============================================================================

CREATE TABLE IF NOT EXISTS providers (
    id BIGINT NOT NULL PRIMARY KEY,
    npi TEXT NOT NULL,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    specialty TEXT,
    department TEXT,
    active BOOLEAN,
    created_at TEXT
);

CREATE INDEX providers_specialty_idx ON providers (specialty);

-- ============================================================================
-- patients — demographic + identifying info.
-- Data classification: PHI (HIPAA-regulated).
-- ============================================================================

CREATE TABLE IF NOT EXISTS patients (
    id BIGINT NOT NULL PRIMARY KEY,
    medical_record_number TEXT NOT NULL,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    date_of_birth TEXT NOT NULL,
    ssn_last_four TEXT,
    email TEXT,
    phone TEXT,
    primary_provider_id BIGINT,
    active BOOLEAN,
    created_at TEXT,
    updated_at TEXT
);

CREATE INDEX patients_mrn_idx ON patients (medical_record_number);
CREATE INDEX patients_provider_idx ON patients (primary_provider_id);

-- ============================================================================
-- encounters — clinical visits / consults / procedures.
-- Data classification: PHI.
-- ============================================================================

CREATE TABLE IF NOT EXISTS encounters (
    id BIGINT NOT NULL PRIMARY KEY,
    patient_id BIGINT NOT NULL,
    provider_id BIGINT NOT NULL,
    encounter_type TEXT NOT NULL,     -- 'OfficeVisit' | 'Consultation' | 'Telehealth' | 'Emergency'
    encounter_date TEXT NOT NULL,
    chief_complaint TEXT,
    diagnosis_codes TEXT,             -- ICD-10, comma-separated
    notes TEXT,
    duration_minutes BIGINT,
    created_at TEXT
);

CREATE INDEX encounters_patient_idx  ON encounters (patient_id);
CREATE INDEX encounters_provider_idx ON encounters (provider_id);
CREATE INDEX encounters_date_idx     ON encounters (encounter_date);

-- ============================================================================
-- access_grants — clinician-level authorisation to view a specific patient.
-- Enforced at query time. Revoked grants are kept for the audit trail via
-- the append-only log.
-- Data classification: Non-PHI.
-- ============================================================================

CREATE TABLE IF NOT EXISTS access_grants (
    id BIGINT NOT NULL PRIMARY KEY,
    provider_id BIGINT NOT NULL,
    patient_id BIGINT NOT NULL,
    granted_at TEXT NOT NULL,
    revoked_at TEXT,
    reason TEXT                       -- 'PrimaryCare' | 'Consult' | 'Emergency' | 'Research'
);

CREATE INDEX access_grants_provider_idx ON access_grants (provider_id);
CREATE INDEX access_grants_patient_idx  ON access_grants (patient_id);

-- ============================================================================
-- audit_log — application-level audit. Every PHI read/write appends a row.
-- This is application audit (bookkeeping inside the tenant's data). The
-- immutable append-only log underneath is the primary audit source of truth.
-- Data classification: Non-PHI (references only — no patient identifiers).
-- ============================================================================

CREATE TABLE IF NOT EXISTS audit_log (
    id BIGINT NOT NULL PRIMARY KEY,
    event_at TEXT NOT NULL,
    actor_kind TEXT NOT NULL,         -- 'provider' | 'service' | 'system'
    actor_id BIGINT NOT NULL,
    action TEXT NOT NULL,             -- 'READ' | 'WRITE' | 'DELETE' | 'EXPORT'
    resource_type TEXT NOT NULL,      -- 'patient' | 'encounter' | 'consent'
    resource_id BIGINT,
    ip_address TEXT,
    user_agent TEXT,
    details TEXT
);

CREATE INDEX audit_log_actor_idx    ON audit_log (actor_id);
CREATE INDEX audit_log_resource_idx ON audit_log (resource_type, resource_id);
CREATE INDEX audit_log_date_idx     ON audit_log (event_at);
