# Contributing to Kimberlite

Thank you for your interest in contributing to Kimberlite.

Kimberlite is a correctness-first system of record designed for regulated
environments. As a result, we place a high bar on design clarity,
determinism, and explainability.

Please read this document carefully before contributing.

---

## Philosophy

Kimberlite follows a few non-negotiable principles:

- The append-only log is the system of record.
- All state is derived and must be replayable.
- Correctness and explainability take precedence over convenience.
- Performance optimizations must preserve determinism.
- Compliance is structural, not procedural.

Contributions are evaluated primarily on how well they preserve these
invariants.

---

## What we welcome

We welcome contributions in the following areas:

- correctness fixes
- performance improvements with clear reasoning
- documentation and design clarification
- tests (especially failure and recovery scenarios)
- tooling that improves observability or verification
- SDK ergonomics (without weakening guarantees)

Small, well-scoped changes are preferred over large feature additions.

---

## What we are cautious about

We are cautious about:

- expanding the query language without strong justification
- introducing background processes with unpredictable behavior
- adding implicit behavior or hidden side effects
- introducing dependencies that reduce deployability or auditability

Feature requests that significantly expand scope may be deferred or
redirected to external integrations.

---

## Design-first contributions

For non-trivial changes, please open a design discussion before
submitting a pull request.

A good design proposal should include:

- the problem being solved
- relevant invariants
- failure modes and recovery behavior
- how correctness is preserved
- how the change can be tested

Code without an accompanying explanation is unlikely to be accepted.

---

## Code style and expectations

- Favor explicitness over cleverness.
- Avoid hidden global state.
- Prefer bounded work and predictable behavior.
- Keep APIs minimal and intention-revealing.
- Tests should be deterministic and reproducible.

---

## Licensing and contributions

Kimberlite is licensed under the Apache License 2.0.

By submitting a contribution, you agree that:

- your contribution is licensed under the Apache 2.0 license
- you have the right to submit the contribution
- you grant the project the right to use, modify, and distribute it

If you are contributing on behalf of an employer, ensure you have
appropriate permission to do so.

We may introduce a Contributor License Agreement (CLA) in the future
to simplify commercial distribution.

---

## Security and correctness issues

If you believe you have found a security or correctness issue that
could affect data integrity or compliance, please do not open a public
issue.

Instead, contact the maintainers privately.

---

## Repository Organization

Kimberlite follows a clean, organized structure:

### Directory Layout

```
kimberlite/
├── .artifacts/         # Temporary build/test artifacts (gitignored)
├── crates/             # All source code (30+ crates)
├── docs/               # Public user-facing documentation
├── docs-internal/      # Internal contributor documentation
├── examples/           # Language-specific examples
├── specs/              # Formal specifications (TLA+, Coq, Alloy)
├── tools/              # Development tools (formal verification)
└── website/            # Public website content
```

### Key Principles

1. **All source code → `/crates`** - All Rust crates in one location
2. **Workspace-level Cargo.toml** - All members defined once
3. **Single justfile** - All commands consolidated (run `just --list`)
4. **No artifacts in root** - Everything goes to `.artifacts/` (gitignored)
5. **No scripts/ directory** - All commands via justfile
6. **Clear documentation** - Public (`docs/`) vs internal (`docs-internal/`)

### Finding Commands

Run `just --list` to see all available commands:

```bash
# Building
just build              # Debug build
just build-release      # Release build

# Testing
just test               # Run all tests
just vopr               # VOPR simulation
just vopr-byzantine     # Byzantine attack tests

# Formal Verification
just verify-local       # All verification
just verify-tla         # TLA+ only
just verify-coq         # Coq only

# Code Quality
just fmt                # Format code
just clippy             # Lint code
just pre-commit         # Run all checks
```

See `justfile` for complete list and detailed documentation.

### Artifacts and Cleanup

All temporary files go to `.artifacts/`:

```bash
just clean-all          # Clean everything
just clean-test         # Clean test artifacts only
just archive-vopr-logs  # Archive logs before cleanup
```

Never commit files from `.artifacts/` - it's fully gitignored.

---

## Getting started

- Read the architecture documentation
- Review the system invariants
- Explore the examples directory
- Start with small, well-contained changes
- Use `just --list` to discover available commands

If you are unsure where to begin, open an issue and ask.

---

Thank you for helping make Kimberlite correct, explainable, and trustworthy.

---

## Release Process

### Required GitHub Secrets

Before tagging a release, ensure these secrets are configured in the repository
(Settings → Secrets and variables → Actions):

| Secret | Used by | Where to get it |
|--------|---------|-----------------|
| `CARGO_REGISTRY_TOKEN` | `release.yml` publish-crates job | [crates.io](https://crates.io/settings/tokens) |
| `PYPI_API_TOKEN` | `sdk-python.yml` publish step | [pypi.org](https://pypi.org/manage/account/token/) |
| `NPM_TOKEN` | `sdk-typescript.yml` publish step | `npm token create` |
| `HOMEBREW_TAP_TOKEN` | `release.yml` homebrew dispatch | GitHub PAT with `repo` scope on `kimberlitedb/homebrew-tap` |

### Release Steps

1. Run `just ci` locally — must be fully green
2. Run `just publish-dry-run` — verifies all crates would publish cleanly
3. Tag: `git tag v0.x.y && git push origin v0.x.y`
4. GitHub Actions triggers automatically:
   - `release.yml` — builds 5-platform binaries, creates GitHub Release, publishes crates, pushes Docker image, triggers Homebrew update
   - `sdk-python.yml` — builds wheels, publishes to PyPI
   - `sdk-typescript.yml` — builds package, publishes to npm
5. Enable GitHub Pages if not already enabled:
   Settings → Pages → Source: **GitHub Actions** (enables `docs.yml` deployment)
