# Finance / SEC Compliance Example

This example demonstrates using Kimberlite for financial trade audit trails with SEC, SOX, and GLBA compliance considerations.

## Key Features Demonstrated

1. **Immutable Trade History** - Every trade recorded in an append-only log with hash-chain integrity
2. **Audit Trail** - Complete record of who accessed or modified trade data
3. **Time Travel** - Reconstruct portfolio positions as of any historical point

## Schema Design

The schema separates trade data from reference data:

- `trades` - Securities transactions with full provenance (Sensitive)
- `positions` - Current and historical portfolio positions (Sensitive)
- `accounts` - Trading account metadata (Restricted)
- `audit_log` - Access and modification audit trail (Non-sensitive metadata)

## Setup

1. Start Kimberlite:

```bash
kimberlite init ./data
kimberlite dev
```

2. Create the schema:

```bash
# In the REPL
kimberlite repl --tenant 1

# Run schema.sql commands
```

3. Run audit queries:

```bash
kimberlite repl --tenant 1
# Paste queries from audit_queries.sql
```

## Compliance Considerations

### SEC Rule 17a-4 (Broker-Dealer Records)

| Requirement | Kimberlite Feature |
|-------------|-------------------|
| Non-rewriteable, non-erasable storage | Immutable append-only log |
| Records preserved for 3-6 years | Hash-chained events, no deletion |
| Immediate availability for 2 years | Point-in-time queries at any offset |
| Audit trail of access | Built-in audit logging |

### SOX (Sarbanes-Oxley)

| Requirement | Kimberlite Feature |
|-------------|-------------------|
| Internal controls over financial reporting | RBAC with role-based access |
| Audit trail for financial data | Immutable event log |
| Data integrity | CRC32 checksums + hash chains |

### GLBA (Gramm-Leach-Bliley)

| Requirement | Kimberlite Feature |
|-------------|-------------------|
| Safeguards for customer financial data | Multi-tenant isolation |
| Access controls | RBAC + ABAC policies |
| Data encryption | AES-256-GCM encryption at rest |

## Time Travel Queries

Reconstruct positions at any historical point for regulatory investigations:

```bash
# See portfolio state at log position 500
kimberlite repl --tenant 1
# SELECT * FROM positions WHERE account_id = 1;
# (use --at flag when available for point-in-time)
```
