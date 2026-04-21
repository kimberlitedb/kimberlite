/**
 * ROADMAP v0.5.1 — smoke test for `@kimberlitedb/client/testing`.
 *
 * Skipped by default because it requires the
 * `kimberlite-test-harness-cli` binary to be built and on `$PATH`
 * (or pointed at via `KIMBERLITE_TEST_HARNESS_BIN`). CI sets
 * `KIMBERLITE_TEST_HARNESS_BIN=target/debug/kimberlite-test-harness-cli`
 * after `cargo build -p kimberlite-test-harness --bin
 * kimberlite-test-harness-cli` and drops the skip.
 *
 * Mirrors the Python gate in `sdks/python/tests/test_harness.py`:
 * only runs when the bin path actually resolves on disk.
 */

import * as fs from 'node:fs';
import { createTestKimberlite, disposeTestKimberlite } from '../src/testing';

function harnessAvailable(): boolean {
  const bin = process.env.KIMBERLITE_TEST_HARNESS_BIN;
  return Boolean(bin && fs.existsSync(bin));
}

const runIfHarnessAvailable = harnessAvailable() ? describe : describe.skip;

runIfHarnessAvailable('TestKimberlite smoke', () => {
  it('creates a harness, runs a SELECT, disposes cleanly', async () => {
    const harness = await createTestKimberlite({ tenant: 42n });
    try {
      expect(harness.addr).toMatch(/^127\.0\.0\.1:\d+$/);
      expect(harness.tenant).toBe(42n);

      await harness.client.execute(
        'CREATE TABLE t (id BIGINT PRIMARY KEY, name TEXT NOT NULL)',
        [],
      );
      await harness.client.execute('INSERT INTO t (id, name) VALUES ($1, $2)', [
        1n,
        'Ada',
      ]);
      const rs = await harness.client.query(
        'SELECT UPPER(name) FROM t WHERE id = $1',
        [1n],
      );
      expect(rs.rows.length).toBe(1);
      // Output column layout is [upper] — a single scalar projection.
      expect(rs.rows[0]).toEqual(['ADA']);
    } finally {
      await disposeTestKimberlite(harness);
    }
  }, 20_000);
});
