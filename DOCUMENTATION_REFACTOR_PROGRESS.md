# Documentation Refactor Progress

**Goal:** Transform documentation from flat structure to TigerBeetle-style progressive disclosure with public/internal separation.

**Status:** Phase 2 Complete (30% overall progress)

---

## ✅ Phase 1: Infrastructure (COMPLETE)

**Completed:**
- [x] Created `/docs` directory structure (6 sections: start, concepts, coding, operating, reference, internals)
- [x] Created `/docs-internal` directory structure (vopr, contributing, design-docs, internal)
- [x] Created README.md for all sections (/docs/README.md, /docs-internal/README.md, + section READMEs)
- [x] Set up doc-testing infrastructure (tests/doc_tests/lib.rs)
- [x] Added `doc-comment` dependency to Cargo.toml
- [x] Updated Justfile with `test-docs` command
- [x] Integrated `test-docs` into `pre-commit` checks

**Files Created:**
- /docs/README.md (master index)
- /docs-internal/README.md (internal docs index)
- /docs/start/README.md
- /docs/concepts/README.md
- /docs/coding/README.md
- /docs/operating/README.md
- /docs/reference/README.md
- /docs/internals/README.md
- tests/doc_tests/lib.rs

---

## ✅ Phase 2: Start & Reference Sections (COMPLETE)

**Completed:**
- [x] Created Start section (3 files)
  - quick-start.md - Accurate to current v0.4.0 state
  - installation.md - All platforms, troubleshooting
  - first-app.md - Healthcare compliance example
- [x] Created Reference/CLI documentation
  - vopr.md - ALL 10 commands documented (run, repro, show, scenarios, stats, timeline, bisect, minimize, dashboard, tui)
  - overview.md - CLI tools overview
- [x] Created internal VOPR scenarios documentation
  - /docs-internal/vopr/scenarios.md - ALL 46 scenarios documented across 11 phases
- [x] Updated CLAUDE.md with correct counts
  - Fixed: 27 → 46 scenarios
  - Fixed: 5 → 10 CLI commands
  - Added new documentation structure explanation

**Files Created:**
- /docs/start/quick-start.md (condensed from getting-started.md, accurate to v0.4.0)
- /docs/start/installation.md (extracted and expanded)
- /docs/start/first-app.md (NEW - healthcare example)
- /docs/reference/cli/vopr.md (NEW - comprehensive, all 10 commands)
- /docs/reference/cli/overview.md (NEW)
- /docs-internal/vopr/scenarios.md (NEW - all 46 scenarios)

**Files Updated:**
- CLAUDE.md (updated scenario count, CLI command count, documentation layout)
- Cargo.toml (added doc-comment dependency)
- justfile (added test-docs command)

---

## ✅ Phase 3: Concepts Section (COMPLETE)

**Status:** Complete

**Completed:**
- [x] Created concepts/overview.md (elevator pitch, 1-page intro)
- [x] Split ARCHITECTURE.md → 10 files:
  - [x] concepts/architecture.md (high-level overview)
  - [x] concepts/data-model.md (append-only log, projections, streams)
  - [x] concepts/consensus.md (VSR protocol explained)
  - [x] concepts/multitenancy.md (tenant isolation, encryption, regional placement)
  - [x] concepts/compliance.md (compliance by construction)
  - [x] internals/architecture/crate-structure.md (30 crates, 5 layers)
  - [x] internals/architecture/kernel.md (pure functional state machine)
  - [x] internals/architecture/storage.md (append-only log format)
  - [x] internals/architecture/crypto.md (dual-hash, envelope encryption)
- [x] Moved PRESSURECRAFT.md:
  - [x] concepts/pressurecraft.md (git mv, preserving history)

**Files Created/Moved:** 10 files

**Note:** COMPLIANCE.md full technical details will move to internals/ in Phase 6.

---

## 🚧 Phase 4: Coding Section (TODO)

**Status:** Not started

**Tasks:**
- [ ] Move quickstart guides from docs/guides/ to docs/coding/quickstarts/:
  - [ ] quickstarts/python.md (from guides/quickstart-python.md)
  - [ ] quickstarts/typescript.md (from guides/quickstart-typescript.md)
  - [ ] quickstarts/rust.md (from guides/quickstart-rust.md)
  - [ ] quickstarts/go.md (from guides/quickstart-go.md)
