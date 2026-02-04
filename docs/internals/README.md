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
- **[VSR Production Gaps](vsr-production-gaps.md)** - Known limitations and future work

---

**Note:** For contributor guides, internal testing details, and VOPR documentation, see [docs-internal/](../../docs-internal/).
