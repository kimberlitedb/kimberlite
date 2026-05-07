-- Finance / Time-Travel Queries
-- Point-in-time portfolio reconstruction for SEC investigations,
-- end-of-quarter reporting, and post-incident forensics.
--
-- Kimberlite ships two time-travel primitives:
--   • SELECT … AT OFFSET n             — log-position-based, exact
--   • SELECT … AS OF TIMESTAMP '...'   — wall-clock, resolved via the audit-log index
--
-- These queries are deterministic: same offset / timestamp → same result,
-- always. That's the SEC 17a-4 "non-rewriteable, non-erasable storage"
-- contract met by construction.

-- ============================================================================
-- Q1. Current portfolio for an account
-- ============================================================================
-- The baseline. What does the account hold *right now*?
SELECT account_id, symbol, quantity, avg_cost_cents, market_value_cents
FROM positions
WHERE account_id = 1
ORDER BY symbol;

-- ============================================================================
-- Q2. Portfolio as of close of business 2024-01-16
-- ============================================================================
-- Regulatory ask: "What did Apex Capital hold at end of day on Jan 16?"
-- The TSLA position (account 2) should NOT appear in account 1's view.
SELECT account_id, symbol, quantity, avg_cost_cents
FROM positions
WHERE account_id = 1
AS OF TIMESTAMP '2024-01-16T23:59:59Z'
ORDER BY symbol;

-- ============================================================================
-- Q3. All trades executed by trader:jsmith before the AAPL sale
-- ============================================================================
-- Pattern: wall-clock time travel + filter. The trader sold 200 AAPL on
-- 2024-01-20; reconstruct everything they did before that.
SELECT id, trade_date, symbol, side, quantity, price_cents
FROM trades
WHERE trader_id = 'trader:jsmith'
AS OF TIMESTAMP '2024-01-20T10:59:59Z'
ORDER BY trade_date, id;

-- ============================================================================
-- Q4. Audit log AT OFFSET — exact reconstruction
-- ============================================================================
-- Pattern: log-position time travel. AT OFFSET 0 returns the empty initial
-- state; subsequent offsets advance one event at a time. Useful when the
-- regulator demands "what events were in this stream at sequence N?"
-- (Replace N with a real offset from your environment.)
SELECT id, timestamp, user_id, action, resource_type, resource_id
FROM audit_log
AT OFFSET 5
ORDER BY id;

-- ============================================================================
-- Q5. Cross-table point-in-time consistency
-- ============================================================================
-- All four tables resolve to the SAME log offset for the same timestamp.
-- That's the immutable-log + derived-view contract: there is no torn read.
SELECT 'accounts' AS source, COUNT(*) AS rows FROM accounts AS OF TIMESTAMP '2024-01-15T23:59:59Z'
UNION ALL
SELECT 'trades',   COUNT(*)            FROM trades   AS OF TIMESTAMP '2024-01-15T23:59:59Z'
UNION ALL
SELECT 'positions', COUNT(*)            FROM positions AS OF TIMESTAMP '2024-01-15T23:59:59Z'
UNION ALL
SELECT 'audit_log', COUNT(*)            FROM audit_log AS OF TIMESTAMP '2024-01-15T23:59:59Z';