- [ ] Move other guides:
  - [ ] guides/connection-pooling.md (from guides/connection-pooling.md)
  - [ ] guides/migrations.md (extract from getting-started.md)
  - [ ] guides/testing.md (from guides/testing-guide.md)
  - [ ] guides/shell-completions.md (from guides/shell-completions.md)
- [ ] Create recipe files (NEW):
  - [ ] recipes/time-travel-queries.md (extract from getting-started.md)
  - [ ] recipes/audit-trails.md (extract from COMPLIANCE.md)
  - [ ] recipes/encryption.md (extract from COMPLIANCE.md)
  - [ ] recipes/data-classification.md (NEW)
  - [ ] recipes/multi-tenant-queries.md (NEW)
- [ ] Move migration guide:
  - [ ] migration-guide.md (from guides/migration-guide.md)
- [ ] Add doc-tests for code examples in all files

**Files to Create/Move:** 15 files

---

## 🚧 Phase 5: Operating Section (TODO)

**Status:** Not started

**Tasks:**
- [ ] Move deployment/security/performance docs:
  - [ ] deployment.md (from DEPLOYMENT.md - KEEP MOST AS-IS)
  - [ ] configuration.md (extract from OPERATIONS.md)
  - [ ] monitoring.md (from INSTRUMENTATION_DESIGN.md - implemented sections only)
  - [ ] security.md (from SECURITY.md - KEEP AS-IS)
  - [ ] performance.md (from PERFORMANCE.md - remove "Future:" sections)
- [ ] Create new files:
  - [ ] troubleshooting.md (NEW - common issues)
  - [ ] cloud/aws.md (from VOPR_DEPLOYMENT.md)
  - [ ] cloud/gcp.md (NEW - placeholder)
  - [ ] cloud/azure.md (NEW - placeholder)
- [ ] Remove "Future:" sections from all files
- [ ] Replace with links to ROADMAP.md

**Files to Create/Move:** 9 files

---

## 🚧 Phase 6: Internals & Internal Docs (TODO)

**Status:** Partially complete (scenarios.md created)

**Tasks:**
- [ ] Split TESTING.md:
  - [ ] internals/testing/overview.md (public testing overview, lines 1-300)
  - [ ] internals/testing/assertions.md (from ASSERTIONS.md - KEEP AS-IS)
  - [ ] internals/testing/property-testing.md (proptest overview)
  - [ ] docs-internal/vopr/overview.md (VOPR deep dive, lines 300-1500)
  - [x] docs-internal/vopr/scenarios.md (ALL 46 scenarios) ✅ DONE
  - [ ] docs-internal/vopr/deployment.md (from VOPR_DEPLOYMENT.md)
  - [ ] docs-internal/vopr/debugging.md (advanced debugging)
  - [ ] docs-internal/vopr/writing-scenarios.md (how to add scenarios)
- [ ] Move design docs to internals/design/:
  - [ ] instrumentation.md (from INSTRUMENTATION_DESIGN.md - KEEP AS-IS)
  - [ ] reconfiguration.md (from RECONFIGURATION_DESIGN.md - KEEP AS-IS)
  - [ ] llm-integration.md (from LLM_INTEGRATION_DESIGN.md - KEEP AS-IS)
  - [ ] data-sharing.md (from DATA_SHARING.md - KEEP AS-IS)
- [ ] Move VSR production gaps:
  - [ ] internals/vsr-production-gaps.md (from VSR_PRODUCTION_GAPS.md - KEEP AS-IS)
- [ ] Create contributor docs:
  - [ ] docs-internal/contributing/getting-started.md (NEW)
  - [ ] docs-internal/contributing/code-review.md (NEW)
  - [ ] docs-internal/contributing/testing-strategy.md (from TESTING.md detailed sections)
  - [ ] docs-internal/contributing/release-process.md (NEW)
- [ ] Move internal materials:
  - [ ] docs-internal/internal/cloud-architecture.md (from CLOUD_ARCHITECTURE.md)
  - [ ] docs-internal/internal/bug-bounty.md (from BUG_BOUNTY.md - or move to public)

**Files to Create/Move:** 18 files (1 done)

---

## 🚧 Phase 7: Polish & Deploy (TODO)

**Status:** Not started

