# Versioning Policy

Kimberlite follows [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html), with the following project-specific clarifications.

## Pre-1.0 (0.x.y)

We are pre-1.0. Per the SemVer spec, the public API is not yet stable. In practice we apply the following rules:

- **Minor bump (0.x → 0.y)** may introduce breaking changes. Every breaking change is called out under a dedicated `### Breaking changes` subsection at the top of the release block in `CHANGELOG.md`, with migration guidance and a link to a per-release migration doc (e.g. `docs/coding/migration-v0.6.md`).
- **Patch bump (0.x.y → 0.x.z)** is guaranteed non-breaking. Bug fixes, documentation, internal refactors, new non-breaking APIs only.
- **Unreleased → 0.x.y** always goes through `CHANGELOG.md` with an explicit section — exactly one `[Unreleased]` block exists at any time, and it becomes the next release block on tag.

Breaking changes include:
- Removing or renaming a `pub` item in a published crate (`kimberlite`, `kimberlite-client`, `kimberlite-types`, `kimberlite-wire`).
- Changing the wire protocol in a way that old clients cannot parse new server responses (or vice versa).
- Changing on-disk format such that old data files cannot be read without explicit migration.
- Changing CLI flags or default behaviour in a way that breaks scripted usage.

Breaking changes are **not** introduced on patch bumps, even pre-1.0.

## 1.0 and later

Once we ship 1.0:

- Strict SemVer applies. Public API is stable across patches and minors.
- Breaking changes require a major bump (`1.x → 2.0`) and a migration guide.
- Security fixes are backported to the previous major for a support window (duration to be defined at 1.0).

## What "public API" means

For the purpose of this policy, the public API is:

1. Every `pub` item in a crate that is published to crates.io (currently: `kimberlite`, `kimberlite-client`, `kimberlite-types`, `kimberlite-wire`, `kimberlite-ffi`).
2. The wire protocol documented in [`docs/reference/protocol.md`](docs/reference/protocol.md).
3. The on-disk format of the append-only log, WAL, and index files.
4. The CLI surface documented in [`docs/reference/cli.md`](docs/reference/cli.md) (flags, subcommands, output formats when used with `--output=json`).
5. The language-specific SDK APIs under `sdks/` that are marked 🧪 Beta or better in [`sdks/README.md`](sdks/README.md).

Items **not** part of the public API:

- Anything `pub(crate)` or unmarked.
- Items in crates not published to crates.io (e.g. `kimberlite-sim`, `kimberlite-bench`).
- Internal CLI output formatting (colour codes, log line shapes) when consumed via stdout rather than `--output=json`.
- Compiler diagnostics from lints the project enforces.

## SDK version alignment

Each SDK is versioned in lockstep with the server's wire-protocol compatibility window, *not* independently. This is documented (and will be machine-enforced in CI via a compatibility matrix) in [`docs/reference/compatibility.md`](docs/reference/compatibility.md) — a matrix that declares, for each Kimberlite server release, which SDK versions speak a compatible wire protocol.

## Enforcement

- CI runs `cargo-semver-checks` on every PR that touches a published crate. Violations fail the build.
- The `just release-dry-run <version>` recipe runs the full 10-gate rehearsal locally (version consistency, CHANGELOG section present, clean build, clippy, tests, doc-tests, publish dry-run, fuzz smoke, VOPR smoke, advisory clean).
- Release commits must include a `### Breaking changes` subsection in the matching CHANGELOG block for any minor bump that introduces incompatibilities.

## MSRV (Minimum Supported Rust Version)

The current MSRV is pinned in `rust-toolchain.toml` (currently **1.88**). An MSRV bump is considered a breaking change and follows the same rules above.

## Questions

Open a [GitHub Discussion](https://github.com/kimberlitedb/kimberlite/discussions) or drop into the [Discord](https://discord.gg/QPChWYjD) — we'd rather be asked than guessed at.
