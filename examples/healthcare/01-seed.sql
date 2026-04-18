-- Seed data for the clinic example.
--
-- Safe to re-run: every INSERT is idempotent on the (id) primary key, so
-- re-running the script returns a clean "OffsetMismatch" on duplicate ids if
-- you haven't reset the dev database. Run `kimberlite init --force` or drop
-- the data dir to start fresh.

-- ============================================================================
-- Providers — two PCPs, two specialists
-- ============================================================================

INSERT INTO providers (id, npi, first_name, last_name, specialty, department, active, created_at)
VALUES
    (1, '1234567890', 'Sarah',   'Williams', 'Internal Medicine', 'Primary Care', TRUE, '2023-01-01 00:00:00'),
    (2, '0987654321', 'Michael', 'Chen',     'Cardiology',        'Cardiology',   TRUE, '2023-02-15 00:00:00'),
    (3, '5551234567', 'Priya',   'Patel',    'Endocrinology',     'Endocrinology',TRUE, '2023-04-01 00:00:00'),
    (4, '4449876543', 'David',   'Ng',       'Family Medicine',   'Primary Care', TRUE, '2023-06-10 00:00:00');

-- ============================================================================
-- Patients — fake, fabricated demographics.  DO NOT use real PHI in examples.
-- ============================================================================

INSERT INTO patients (id, medical_record_number, first_name, last_name, date_of_birth,
                      ssn_last_four, email, phone, primary_provider_id, active,
                      created_at, updated_at)
VALUES
    (1, 'MRN-001234', 'Jane',  'Doe',     '1985-03-15 00:00:00', '0001', 'jane.doe@example.com',   '617-555-0101', 1, TRUE, '2024-01-01 00:00:00', '2024-01-01 00:00:00'),
    (2, 'MRN-005678', 'John',  'Smith',   '1972-08-22 00:00:00', '0002', 'john.smith@example.com', '617-555-0102', 2, TRUE, '2024-01-02 00:00:00', '2024-01-02 00:00:00'),
    (3, 'MRN-009999', 'Alice', 'Johnson', '1990-11-05 00:00:00', '0003', 'alice.j@example.com',    '617-555-0103', 1, TRUE, '2024-01-05 00:00:00', '2024-01-05 00:00:00'),
    (4, 'MRN-013571', 'Bob',   'Williams','1958-04-17 00:00:00', '0004', 'bob.w@example.com',      '617-555-0104', 4, TRUE, '2024-01-10 00:00:00', '2024-01-10 00:00:00');

-- ============================================================================
-- Access grants — who is authorised to see whom
-- ============================================================================

INSERT INTO access_grants (id, provider_id, patient_id, granted_at, reason)
VALUES
    (1, 1, 1, '2024-01-01 00:00:00', 'PrimaryCare'),
    (2, 2, 1, '2024-01-20 00:00:00', 'Consult'),
    (3, 2, 2, '2024-01-02 00:00:00', 'PrimaryCare'),
    (4, 1, 3, '2024-01-05 00:00:00', 'PrimaryCare'),
    (5, 3, 3, '2024-03-01 00:00:00', 'Consult'),
    (6, 4, 4, '2024-01-10 00:00:00', 'PrimaryCare');

-- ============================================================================
-- Encounters — a few visits per patient
-- ============================================================================

INSERT INTO encounters (id, patient_id, provider_id, encounter_type, encounter_date,
                        chief_complaint, diagnosis_codes, notes, duration_minutes, created_at)
VALUES
    (1, 1, 1, 'OfficeVisit',  '2024-01-15 09:00:00', 'Annual physical examination', 'Z00.00',      'Routine exam, all normal.',  30, '2024-01-15 09:35:00'),
    (2, 1, 2, 'Consultation', '2024-01-20 14:30:00', 'Chest pain evaluation',       'R07.9',       'Stress test scheduled.',     45, '2024-01-20 15:20:00'),
    (3, 2, 2, 'OfficeVisit',  '2024-01-22 10:00:00', 'Atrial fibrillation follow-up','I48.0',      'Medication adjusted.',       30, '2024-01-22 10:35:00'),
    (4, 3, 1, 'Telehealth',   '2024-02-10 11:00:00', 'Medication review',           'Z79.899',     'Refills approved.',          15, '2024-02-10 11:20:00'),
    (5, 3, 3, 'Consultation', '2024-03-05 13:00:00', 'Type 2 diabetes management',  'E11.9',       'A1c 7.2, diet counseling.',  45, '2024-03-05 14:00:00'),
    (6, 4, 4, 'OfficeVisit',  '2024-03-12 08:30:00', 'Hypertension follow-up',      'I10',         'BP 142/90, increase dose.',  20, '2024-03-12 09:00:00');

-- ============================================================================
-- A small seed of audit events so the 02-audit-queries.sql has something to
-- chew on.  Production audit entries are written by the application on every
-- PHI access — see clinic.ts / clinic.py / clinic.rs.
-- ============================================================================

INSERT INTO audit_log (id, event_at, actor_kind, actor_id, action, resource_type, resource_id, ip_address, details)
VALUES
    (1, '2024-01-15 09:00:00', 'provider', 1, 'READ',  'patient',   1, '10.0.0.11', 'Pre-visit chart review'),
    (2, '2024-01-15 09:35:00', 'provider', 1, 'WRITE', 'encounter', 1, '10.0.0.11', 'Visit note saved'),
    (3, '2024-01-20 14:30:00', 'provider', 2, 'READ',  'patient',   1, '10.0.0.22', 'Consult prep'),
    (4, '2024-01-20 15:20:00', 'provider', 2, 'WRITE', 'encounter', 2, '10.0.0.22', 'Consult note saved'),
    (5, '2024-01-22 10:00:00', 'provider', 2, 'READ',  'patient',   2, '10.0.0.22', 'Routine follow-up'),
    (6, '2024-02-10 11:00:00', 'provider', 1, 'READ',  'patient',   3, '10.0.0.11', 'Telehealth intake'),
    (7, '2024-03-05 13:00:00', 'provider', 3, 'READ',  'patient',   3, '10.0.0.33', 'Endo consult prep');