**Tasks:**
- [ ] Set up 301 redirects for old paths
- [ ] Update all cross-references between docs
- [ ] Run link checker (no broken links)
- [ ] Verify CLAUDE.md is fully updated
- [ ] Deploy website with new docs
- [ ] Test doc-tests in CI
- [ ] Create reference/sql/ documentation:
  - [ ] overview.md (from SQL_ENGINE.md lines 1-50)
  - [ ] ddl.md (CREATE/DROP sections)
  - [ ] dml.md (INSERT/UPDATE/DELETE sections)
  - [ ] queries.md (SELECT section)
- [ ] Create reference/sdk/ documentation:
  - [ ] overview.md (from SDK.md lines 1-100)
  - [ ] python-api.md (from SDK.md Python section)
  - [ ] typescript-api.md (from SDK.md TypeScript section)
  - [ ] rust-api.md (from SDK.md Rust section)
  - [ ] go-api.md (from SDK.md Go section - mark as "Planned")
- [ ] Move protocol docs:
  - [ ] reference/protocol.md (from PROTOCOL.md - KEEP AS-IS)
  - [ ] reference/agent-protocol.md (from AGENT_PROTOCOL.md - KEEP AS-IS)

**Files to Create/Move:** 13 files

---

## Summary Statistics

**Overall Progress:** ~45% (3 of 7 phases complete)

**Files Created:** 26 files
- Directory structure: 25 directories
- README files: 8 files
- Start section: 3 files
- Reference/CLI: 2 files
- Internal/VOPR: 1 file (scenarios)
- Infrastructure: 1 file (doc_tests/lib.rs)

**Files Updated:** 3 files
- CLAUDE.md (scenario/CLI counts, documentation layout)
- Cargo.toml (doc-comment dependency)
- justfile (test-docs command)

**Remaining Work:**
- Phase 4 (Coding): 15 files
- Phase 5 (Operating): 9 files
- Phase 6 (Internals/Internal): 17 files (1 done)
- Phase 7 (Polish/Deploy): 13 files

**Total Remaining:** ~53 files to create/migrate/update

---

## Critical Accomplishments

### ✅ Fixed Accuracy Issues

1. **VOPR Scenarios:** 27 → 46 (100% coverage)
   - Documented all 11 phases
   - Each scenario includes purpose, faults, invariants, usage
   - Internal contributor documentation created

2. **VOPR CLI Commands:** 5 → 10 (100% coverage)
   - All commands documented with full syntax, options, examples
   - Integration with Justfile documented
   - Output formats explained
   - Exit codes standardized

3. **CLAUDE.md Updated:**
   - Correct scenario count (46)
   - Correct CLI command count (10)
   - New documentation structure documented
   - Public vs internal separation explained

### ✅ Infrastructure Established

1. **Directory Structure:** TigerBeetle-style progressive disclosure
   - /docs (public user-facing)
   - /docs-internal (contributors only)
   - 6 public sections + 1 internal section

2. **Doc-Testing:** Infrastructure ready
   - tests/doc_tests/lib.rs created
   - doc-comment dependency added
   - Justfile integration (test-docs, pre-commit)

3. **Navigation:** README files provide clear entry points
   - Master index at /docs/README.md
   - Section indexes for each category
   - Internal docs index at /docs-internal/README.md

---

## Next Steps (Recommendations)

### Immediate (Phase 3)
1. Create concepts/overview.md (elevator pitch)
2. Split ARCHITECTURE.md into concepts + internals files
3. Move PRESSURECRAFT.md to concepts/

### High Priority (Phase 4 & 5)
4. Move quickstart guides to coding/quickstarts/
5. Create recipe files (time-travel, audit-trails, encryption)
6. Move deployment/security docs to operating/
7. Create troubleshooting.md

### Before v0.5.0 Release (Phase 6 & 7)
8. Split TESTING.md (public overview + internal details)
9. Create contributor guides (getting-started, code-review)
10. Set up website integration (markdown rendering)
11. Add 301 redirects for old paths

---

## Dependencies

- **Website Integration** depends on:
  - All documentation files migrated
  - Cross-references updated
  - Link validation passing

- **Doc-Tests** depend on:
  - Code examples in documentation using proper annotations
  - Examples compile against current codebase

- **Phase 7 (Polish)** depends on:
  - Phases 3-6 complete
  - All cross-references identified

---

**Last Updated:** 2026-02-05
**Phases Complete:** 3 / 7 (43%)
**Files Created:** 26 / ~78 (33%)
