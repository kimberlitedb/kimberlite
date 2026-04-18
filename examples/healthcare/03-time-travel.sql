-- Point-in-time queries for compliance investigations.
--
-- Kimberlite stores every INSERT/UPDATE/DELETE as an immutable log entry,
-- so historical state is first-class.  "What did this chart look like on
-- 2024-01-15?" is a single SQL query, not a restore-from-backup.
--
-- Two clause syntaxes are supported:
--
--   AS OF TIMESTAMP '2024-01-15 10:30:00'    -- wall-clock
--   AT OFFSET 4200                            -- log offset captured earlier
--
-- Both compose with WHERE, JOIN, GROUP BY, CTEs, etc.

-- ----------------------------------------------------------------------------
-- Q1: Patient chart snapshot at a historical moment.
--     Use case: "What did the chart look like when the consult note was
--     written, for the malpractice review?"
-- ----------------------------------------------------------------------------

SELECT id, medical_record_number, first_name, last_name, primary_provider_id, active
FROM patients
AS OF TIMESTAMP '2024-01-20 14:30:00'
WHERE id = 1;

-- ----------------------------------------------------------------------------
-- Q2: Historical encounter list for a patient as of a given date.
-- ----------------------------------------------------------------------------

SELECT
    e.id,
    e.encounter_date,
    e.encounter_type,
    e.diagnosis_codes,
    p.first_name || ' ' || p.last_name AS provider
FROM encounters e
JOIN providers p ON p.id = e.provider_id
AS OF TIMESTAMP '2024-02-01 00:00:00'
WHERE e.patient_id = 1
ORDER BY e.encounter_date;

-- ----------------------------------------------------------------------------
-- Q3: Compare current state to historical state (side-by-side).
--     Demonstrates two queries — the app / REPL shows them together.
-- ----------------------------------------------------------------------------

-- Current:
SELECT id, first_name, last_name, primary_provider_id FROM patients WHERE id = 3;

-- As of 2024-01-10 (before Alice was established with Dr. Williams):
SELECT id, first_name, last_name, primary_provider_id
FROM patients
AS OF TIMESTAMP '2024-01-03 00:00:00'
WHERE id = 3;

-- ----------------------------------------------------------------------------
-- Q4: Reconstructing who had access to a patient at a specific moment.
--     Access grants are appended + revoked via the same append-only log, so
--     historical grants are reachable even after revocation.
-- ----------------------------------------------------------------------------

SELECT
    g.provider_id,
    p.first_name || ' ' || p.last_name AS provider,
    g.reason,
    g.granted_at
FROM access_grants g
JOIN providers p ON p.id = g.provider_id
AS OF TIMESTAMP '2024-02-15 00:00:00'
WHERE g.patient_id = 3
  AND g.revoked_at IS NULL
ORDER BY g.granted_at;

-- ----------------------------------------------------------------------------
-- Q5: Audit-log playback by log offset.
--     If the application captured `client.lastRequestId` at the time of a
--     suspicious write, you can query state immediately before or after that
--     log offset.
-- ----------------------------------------------------------------------------

-- State at offset 10 (replace with a real offset from your dev environment):
SELECT * FROM encounters AT OFFSET 10 ORDER BY id;
