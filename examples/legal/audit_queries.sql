-- Legal Audit Queries
-- Useful queries for chain of custody, eDiscovery, and case management

-- ============================================
-- CHAIN OF CUSTODY
-- ============================================

-- Full custody chain for a specific document
SELECT * FROM custody_log
WHERE document_id = 1;

-- All custody actions for a case
SELECT * FROM custody_log
WHERE case_id = 1;

-- Evidence transfers (potential handling gaps)
SELECT id, document_id, action, from_custodian, to_custodian, timestamp, location
FROM custody_log
WHERE action = 'TRANSFERRED';

-- ============================================
-- eDISCOVERY PRODUCTION
-- ============================================

-- All documents for a case
SELECT id, document_type, title, classification, file_hash, review_status
FROM documents
WHERE case_id = 1;

-- Documents pending privilege review
SELECT id, case_id, title, classification, privilege_status
FROM documents
WHERE review_status = 'Pending';

-- Non-privileged documents ready for production
SELECT id, title, document_type, file_hash, file_size
FROM documents
WHERE privilege_status = 'Not Privileged' AND review_status = 'Reviewed';

-- ============================================
-- LEGAL HOLDS
-- ============================================

-- Active holds
SELECT * FROM holds
WHERE status = 'Active';

-- Holds for a specific case
SELECT id, hold_type, scope, issued_by, issued_date, custodians
FROM holds
WHERE case_id = 1;

-- ============================================
-- CASE MANAGEMENT
-- ============================================

-- Active cases
SELECT id, case_number, title, status, lead_attorney
FROM cases
WHERE status = 'Active';

-- Cases by attorney
SELECT * FROM cases
WHERE lead_attorney = 'attorney:rgarcia';

-- ============================================
-- ACCESS AUDIT (COMPLIANCE)
-- ============================================

-- Who accessed a specific document?
SELECT * FROM audit_log
WHERE resource_type = 'document' AND resource_id = 1;

-- All collection activities
SELECT * FROM audit_log
WHERE action = 'COLLECT';

-- Activity by specific user
SELECT * FROM audit_log
WHERE user_id = 'attorney:kpatel';

-- ============================================
-- INTEGRITY VERIFICATION
-- ============================================

-- Verify document hashes match custody records
SELECT d.id, d.title, d.file_hash, c.integrity_hash, c.timestamp
FROM documents d
INNER JOIN custody_log c ON d.id = c.document_id
WHERE c.action = 'COLLECTED';

-- ============================================
-- TIME TRAVEL QUERIES (LITIGATION)
-- ============================================

-- To reconstruct case state at any point:
-- kmb repl --tenant 1
-- Use --at flag to query at a specific log position
--
-- This enables:
-- 1. eDiscovery: "What documents existed at the time of the hold notice?"
-- 2. Chain of custody: "Who had possession of evidence on a specific date?"
-- 3. Privilege review: "What was the classification status before re-review?"
-- 4. Spoliation defense: "Prove data was preserved from the moment of hold"
