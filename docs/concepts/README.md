---
title: "Concepts - Understanding Kimberlite"
section: "concepts"
slug: "README"
order: 0
---

# Concepts - Understanding Kimberlite

Learn the strategic "why" behind Kimberlite's design decisions and approach.

## Core Concepts

### [Overview](overview.md)
What is Kimberlite? Why does it exist? Who is it for?

### [Architecture](architecture.md)
High-level system design: functional core, append-only log, derived views.

### [Data Model](data-model.md)
Everything is an immutable, ordered log. All state is a derived view.

### [Consensus](consensus.md)
How Viewstamped Replication (VSR) provides safety and liveness guarantees.

### [Compliance](compliance.md)
Why "compliance-first" matters and how Kimberlite enforces it.

### [Multi-tenancy](multitenancy.md)
Tenant isolation, security boundaries, and performance isolation.

### [Pressurecraft](pressurecraft.md)
Our coding philosophy: functional core/imperative shell, make illegal states unrepresentable, assertion density.

## Who Should Read This?

- **Evaluators** - Deciding if Kimberlite fits your use case
- **Architects** - Understanding system design trade-offs
- **Developers** - Learning mental models before coding
- **Operators** - Understanding what you're deploying

## Next Steps

After understanding concepts:
- **[Start Coding](../coding/)** - Build applications with these concepts
- **[Internals](../internals/)** - Dive deeper into implementation details
- **[Reference](../reference/)** - Look up API specifics
