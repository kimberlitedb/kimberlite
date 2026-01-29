-- Healthcare Audit Queries
-- Useful queries for compliance and audit purposes

-- ============================================
-- PATIENT ACCESS AUDIT
-- ============================================

-- Who accessed a specific patient's records?
SELECT * FROM audit_log
WHERE resource_type = 'patient' AND resource_id = 1;

-- All patient data access in a time range
-- (Note: string comparison for dates until BETWEEN is supported)
SELECT * FROM audit_log
WHERE resource_type = 'patient'
  AND action = 'READ';

-- ============================================
-- PROVIDER ACTIVITY
-- ============================================

-- All activity by a specific provider
SELECT * FROM audit_log
WHERE user_id = 'provider:1';

-- Write operations (potential PHI modifications)
SELECT * FROM audit_log
WHERE action = 'WRITE';

-- ============================================
-- ENCOUNTER HISTORY
-- ============================================

-- All encounters for a patient
SELECT * FROM encounters
WHERE patient_id = 1;

-- Encounters by provider
SELECT * FROM encounters
WHERE provider_id = 1;

-- ============================================
-- DATA INVENTORY
-- ============================================

-- List all patients (for data inventory)
SELECT id, medical_record_number, first_name, last_name
FROM patients;

-- Active providers
SELECT id, npi, first_name, last_name, specialty
FROM providers
WHERE active = TRUE;

-- ============================================
-- TIME TRAVEL QUERIES (COMPLIANCE)
-- ============================================

-- To see historical state, use --at flag:
-- kimberlite query --at <position> "SELECT * FROM patients WHERE id = 1"

-- This allows:
-- 1. Reconstructing state at any point in time
-- 2. Investigating what data looked like during an incident
-- 3. Compliance audits requiring historical snapshots
