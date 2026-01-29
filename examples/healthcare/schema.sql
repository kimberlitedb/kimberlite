-- Healthcare / HIPAA Schema
-- Demonstrates data classification and audit-ready design

-- ============================================
-- PATIENTS TABLE (PHI - Protected Health Info)
-- ============================================
-- Contains directly identifiable patient information
-- Data classification: PHI

CREATE TABLE patients (
  id BIGINT NOT NULL,
  medical_record_number TEXT NOT NULL,
  first_name TEXT NOT NULL,
  last_name TEXT NOT NULL,
  date_of_birth TEXT NOT NULL,
  ssn_last_four TEXT,
  email TEXT,
  phone TEXT,
  address_line1 TEXT,
  address_city TEXT,
  address_state TEXT,
  address_zip TEXT,
  created_at TIMESTAMP,
  updated_at TIMESTAMP
);

-- ============================================
-- ENCOUNTERS TABLE (PHI)
-- ============================================
-- Clinical encounters / visits
-- Data classification: PHI

CREATE TABLE encounters (
  id BIGINT NOT NULL,
  patient_id BIGINT NOT NULL,
  provider_id BIGINT NOT NULL,
  encounter_type TEXT NOT NULL,
  encounter_date TEXT NOT NULL,
  chief_complaint TEXT,
  diagnosis_codes TEXT,
  notes TEXT,
  created_at TIMESTAMP
);

-- ============================================
-- PROVIDERS TABLE (Non-PHI)
-- ============================================
-- Healthcare provider information
-- Data classification: Non-PHI

CREATE TABLE providers (
  id BIGINT NOT NULL,
  npi TEXT NOT NULL,
  first_name TEXT NOT NULL,
  last_name TEXT NOT NULL,
  specialty TEXT,
  department TEXT,
  active BOOLEAN
);

-- ============================================
-- AUDIT LOG (Non-PHI Metadata)
-- ============================================
-- Records all data access for compliance
-- Data classification: Non-PHI (contains only references)

CREATE TABLE audit_log (
  id BIGINT NOT NULL,
  timestamp TEXT NOT NULL,
  user_id TEXT NOT NULL,
  action TEXT NOT NULL,
  resource_type TEXT NOT NULL,
  resource_id BIGINT,
  ip_address TEXT,
  user_agent TEXT,
  details TEXT
);

-- ============================================
-- SAMPLE DATA
-- ============================================

-- Providers
INSERT INTO providers (id, npi, first_name, last_name, specialty, department, active)
VALUES (1, '1234567890', 'Sarah', 'Williams', 'Internal Medicine', 'Primary Care', TRUE);

INSERT INTO providers (id, npi, first_name, last_name, specialty, department, active)
VALUES (2, '0987654321', 'Michael', 'Chen', 'Cardiology', 'Cardiology', TRUE);

-- Patients (Example PHI - use fake data only!)
INSERT INTO patients (id, medical_record_number, first_name, last_name, date_of_birth, email, address_city, address_state)
VALUES (1, 'MRN-001234', 'Jane', 'Doe', '1985-03-15', 'jane.doe@example.com', 'Boston', 'MA');

INSERT INTO patients (id, medical_record_number, first_name, last_name, date_of_birth, email, address_city, address_state)
VALUES (2, 'MRN-005678', 'John', 'Smith', '1972-08-22', 'john.smith@example.com', 'Cambridge', 'MA');

-- Encounters
INSERT INTO encounters (id, patient_id, provider_id, encounter_type, encounter_date, chief_complaint, diagnosis_codes)
VALUES (1, 1, 1, 'Office Visit', '2024-01-15', 'Annual physical examination', 'Z00.00');

INSERT INTO encounters (id, patient_id, provider_id, encounter_type, encounter_date, chief_complaint, diagnosis_codes)
VALUES (2, 1, 2, 'Consultation', '2024-01-20', 'Chest pain evaluation', 'R07.9');

-- Audit entries
INSERT INTO audit_log (id, timestamp, user_id, action, resource_type, resource_id, ip_address)
VALUES (1, '2024-01-15T09:00:00Z', 'provider:1', 'READ', 'patient', 1, '10.0.0.1');

INSERT INTO audit_log (id, timestamp, user_id, action, resource_type, resource_id, ip_address)
VALUES (2, '2024-01-15T09:05:00Z', 'provider:1', 'WRITE', 'encounter', 1, '10.0.0.1');

INSERT INTO audit_log (id, timestamp, user_id, action, resource_type, resource_id, ip_address)
VALUES (3, '2024-01-20T14:30:00Z', 'provider:2', 'READ', 'patient', 1, '10.0.0.2');
