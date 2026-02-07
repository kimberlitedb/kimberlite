-- Finance / SEC Compliance Schema
-- Demonstrates trade audit trail and regulatory record-keeping

-- ============================================
-- ACCOUNTS TABLE (Restricted)
-- ============================================
-- Trading account metadata
-- Data classification: Restricted

CREATE TABLE accounts (
  id BIGINT NOT NULL,
  account_number TEXT NOT NULL,
  account_type TEXT NOT NULL,
  owner_name TEXT NOT NULL,
  owner_entity TEXT,
  custodian TEXT,
  status TEXT NOT NULL,
  opened_date TEXT NOT NULL,
  closed_date TEXT,
  created_at TIMESTAMP,
  PRIMARY KEY (id)
);

-- ============================================
-- TRADES TABLE (Sensitive)
-- ============================================
-- Securities transactions with full provenance
-- Data classification: Sensitive (SEC 17a-4)

CREATE TABLE trades (
  id BIGINT NOT NULL,
  account_id BIGINT NOT NULL,
  trade_date TEXT NOT NULL,
  settlement_date TEXT,
  symbol TEXT NOT NULL,
  side TEXT NOT NULL,
  quantity BIGINT NOT NULL,
  price_cents BIGINT NOT NULL,
  total_cents BIGINT NOT NULL,
  currency TEXT NOT NULL,
  exchange TEXT,
  order_type TEXT NOT NULL,
  execution_venue TEXT,
  counterparty TEXT,
  trader_id TEXT NOT NULL,
  compliance_status TEXT NOT NULL,
  notes TEXT,
  created_at TIMESTAMP,
  PRIMARY KEY (id)
);

-- ============================================
-- POSITIONS TABLE (Sensitive)
-- ============================================
-- Current portfolio holdings
-- Data classification: Sensitive

CREATE TABLE positions (
  id BIGINT NOT NULL,
  account_id BIGINT NOT NULL,
  symbol TEXT NOT NULL,
  quantity BIGINT NOT NULL,
  avg_cost_cents BIGINT NOT NULL,
  market_value_cents BIGINT,
  last_updated TEXT NOT NULL,
  PRIMARY KEY (id)
);

-- ============================================
-- AUDIT LOG (Non-sensitive Metadata)
-- ============================================
-- Records all data access for SEC/SOX compliance
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

-- Accounts
INSERT INTO accounts (id, account_number, account_type, owner_name, owner_entity, custodian, status, opened_date)
VALUES (1, 'ACCT-100234', 'Institutional', 'Apex Capital Management', 'Apex Capital LLC', 'State Street', 'Active', '2023-06-15');

INSERT INTO accounts (id, account_number, account_type, owner_name, owner_entity, custodian, status, opened_date)
VALUES (2, 'ACCT-100567', 'Individual', 'Sarah Chen', NULL, 'Pershing', 'Active', '2024-01-10');

-- Trades
INSERT INTO trades (id, account_id, trade_date, settlement_date, symbol, side, quantity, price_cents, total_cents, currency, exchange, order_type, counterparty, trader_id, compliance_status)
VALUES (1, 1, '2024-01-15', '2024-01-17', 'AAPL', 'BUY', 500, 18950, 9475000, 'USD', 'NASDAQ', 'LIMIT', 'Goldman Sachs', 'trader:jsmith', 'Cleared');

INSERT INTO trades (id, account_id, trade_date, settlement_date, symbol, side, quantity, price_cents, total_cents, currency, exchange, order_type, counterparty, trader_id, compliance_status)
VALUES (2, 1, '2024-01-15', '2024-01-17', 'MSFT', 'BUY', 200, 39000, 7800000, 'USD', 'NASDAQ', 'MARKET', 'Morgan Stanley', 'trader:jsmith', 'Cleared');

INSERT INTO trades (id, account_id, trade_date, settlement_date, symbol, side, quantity, price_cents, total_cents, currency, exchange, order_type, counterparty, trader_id, compliance_status)
VALUES (3, 2, '2024-01-16', '2024-01-18', 'TSLA', 'BUY', 100, 21500, 2150000, 'USD', 'NASDAQ', 'LIMIT', 'Citadel', 'trader:mlee', 'Cleared');

INSERT INTO trades (id, account_id, trade_date, settlement_date, symbol, side, quantity, price_cents, total_cents, currency, exchange, order_type, counterparty, trader_id, compliance_status)
VALUES (4, 1, '2024-01-20', '2024-01-22', 'AAPL', 'SELL', 200, 19200, 3840000, 'USD', 'NASDAQ', 'LIMIT', 'JPMorgan', 'trader:jsmith', 'Cleared');

-- Positions (after above trades)
INSERT INTO positions (id, account_id, symbol, quantity, avg_cost_cents, market_value_cents, last_updated)
VALUES (1, 1, 'AAPL', 300, 18950, 5760000, '2024-01-20');

INSERT INTO positions (id, account_id, symbol, quantity, avg_cost_cents, market_value_cents, last_updated)
VALUES (2, 1, 'MSFT', 200, 39000, 7800000, '2024-01-15');

INSERT INTO positions (id, account_id, symbol, quantity, avg_cost_cents, market_value_cents, last_updated)
VALUES (3, 2, 'TSLA', 100, 21500, 2150000, '2024-01-16');

-- Audit entries
INSERT INTO audit_log (id, timestamp, user_id, action, resource_type, resource_id, ip_address, details)
VALUES (1, '2024-01-15T09:30:00Z', 'trader:jsmith', 'TRADE_EXECUTE', 'trade', 1, '10.0.1.5', 'BUY 500 AAPL @ 189.50');

INSERT INTO audit_log (id, timestamp, user_id, action, resource_type, resource_id, ip_address, details)
VALUES (2, '2024-01-15T09:31:00Z', 'trader:jsmith', 'TRADE_EXECUTE', 'trade', 2, '10.0.1.5', 'BUY 200 MSFT @ 390.00');

INSERT INTO audit_log (id, timestamp, user_id, action, resource_type, resource_id, ip_address, details)
VALUES (3, '2024-01-16T10:15:00Z', 'trader:mlee', 'TRADE_EXECUTE', 'trade', 3, '10.0.1.8', 'BUY 100 TSLA @ 215.00');

INSERT INTO audit_log (id, timestamp, user_id, action, resource_type, resource_id, ip_address, details)
VALUES (4, '2024-01-18T14:00:00Z', 'compliance:admin', 'READ', 'account', 1, '10.0.2.1', 'Quarterly review - Apex Capital');

INSERT INTO audit_log (id, timestamp, user_id, action, resource_type, resource_id, ip_address, details)
VALUES (5, '2024-01-20T11:00:00Z', 'trader:jsmith', 'TRADE_EXECUTE', 'trade', 4, '10.0.1.5', 'SELL 200 AAPL @ 192.00');
