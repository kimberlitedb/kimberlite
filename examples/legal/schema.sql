-- Legal / Chain of Custody Schema
-- Demonstrates immutable evidence tracking and legal hold management

-- ============================================
-- CASES TABLE (Restricted)
-- ============================================
-- Legal case metadata
-- Data classification: Restricted

CREATE TABLE cases (
  id BIGINT NOT NULL,
  case_number TEXT NOT NULL,
  case_type TEXT NOT NULL,
  title TEXT NOT NULL,
  status TEXT NOT NULL,
  lead_attorney TEXT NOT NULL,
  client_name TEXT NOT NULL,
  opposing_party TEXT,
  court TEXT,
  filed_date TEXT,
  closed_date TEXT,
  created_at TIMESTAMP,
  PRIMARY KEY (id)
);

-- ============================================
-- DOCUMENTS TABLE (Privileged)
-- ============================================
-- Evidence documents and exhibits
-- Data classification: Attorney-Client Privileged

CREATE TABLE documents (
  id BIGINT NOT NULL,
  case_id BIGINT NOT NULL,
  document_type TEXT NOT NULL,
  title TEXT NOT NULL,
  classification TEXT NOT NULL,
  file_hash TEXT NOT NULL,
  file_size BIGINT,
  source TEXT NOT NULL,
  collected_by TEXT NOT NULL,
  collected_date TEXT NOT NULL,
  privilege_status TEXT,
  review_status TEXT NOT NULL,
  notes TEXT,
  created_at TIMESTAMP,
  PRIMARY KEY (id)
);

-- ============================================
-- CUSTODY LOG (Restricted)
-- ============================================
-- Immutable chain of custody records
-- Data classification: Restricted (legal proceeding)

CREATE TABLE custody_log (
  id BIGINT NOT NULL,
  document_id BIGINT NOT NULL,
  case_id BIGINT NOT NULL,
  action TEXT NOT NULL,
  from_custodian TEXT,
  to_custodian TEXT NOT NULL,
  location TEXT NOT NULL,
  purpose TEXT NOT NULL,
  timestamp TEXT NOT NULL,
  witness TEXT,
  integrity_hash TEXT,
  notes TEXT,
  PRIMARY KEY (id)
);

-- ============================================
-- HOLDS TABLE (Restricted)
-- ============================================
-- Legal hold directives
-- Data classification: Restricted

CREATE TABLE holds (
  id BIGINT NOT NULL,
  case_id BIGINT NOT NULL,
  hold_type TEXT NOT NULL,
  scope TEXT NOT NULL,
  issued_by TEXT NOT NULL,
  issued_date TEXT NOT NULL,
  released_date TEXT,
  status TEXT NOT NULL,
  custodians TEXT NOT NULL,
  notes TEXT,
  PRIMARY KEY (id)
);

-- ============================================
-- AUDIT LOG (Non-sensitive Metadata)
-- ============================================
-- Records all data access for compliance
-- Data classification: Non-sensitive (contains only references)

CREATE TABLE audit_log (
  id BIGINT NOT NULL,
  timestamp TEXT NOT NULL,
  user_id TEXT NOT NULL,
  action TEXT NOT NULL,
  resource_type TEXT NOT NULL,
  resource_id BIGINT,
  ip_address TEXT,
  details TEXT,
  PRIMARY KEY (id)
);

-- ============================================
-- SAMPLE DATA
-- ============================================

-- Cases
INSERT INTO cases (id, case_number, case_type, title, status, lead_attorney, client_name, opposing_party, court, filed_date)
VALUES (1, 'CV-2024-001234', 'Civil Litigation', 'Acme Corp v. Widget Inc - Patent Infringement', 'Active', 'attorney:rgarcia', 'Acme Corporation', 'Widget Industries Inc.', 'US District Court - Northern District', '2024-01-10');

INSERT INTO cases (id, case_number, case_type, title, status, lead_attorney, client_name, opposing_party, court, filed_date)
VALUES (2, 'CR-2024-005678', 'Regulatory Investigation', 'SEC Investigation - Insider Trading Allegation', 'Active', 'attorney:kpatel', 'DataFlow Systems', 'SEC', 'Administrative Proceeding', '2024-02-01');

-- Documents
INSERT INTO documents (id, case_id, document_type, title, classification, file_hash, file_size, source, collected_by, collected_date, privilege_status, review_status)
VALUES (1, 1, 'Email', 'Patent Discussion Thread - Engineering Team', 'Confidential', 'sha256:a1b2c3d4e5f6', 24576, 'Email Server', 'paralegal:jdoe', '2024-01-15', 'Under Review', 'Pending');

