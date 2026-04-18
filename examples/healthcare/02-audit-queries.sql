-- Application-level audit queries.
--
-- The audit_log table is written by the application on every PHI access.
-- (The append-only log *underneath* is the source of truth — see
-- 03-time-travel.sql — but the audit_log projection is what compliance
-- officers typically query.)
--
-- Run individually:  kimberlite query "$(sed -n '/^-- Q1/,/^-- Q2/p' examples/healthcare/02-audit-queries.sql)"
-- Or all at once:    kimberlite query -f examples/healthcare/02-audit-queries.sql

-- ----------------------------------------------------------------------------
-- Q1: Who accessed a specific patient in a date range?
--     HIPAA "accounting of disclosures" request (§164.528).
-- ----------------------------------------------------------------------------

SELECT
    a.event_at,
    a.actor_kind,
    p.first_name || ' ' || p.last_name AS actor_name,
    a.action,
    a.ip_address,
    a.details
FROM audit_log a
LEFT JOIN providers p ON a.actor_kind = 'provider' AND a.actor_id = p.id
WHERE a.resource_type = 'patient'
  AND a.resource_id = 1
  AND a.event_at BETWEEN '2024-01-01 00:00:00' AND '2024-12-31 23:59:59'
ORDER BY a.event_at;

-- ----------------------------------------------------------------------------
-- Q2: Access frequency — who reads the most charts?
-- ----------------------------------------------------------------------------

SELECT
    p.first_name || ' ' || p.last_name AS provider,
    p.specialty,
    COUNT(*) AS chart_reads
FROM audit_log a
JOIN providers p ON p.id = a.actor_id
WHERE a.actor_kind = 'provider'
  AND a.action = 'READ'
  AND a.resource_type = 'patient'
GROUP BY p.first_name, p.last_name, p.specialty
HAVING COUNT(*) > 0
ORDER BY chart_reads DESC;

-- ----------------------------------------------------------------------------
-- Q3: Unauthorised-access candidates — reads by providers WITHOUT an
--     active access_grant for that patient. (Would normally be zero in a
--     well-run clinic; any row here warrants investigation.)
-- ----------------------------------------------------------------------------

SELECT
    a.event_at,
    a.actor_id AS provider_id,
    a.resource_id AS patient_id,
    a.ip_address,
    a.details
FROM audit_log a
LEFT JOIN access_grants g
    ON g.provider_id = a.actor_id
   AND g.patient_id  = a.resource_id
   AND g.revoked_at IS NULL
WHERE a.actor_kind = 'provider'
  AND a.action = 'READ'
  AND a.resource_type = 'patient'
  AND g.id IS NULL
ORDER BY a.event_at DESC;

-- ----------------------------------------------------------------------------
-- Q4: Daily write volume (encounter + chart edits) per provider.
--     Useful for spotting anomalies — an unusual burst is a breach indicator.
-- ----------------------------------------------------------------------------

WITH daily AS (
    SELECT
        actor_id,
        action,
        COUNT(*) AS ops
    FROM audit_log
    WHERE actor_kind = 'provider'
      AND action IN ('WRITE', 'DELETE')
    GROUP BY actor_id, action
)
SELECT
    p.first_name || ' ' || p.last_name AS provider,
    daily.action,
    daily.ops
FROM daily
JOIN providers p ON p.id = daily.actor_id
ORDER BY daily.ops DESC;

-- ----------------------------------------------------------------------------
-- Q5: Encounter volume by provider and specialty (operational metric).
-- ----------------------------------------------------------------------------

SELECT
    p.specialty,
    p.first_name || ' ' || p.last_name AS provider,
    COUNT(e.id) AS encounters,
    AVG(e.duration_minutes) AS avg_minutes
FROM providers p
LEFT JOIN encounters e ON e.provider_id = p.id
WHERE p.active = TRUE
GROUP BY p.specialty, p.first_name, p.last_name
ORDER BY p.specialty, encounters DESC;
