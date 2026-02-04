# Internals - Deep Technical Details

For contributors and those who want to understand how Kimberlite works internally.

## Architecture Deep Dives

### [Crate Structure](architecture/crate-structure.md)
Workspace organization and crate dependencies.

### [Kernel](architecture/kernel.md)
Pure functional state machine: Commands → State + Effects.

### [Storage](architecture/storage.md)
Append-only log with CRC32 checksums and hash chains.

### [Cryptography](architecture/crypto.md)
Dual-hash system (SHA-256 for compliance, BLAKE3 for performance).

## Testing

### [Testing Overview](testing/overview.md)
Testing philosophy and infrastructure.

### [Assertions](testing/assertions.md)
Production assertions for safety-critical invariants.

### [Property Testing](testing/property-testing.md)
Property-based testing with proptest.

## Design Documents

### [Instrumentation](design/instrumentation.md)
Metrics, tracing, and observability design.

### [Reconfiguration](design/reconfiguration.md)
Cluster membership changes design.

### [LLM Integration](design/llm-integration.md)
AI agent protocol and integration patterns.

### [Data Sharing](design/data-sharing.md)
Multi-tenant data sharing and access control.

## Implementation Details

### [Compliance Implementation](compliance-implementation.md)
Full technical details of compliance features (HIPAA, GDPR, etc.).

### [VSR Production Gaps](vsr-production-gaps.md)
Known gaps between current VSR implementation and production requirements.

## Who Should Read This?

- **Contributors** - Understanding codebase before making changes
- **Advanced users** - Optimizing for specific use cases
- **Researchers** - Understanding algorithmic choices
- **Auditors** - Verifying safety claims

## For Contributors

See [/docs-internal](../../docs-internal/) for:
- VOPR testing details (46 scenarios, AWS deployment)
- Contributor guides and processes
- Internal design discussions