INSERT INTO documents (id, case_id, document_type, title, classification, file_hash, file_size, source, collected_by, collected_date, privilege_status, review_status)
VALUES (2, 1, 'Contract', 'Licensing Agreement - Widget Inc 2022', 'Confidential', 'sha256:b2c3d4e5f6a1', 102400, 'Document Management', 'paralegal:jdoe', '2024-01-15', 'Not Privileged', 'Reviewed');

INSERT INTO documents (id, case_id, document_type, title, classification, file_hash, file_size, source, collected_by, collected_date, privilege_status, review_status)
VALUES (3, 2, 'Trading Records', 'Q4 2023 Trade Log - DataFlow Executives', 'Highly Confidential', 'sha256:c3d4e5f6a1b2', 512000, 'Brokerage Records', 'attorney:kpatel', '2024-02-05', 'Work Product', 'Reviewed');

-- Custody log (chain of custody)
INSERT INTO custody_log (id, document_id, case_id, action, from_custodian, to_custodian, location, purpose, timestamp, witness, integrity_hash)
VALUES (1, 1, 1, 'COLLECTED', NULL, 'paralegal:jdoe', 'Evidence Room A', 'Initial collection from email server', '2024-01-15T10:00:00Z', 'attorney:rgarcia', 'blake3:x1y2z3');

INSERT INTO custody_log (id, document_id, case_id, action, from_custodian, to_custodian, location, purpose, timestamp, witness, integrity_hash)
VALUES (2, 1, 1, 'TRANSFERRED', 'paralegal:jdoe', 'attorney:rgarcia', 'Attorney Office', 'Privilege review', '2024-01-16T09:00:00Z', NULL, 'blake3:y2z3x1');

INSERT INTO custody_log (id, document_id, case_id, action, from_custodian, to_custodian, location, purpose, timestamp, witness, integrity_hash)
VALUES (3, 2, 1, 'COLLECTED', NULL, 'paralegal:jdoe', 'Evidence Room A', 'Document collection', '2024-01-15T10:30:00Z', 'attorney:rgarcia', 'blake3:z3x1y2');

INSERT INTO custody_log (id, document_id, case_id, action, from_custodian, to_custodian, location, purpose, timestamp, witness, integrity_hash)
VALUES (4, 3, 2, 'COLLECTED', NULL, 'attorney:kpatel', 'Secure Evidence Vault', 'Subpoena response collection', '2024-02-05T14:00:00Z', 'paralegal:asmith', 'blake3:w4v5u6');

-- Legal holds
INSERT INTO holds (id, case_id, hold_type, scope, issued_by, issued_date, status, custodians)
VALUES (1, 1, 'Litigation Hold', 'All documents related to Widget Inc patent discussions from 2022-2024', 'attorney:rgarcia', '2024-01-10', 'Active', 'engineering@acme.com, legal@acme.com');

INSERT INTO holds (id, case_id, hold_type, scope, issued_by, issued_date, status, custodians)
VALUES (2, 2, 'Regulatory Hold', 'All trading records and communications for DataFlow executives Q3-Q4 2023', 'attorney:kpatel', '2024-02-01', 'Active', 'executives@dataflow.com, trading@dataflow.com');

-- Audit entries
INSERT INTO audit_log (id, timestamp, user_id, action, resource_type, resource_id, ip_address, details)
VALUES (1, '2024-01-15T10:00:00Z', 'paralegal:jdoe', 'COLLECT', 'document', 1, '10.0.3.1', 'Collected email thread from server');

INSERT INTO audit_log (id, timestamp, user_id, action, resource_type, resource_id, ip_address, details)
VALUES (2, '2024-01-16T09:00:00Z', 'attorney:rgarcia', 'REVIEW', 'document', 1, '10.0.3.5', 'Privilege review - pending determination');

INSERT INTO audit_log (id, timestamp, user_id, action, resource_type, resource_id, ip_address, details)
VALUES (3, '2024-02-05T14:00:00Z', 'attorney:kpatel', 'COLLECT', 'document', 3, '10.0.3.8', 'Subpoena response - trading records collected');

INSERT INTO audit_log (id, timestamp, user_id, action, resource_type, resource_id, ip_address, details)
VALUES (4, '2024-02-06T11:00:00Z', 'attorney:kpatel', 'REVIEW', 'document', 3, '10.0.3.8', 'Work product review complete');
