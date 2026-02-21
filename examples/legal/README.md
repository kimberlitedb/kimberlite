# Legal / Chain of Custody Example

This example demonstrates using Kimberlite for legal case management with immutable evidence tracking and chain of custody.

## Key Features Demonstrated

1. **Chain of Custody** - Immutable, hash-chained record of evidence handling
2. **Legal Holds** - Prevent modification or deletion of case-related data
3. **eDiscovery Support** - Point-in-time queries for litigation document production

## Schema Design

The schema separates case management from evidence tracking:

- `cases` - Legal case metadata (Restricted)
- `documents` - Evidence documents with classification (Privileged)
- `custody_log` - Immutable chain of custody records (Restricted)
- `holds` - Legal hold directives (Restricted)
- `audit_log` - Access audit trail (Non-sensitive metadata)

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

### Chain of Custody

| Requirement | Kimberlite Feature |
|-------------|-------------------|
| Tamper-evident records | Hash-chained append-only log |
| Continuous custody tracking | Immutable custody_log entries |
| Identity of handlers | Audit trail with user attribution |
| Timestamps of transfers | CRC32 checksums + ordered offsets |

### eDiscovery (FRCP Rule 26)

| Requirement | Kimberlite Feature |
|-------------|-------------------|
| Preservation of relevant data | Legal holds prevent deletion |
| Production of documents | Point-in-time query export |
| Metadata preservation | Full event metadata in log |
| Defensible collection | Hash chain proves integrity |

### ABA Model Rules (Professional Ethics)

| Requirement | Kimberlite Feature |
|-------------|-------------------|
| Client confidentiality (Rule 1.6) | Multi-tenant isolation + RBAC |
| Competent representation (Rule 1.1) | Complete case audit trail |
| Safekeeping property (Rule 1.15) | Immutable evidence records |

## Time Travel Queries

Reconstruct case state for litigation timeline:

```bash
# See evidence state at log position 200
kimberlite repl --tenant 1
# SELECT * FROM documents WHERE case_id = 1;
# (use --at flag when available for point-in-time)
```
