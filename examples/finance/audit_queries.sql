-- Finance Audit Queries
-- Useful queries for SEC/SOX compliance and trade surveillance

-- ============================================
-- TRADE AUDIT TRAIL
-- ============================================

-- All trades for a specific account
SELECT * FROM trades
WHERE account_id = 1;

-- All trades by a specific trader
SELECT * FROM trades
WHERE trader_id = 'trader:jsmith';

-- Trades for a specific security
SELECT * FROM trades
WHERE symbol = 'AAPL';

-- Large trades (over $50,000) for surveillance
SELECT id, trade_date, symbol, side, quantity, total_cents, trader_id
FROM trades
WHERE total_cents > 5000000;

-- ============================================
-- COMPLIANCE REVIEW
-- ============================================

-- Trades pending compliance review
SELECT id, trade_date, symbol, side, quantity, trader_id, compliance_status
FROM trades
WHERE compliance_status = 'Pending';

-- All SELL orders (potential wash sale detection)
SELECT id, account_id, trade_date, symbol, quantity, price_cents
FROM trades
WHERE side = 'SELL';

-- ============================================
-- PORTFOLIO POSITIONS
-- ============================================

-- Current positions for an account
SELECT symbol, quantity, avg_cost_cents, market_value_cents
FROM positions
WHERE account_id = 1;

-- All positions across accounts for a symbol
SELECT account_id, quantity, avg_cost_cents, last_updated
FROM positions
WHERE symbol = 'AAPL';

-- ============================================
-- ACCESS AUDIT (SEC 17a-4)
-- ============================================

-- Who accessed a specific account's data?
SELECT * FROM audit_log
WHERE resource_type = 'account' AND resource_id = 1;

-- All trade executions
SELECT * FROM audit_log
WHERE action = 'TRADE_EXECUTE';

-- Activity by specific user
SELECT * FROM audit_log
WHERE user_id = 'trader:jsmith';

-- Compliance team reviews
SELECT * FROM audit_log
WHERE user_id = 'compliance:admin';

-- ============================================
-- TIME TRAVEL QUERIES (REGULATORY)
-- ============================================

-- To reconstruct portfolio state at any point:
-- kmb repl --tenant 1
-- Use --at flag to query at a specific log position
--
-- This enables:
-- 1. SEC examination: "What did the portfolio look like on Jan 17?"
-- 2. SOX audit: "Were internal controls in place at quarter-end?"
-- 3. Trade surveillance: "Reconstruct order book state before suspicious trade"
-- 4. Client dispute: "What was the account value at close of business?"
