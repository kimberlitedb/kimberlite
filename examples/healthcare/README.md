# Healthcare / HIPAA Example

This example demonstrates using Kimberlite for healthcare data with HIPAA compliance considerations.

## Key Features Demonstrated

1. **Data Classification** - Streams with PHI, Non-PHI, and De-identified classification
2. **Audit Trail** - Immutable record of all data access
3. **Time Travel** - Point-in-time queries for compliance investigations

## Schema Design

The schema separates PHI from non-PHI data:

- `patients` - Core patient demographics (PHI)
- `encounters` - Clinical encounters (PHI)
- `audit_log` - Access audit trail (Non-PHI metadata)

## Setup

1. Start Kimberlite:

```bash
kimberlite init ./data
kimberlite start ./data
```

2. Create the schema:

```bash
# In the REPL
kimberlite repl

# Run schema.sql commands
```

3. Run audit queries:

```bash
kimberlite query "$(cat audit_queries.sql)"
```

## Compliance Considerations

### HIPAA Technical Safeguards

| Safeguard | Kimberlite Feature |
|-----------|-------------------|
| Access Control | Multi-tenant isolation |
| Audit Controls | Immutable append-only log |
| Integrity | Hash-chained events |
| Transmission Security | TLS support |

### Data Retention

Kimberlite's immutable design ensures:
- Data cannot be silently modified
- Complete audit trail of all changes
- Point-in-time reconstruction for investigations

## Time Travel Queries

Query historical state for compliance audits:

```bash
# See data as it was at log position 100
kimberlite query --at 100 "SELECT * FROM patients WHERE id = 1"
```
