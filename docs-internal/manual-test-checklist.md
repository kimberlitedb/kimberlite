# Manual Test Checklist — v0.4 OSS Launch

Run top-to-bottom in one sitting (~90 minutes) to validate every user-facing
surface Kimberlite ships. Intended for maintainers pre-release, not end-users.

Each section is independent — skip any you've recently verified. Check boxes
as you go; open issues for anything that fails.

**Setup for the whole checklist:**

```bash
# In one terminal (leave running):
cd ~/your/kimberlite/checkout
cargo build --release -p kimberlite-cli
export PATH="$PWD/target/release:$PATH"
```

---

## A. Install & init (~10 min)

- [ ] `curl -fsSL https://kimberlite.dev/install.sh | sh` runs to completion on a fresh shell (or `install.sh` at repo root works with `./install.sh`).
- [ ] `kimberlite --version` prints the current workspace version.
- [ ] `kimberlite init /tmp/kimberlite-check-demo --template default --yes` completes silently and produces `/tmp/kimberlite-check-demo/.kimberlite/`.
- [ ] `kimberlite init /tmp/kimberlite-check-wizard` (no flags) runs the interactive wizard; diamond-styled prompts render; every template option produces a runnable project.
- [ ] `kimberlite-check-demo/kimberlite.toml` is readable + well-commented.

## B. Dev server + Studio UI (~10 min)

- [ ] `cd /tmp/kimberlite-check-demo && kimberlite dev` prints the three-service banner:
  - Database on `127.0.0.1:5432`
  - Studio on `http://127.0.0.1:5555`
  - HTTP sidecar (metrics) on `127.0.0.1:9090`
- [ ] `curl -sf http://127.0.0.1:9090/metrics | head` returns Prometheus metrics.
- [ ] `curl -sf http://127.0.0.1:9090/health` returns 200 OK.
- [ ] Studio landing page loads; schema browser visible; SQL editor accepts `SELECT 1`.
- [ ] Studio audit view shows every write from step D below in chronological order.
- [ ] `Ctrl+C` kills all three services cleanly.
- [ ] Verify with `lsof -iTCP:5432 -iTCP:5555 -iTCP:9090 -sTCP:LISTEN` — no orphans.

## C. REPL + SQL coverage (~15 min)

Connect:

- [ ] `kimberlite repl --tenant 1 --address 127.0.0.1:5432` connects; banner shows server + tenant.
- [ ] Tab completion works on `SEL<tab>` → `SELECT`.
- [ ] Multi-line input triggers the `...>` continuation prompt on lines without `;`.

Apply the healthcare example schema (see `examples/healthcare/00-setup.sh`), then:

- [ ] `SELECT * FROM patients;` returns the seed rows.
- [ ] `SELECT name FROM patients WHERE id = $1;` with a parameter works (use the SDK or CLI args).
- [ ] CTE: `WITH recent AS (SELECT * FROM encounters WHERE encounter_date > '2024-01-01 00:00:00') SELECT COUNT(*) FROM recent;`
- [ ] CASE WHEN: `SELECT name, CASE WHEN dob < '1980-01-01' THEN 'senior' ELSE 'adult' END AS bucket FROM patients;`
- [ ] BETWEEN: `SELECT * FROM encounters WHERE encounter_date BETWEEN '2024-01-01' AND '2024-06-30';`
- [ ] LEFT JOIN: `SELECT p.name, COUNT(e.id) AS visits FROM patients p LEFT JOIN encounters e ON e.patient_id = p.id GROUP BY p.name;` returns patients with visit counts (including zero).
- [ ] AS OF TIMESTAMP: after inserting a row, query `SELECT * FROM patients AS OF TIMESTAMP '2024-01-01 00:00:00';` returns the historical state.
- [ ] AT OFFSET: capture a `client.lastRequestId` from an SDK call, then `SELECT * FROM patients AT OFFSET <n>;` returns pre-write state.
- [ ] Window function attempt: `SELECT id, ROW_NUMBER() OVER (ORDER BY id) FROM patients;` — fails with a clear "not supported" error, not a silent wrong answer.
- [ ] `WITH RECURSIVE ...` — fails with "WITH RECURSIVE is not supported".

## D. SDK round-trip (~20 min)

Start the healthcare example server:

```bash
examples/healthcare/00-setup.sh
```

### Rust

- [ ] `cd examples/rust && cargo run --example basic` completes.
- [ ] `cargo run --example time_travel` completes.
- [ ] `cargo run --example clinic` prints the full walkthrough (admin.listTables → typed query → consent → erasure → pool stats → `✅ clinic walkthrough complete`).

### TypeScript (validates Node 18/20/22/24 support)

