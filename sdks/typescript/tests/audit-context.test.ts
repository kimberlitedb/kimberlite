/**
 * Tests for audit-context propagation.
 *
 * AUDIT-2026-04 S2.4 — pins the AsyncLocalStorage behaviour so
 * nested async calls see the right context.
 */

import {
  AuditContext,
  currentAudit,
  requireAudit,
  runWithAudit,
} from '../src/audit-context';

const ALICE: AuditContext = {
  actor: 'alice@example.com',
  reason: 'chart-review',
  correlationId: 'req-123',
};

const BOB: AuditContext = {
  actor: 'bob@example.com',
  reason: 'break-glass',
};

describe('audit context', () => {
  it('returns undefined when no context is active', () => {
    expect(currentAudit()).toBeUndefined();
  });

  it('runWithAudit exposes the context to the inner callable', () => {
    const result = runWithAudit(ALICE, () => currentAudit());
    expect(result).toEqual(ALICE);
  });

  it('context is cleared after the runWithAudit scope exits', () => {
    runWithAudit(ALICE, () => undefined);
    expect(currentAudit()).toBeUndefined();
  });

  it('nested runWithAudit overrides the outer context', () => {
    runWithAudit(ALICE, () => {
      const outer = currentAudit();
      expect(outer?.actor).toBe('alice@example.com');

      runWithAudit(BOB, () => {
        const inner = currentAudit();
        expect(inner?.actor).toBe('bob@example.com');
        expect(inner?.reason).toBe('break-glass');
      });

      // On return from the inner scope, the outer context is
      // restored.
      const restored = currentAudit();
      expect(restored?.actor).toBe('alice@example.com');
    });
  });

  it('context survives await boundaries', async () => {
    await runWithAudit(ALICE, async () => {
      expect(currentAudit()?.actor).toBe('alice@example.com');
      await Promise.resolve();
      // Still visible after the await.
      expect(currentAudit()?.actor).toBe('alice@example.com');
      // And after a microtask.
      await new Promise((resolve) => setTimeout(resolve, 1));
      expect(currentAudit()?.actor).toBe('alice@example.com');
    });
  });

  it('requireAudit throws when no context is active', () => {
    expect(() => requireAudit()).toThrow(/no audit context active/);
  });

  it('requireAudit returns the context when one is active', () => {
    runWithAudit(ALICE, () => {
      expect(requireAudit()).toEqual(ALICE);
    });
  });

  it('parallel runWithAudit calls do not cross-contaminate', async () => {
    const results = await Promise.all([
      runWithAudit(ALICE, async () => {
        await new Promise((resolve) => setTimeout(resolve, 5));
        return currentAudit()?.actor;
      }),
      runWithAudit(BOB, async () => {
        await new Promise((resolve) => setTimeout(resolve, 1));
        return currentAudit()?.actor;
      }),
    ]);
    expect(results).toEqual(['alice@example.com', 'bob@example.com']);
  });
});
