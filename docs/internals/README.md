# Internals - Deep Technical Details

Technical deep dives for contributors and those wanting to understand Kimberlite's implementation.

## Architecture

- **[Crate Structure](architecture/crate-structure.md)** - 30 crates, 5 layers
- **[Kernel](architecture/kernel.md)** - Pure functional state machine
- **[Storage](architecture/storage.md)** - Append-only log format
- **[Cryptography](architecture/crypto.md)** - Dual-hash strategy, envelope encryption

## Testing

- **[Overview](testing/overview.md)** - Testing philosophy and strategies
- **[Assertions](testing/assertions.md)** - Production assertion patterns
- **[Property Testing](testing/property-testing.md)** - Proptest strategies

## Design Documents

- **[Instrumentation](design/instrumentation.md)** - Observability architecture
- **[Reconfiguration](design/reconfiguration.md)** - Dynamic cluster membership
- **[LLM Integration](design/llm-integration.md)** - AI-assisted operations
- **[Data Sharing](design/data-sharing.md)** - Cross-tenant data sharing

## Implementation Details

- **[Compliance Implementation](compliance-implementation.md)** - Technical compliance details
- **[VSR Implementation](vsr.md)** - Viewstamped Replication internals
- **[Clock Synchronization](clock-synchronization.md)** - Cluster-wide time consensus
- **[Client Sessions](client-sessions.md)** - VRR bug fixes
- **[Repair Budget](repair-budget.md)** - Repair storm prevention
- **[Log Scrubbing](log-scrubbing.md)** - Background corruption detection

---

**Note:** For contributor guides, internal testing details, and VOPR documentation, see [docs-internal/](../../docs-internal/).
