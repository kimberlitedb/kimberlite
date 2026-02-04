# Kimberlite Internal Documentation

**Audience:** Kimberlite contributors, maintainers, and core team members.

This directory contains internal documentation that is not intended for public consumption. For user-facing documentation, see [/docs](../docs/).

## What's In Here

### [VOPR Testing](vopr/)
Deep dive into our deterministic simulation testing framework.

- [Overview](vopr/overview.md) - VOPR architecture and capabilities
- [Scenarios](vopr/scenarios.md) - All 46 test scenarios documented
- [Deployment](vopr/deployment.md) - AWS testing infrastructure
- [Debugging](vopr/debugging.md) - Advanced debugging techniques
- [Writing Scenarios](vopr/writing-scenarios.md) - How to add new scenarios

### [Contributing](contributing/)
Guides for contributing to Kimberlite.

- [Getting Started](contributing/getting-started.md) - Contributor setup
- [Code Review](contributing/code-review.md) - Code review guidelines
- [Testing Strategy](contributing/testing-strategy.md) - Detailed testing approach
- [Release Process](contributing/release-process.md) - How we ship releases

### [Design Docs](design-docs/)
Internal design discussions and decisions.

- [Active](design-docs/active/) - Current design discussions
- [Archive](design-docs/archive/) - Completed design documents

### [Internal](internal/)
Team-internal processes and materials.

- [Cloud Architecture](internal/cloud-architecture.md) - Internal cloud infrastructure
- [Bug Bounty](internal/bug-bounty.md) - Bug bounty program details
- [Roadmap (Internal)](internal/roadmap-internal.md) - Internal roadmap details

---

## Public vs Internal Documentation

**Public Documentation** (`/docs`):
- User-facing guides and references
- Getting started tutorials
- API documentation
- High-level testing overview
- Architecture concepts

**Internal Documentation** (this directory):
- VOPR implementation details (46 scenarios, AWS deployment)
- Contributor onboarding and processes
- Detailed testing strategies
- Internal infrastructure
- Team processes

---

## Contributing

When adding new features to Kimberlite:

1. **Update public docs** (`/docs`) if users need to know about it
2. **Update internal docs** (this directory) if contributors need implementation details
3. **Keep them separate** - don't duplicate content between public and internal docs

For questions about documentation, reach out to the core team.