- [ ] `cd sdks/typescript && npm install && npm run build:native && npm run build` — clean.
- [ ] `npm test` — all unit + integration tests pass.
- [ ] `node --require ts-node/register examples/healthcare/clinic.ts` prints the full walkthrough.
- [ ] (Optional) repeat with `nvm use 18`, `nvm use 20`, `nvm use 22`, `nvm use 24` — each one runs the above without recompiling.
- [ ] `node -e "require('@kimberlitedb/client')"` in a fresh temp dir after `npm install @kimberlitedb/client` works (or use `npm install --save ../kimberlite/sdks/typescript` for a local build).

### Python

- [ ] `pip install -e sdks/python` (fresh venv) installs without errors.
- [ ] `pytest sdks/python/tests/` — all pass.
- [ ] `python examples/healthcare/clinic.py` prints the full walkthrough.

### First-app tutorial (covers all three)

- [ ] Follow `docs/start/first-app.md` end-to-end in a brand-new temp dir; output matches the "Expected output" block in the doc.

## E. Compliance primitives (~15 min)

Against the healthcare server (`examples/healthcare/00-setup.sh`):

- [ ] **Two-tenant isolation.** Create tenant 2 via `client.admin.tenant.create(2n, 'other-clinic')`, connect a client as tenant 2, and confirm `SELECT * FROM patients` returns 0 rows for tenant 2 even though tenant 1 has data.
- [ ] **Consent grant/check/withdraw round-trip** via TS SDK: grant Research consent for `patient:1`, check returns `true`, withdraw, check returns `false`.
- [ ] **Erasure flow.** Request erasure for `patient:1`, `markProgress`, verify `erasure.status` shows `InProgress`. (Completion is application-side; don't block the checklist on it.)
- [ ] **Audit trail.** Query `audit_log` — every prior action should have a row with actor + action + resource + timestamp.
- [ ] **RBAC / masking (via SQL-level rewriting).** Connect as a `User` role (issue an API key with `admin.issueApiKey({ roles: ['User'], … })`), confirm queries are scoped to that user's `access_grants`.

## F. CLI utilities (~5 min)

- [ ] `kimberlite info --server 127.0.0.1:5432` prints version + health.
- [ ] `kimberlite query --tenant 1 --server 127.0.0.1:5432 "SELECT 1"` prints the scalar result.
- [ ] `kimberlite compliance --help` lists available sub-commands; pick one and confirm it doesn't panic.
- [ ] `just --list` runs; spot-check 5 recipes succeed.

## G. Testing & simulation (~10 min)

- [ ] `just test` — full workspace test suite passes.
- [ ] `just nextest` — same via nextest (faster).
- [ ] `just vopr-quick` — 100-iteration smoke test is green.
- [ ] `just vopr-scenario baseline 10000` — 10k-iteration baseline scenario green, prints sims/sec.
- [ ] `just fuzz-smoke` — 1-minute fuzz run, no crashes.
- [ ] `just clippy` — clean workspace lint.

## H. Docker (~5 min)

- [ ] `docker build -t kimberlite-test .` builds the release image.
- [ ] `docker run --rm -p 5432:5432 kimberlite-test` starts; `kimberlite info --server 127.0.0.1:5432` from host succeeds.
- [ ] `docker pull ghcr.io/kimberlitedb/kimberlite:latest` works (or confirm the latest tag is published on release).

## I. Website + docs (~5 min)

- [ ] Visit `https://kimberlite.dev` and scan landing + docs pages for broken links / placeholder text.
- [ ] `lychee --config .lycheeignore docs/ README.md` — no broken internal links.
- [ ] Run the healthcare-example README commands verbatim; every fenced `bash` block executes without edits.
- [ ] `grep -r "Coming soon\|TBD\|XXX" docs/ README.md` — any hits should have a version target (e.g. "v0.5.0").

## J. Release sanity (~5 min)

- [ ] **Workspace `Cargo.toml` version matches the CHANGELOG entry.** Historically these have drifted — bump `[workspace.package].version` before tagging.
- [ ] TS SDK `sdks/typescript/package.json` version + Python `sdks/python/pyproject.toml` version match the workspace version.
- [ ] `just publish-dry-run` — walks the crates.io + npm + PyPI release pipeline without actually publishing.
- [ ] `git log --oneline v<PREV>..HEAD` matches the CHANGELOG entry for the target release (inspect by hand; look for anything that belongs in a different version).
- [ ] CHANGELOG entry for the release version has at least: `Theme`, `Breaking changes`, `New features`, `Deprecations` sections.
- [ ] `docs/reference/sdk/parity.md` is current for the release — spot-check 3 rows against the SDK source.
- [ ] README status block mentions the current version's breaking-change posture (or points at CHANGELOG).

---

## On failure

1. Capture the failing command, full output, and environment (`uname -a`, `node --version`, `kimberlite --version`).
2. File under `.github/ISSUE_TEMPLATE/bug_report.yml`.
3. If the failure is in a compliance primitive, tag `area:compliance`; if it's a SQL/parser issue, `area:query`; etc.
4. If a failure blocks the launch, add the fix to the release plan in `ROADMAP.md` and block the tag.

Green across the whole checklist is the pre-condition for cutting a release tag.
