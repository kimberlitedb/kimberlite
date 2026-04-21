/**
 * `@kimberlitedb/client/testing` — v0.5.1 test harness wrapper.
 *
 * Replaces downstream `FakeKimberlite` regex-SQL fakes with a real
 * in-process Kimberlite instance so every SQL feature (`ILIKE`, `NOT
 * IN`, `UPPER(col)`, `CAST`, `||`, scalar projections) is immediately
 * testable against the real parser + planner + executor. No "passes
 * in test, breaks in prod" class of bugs.
 *
 * Under the hood the wrapper spawns the
 * `kimberlite-test-harness-cli` binary shipped with the Rust
 * workspace, reads `ADDR=127.0.0.1:<port>` from stdout, connects a
 * normal `Client`, and returns a handle. Disposal terminates the
 * child process which in turn shuts the harness down deterministically
 * (the tempdir is cleaned up, the polling thread joined).
 *
 * @example
 *
 * ```ts
 * import { createTestKimberlite, disposeTestKimberlite } from '@kimberlitedb/client/testing';
 *
 * const harness = await createTestKimberlite();
 * try {
 *   await harness.client.execute('CREATE TABLE t (id BIGINT PRIMARY KEY, name TEXT)', []);
 *   await harness.client.execute('INSERT INTO t (id, name) VALUES ($1, $2)', [1n, 'Ada']);
 *   const rs = await harness.client.query("SELECT UPPER(name) FROM t WHERE id = 1", []);
 *   expect(rs.rows[0]).toEqual(['ADA']);
 * } finally {
 *   await disposeTestKimberlite(harness);
 * }
 * ```
 *
 * @packageDocumentation
 */

import { spawn, ChildProcess } from 'node:child_process';
import * as readline from 'node:readline';
import { Client } from './client';

/**
 * Options for {@link createTestKimberlite}.
 */
export interface TestKimberliteOptions {
  /**
   * Override the tenant id the harness client binds to. Defaults to
   * `1_000_000` — the same value the Rust crate uses.
   */
  tenant?: bigint | number;
  /**
   * Path to the harness launcher binary. Defaults to the
   * `KIMBERLITE_TEST_HARNESS_BIN` environment variable, falling back
   * to `kimberlite-test-harness-cli` on `$PATH`. CI pipelines should
   * set this explicitly to the workspace-local
   * `target/debug/kimberlite-test-harness-cli` to avoid PATH lookup.
   */
  binaryPath?: string;
  /**
   * Timeout (ms) for the child process to emit its `ADDR=` line.
   * Default: 15s — matches the harness's cold spin-up budget (<50ms
   * on CI) with plenty of slack for slow-boot Docker runners.
   */
  readyTimeoutMs?: number;
}

/**
 * Handle returned by {@link createTestKimberlite}. Pass to
 * {@link disposeTestKimberlite} at teardown.
 */
export interface TestKimberlite {
  /** Loopback address the harness bound to. */
  addr: string;
  /** Tenant id the client is scoped to. */
  tenant: bigint;
  /** Ready-to-use SDK client. Use exactly as in production. */
  client: Client;
  /**
   * Private handle to the child process. Callers should invoke
   * {@link disposeTestKimberlite} — direct `.kill()` bypasses the
   * graceful-shutdown path.
   *
   * @internal
   */
  _child: ChildProcess;
}

const DEFAULT_BINARY = 'kimberlite-test-harness-cli';
const DEFAULT_READY_TIMEOUT_MS = 15_000;

/**
 * Spawn a fresh in-process Kimberlite instance and return a connected
 * client + metadata. One harness per test is the recommended shape;
 * the cold spin-up is < 50ms on CI.
 */
export async function createTestKimberlite(
  options: TestKimberliteOptions = {},
): Promise<TestKimberlite> {
  const binary =
    options.binaryPath ?? process.env.KIMBERLITE_TEST_HARNESS_BIN ?? DEFAULT_BINARY;
  const readyTimeout = options.readyTimeoutMs ?? DEFAULT_READY_TIMEOUT_MS;

  const args: string[] = [];
  if (options.tenant !== undefined) {
    args.push(`--tenant=${BigInt(options.tenant).toString()}`);
  }

  const child = spawn(binary, args, {
    stdio: ['pipe', 'pipe', 'inherit'],
  });
  if (child.stdout === null || child.stdin === null) {
    throw new Error('spawn returned a child with null stdio pipes; check TestKimberliteOptions.binaryPath');
  }

  // Read the first two machine-readable lines.
  const rl = readline.createInterface({ input: child.stdout });
  let addr: string | undefined;
  let tenant: bigint | undefined;

  const ready = new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(
        new Error(
          `kimberlite-test-harness-cli did not emit ADDR within ${readyTimeout}ms`,
        ),
      );
    }, readyTimeout);

    rl.on('line', (line) => {
      if (line.startsWith('ADDR=')) {
        addr = line.slice('ADDR='.length).trim();
      } else if (line.startsWith('TENANT=')) {
        tenant = BigInt(line.slice('TENANT='.length).trim());
      }
      if (addr !== undefined && tenant !== undefined) {
        clearTimeout(timer);
        resolve();
      }
    });

    child.once('error', (err) => {
      clearTimeout(timer);
      reject(err);
    });
    child.once('exit', (code) => {
      clearTimeout(timer);
      if (addr === undefined) {
        reject(
          new Error(
            `kimberlite-test-harness-cli exited early with code ${code ?? '<none>'} before emitting ADDR`,
          ),
        );
      }
    });
  });

  await ready;
  if (addr === undefined || tenant === undefined) {
    // ready promise resolved via both values being set — this is
    // defensive paranoia against flow-sensitivity gaps.
    throw new Error('internal: ready resolved without addr/tenant');
  }

  // Stop piping stdout into readline now that we have what we need;
  // subsequent lines are tracing output the parent can ignore or
  // capture separately.
  rl.close();
  child.stdout!.resume();

  const client = await Client.connect({
    addresses: [addr],
    tenantId: tenant,
  });

  return {
    addr,
    tenant,
    client,
    _child: child,
  };
}

/**
 * Dispose a harness returned by {@link createTestKimberlite}. Sends
 * the `shutdown` IPC signal and waits for the child to exit.
 */
export async function disposeTestKimberlite(harness: TestKimberlite): Promise<void> {
  // Best-effort graceful path: write `shutdown\n` to stdin. If the
  // child is already gone, `.end()` throws synchronously — caller
  // shouldn't care in that case.
  try {
    harness._child.stdin?.end('shutdown\n');
  } catch {
    // fall through to kill()
  }

  try {
    await harness.client.disconnect();
  } catch {
    // Client already closed by the child exiting. Ignore.
  }

  await new Promise<void>((resolve) => {
    let done = false;
    const finish = () => {
      if (!done) {
        done = true;
        resolve();
      }
    };
    harness._child.once('exit', finish);
    // 5s upper bound — then SIGKILL and move on.
    setTimeout(() => {
      if (!done) {
        try {
          harness._child.kill('SIGKILL');
        } catch {
          // already gone
        }
        finish();
      }
    }, 5_000).unref();
  });
}
