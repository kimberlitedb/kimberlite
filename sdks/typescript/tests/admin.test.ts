/**
 * Unit tests for the TS admin wrapper. Shape checks only — end-to-end
 * validation against a live server lives in the Phase 8 framework example
 * suite.
 */

import { AdminNamespace } from '../src/admin';

describe('AdminNamespace surface', () => {
  it('exposes schema + tenant + api-key + server-info methods', () => {
    const proto = AdminNamespace.prototype;
    // Schema
    expect(typeof proto.listTables).toBe('function');
    expect(typeof proto.describeTable).toBe('function');
    expect(typeof proto.listIndexes).toBe('function');
    // Tenant lifecycle
    expect(typeof proto.createTenant).toBe('function');
    expect(typeof proto.listTenants).toBe('function');
    expect(typeof proto.deleteTenant).toBe('function');
    expect(typeof proto.getTenant).toBe('function');
    // API keys
    expect(typeof proto.issueApiKey).toBe('function');
    expect(typeof proto.revokeApiKey).toBe('function');
    expect(typeof proto.listApiKeys).toBe('function');
    expect(typeof proto.rotateApiKey).toBe('function');
    // Server info
    expect(typeof proto.serverInfo).toBe('function');
  });

  it('construction takes a native client (non-null)', () => {
    const fake: any = {
      listTables: async () => [],
      serverInfo: async () => ({
        buildVersion: '0.5.0',
        protocolVersion: 2,
        capabilities: ['admin.v1'],
        uptimeSecs: 0n,
        clusterMode: 'Standalone',
        tenantCount: 0,
      }),
    };
    const admin = new AdminNamespace(fake);
    expect(admin).toBeInstanceOf(AdminNamespace);
  });
});

describe('AdminNamespace error wrapping', () => {
  it('wraps native errors through wrapNativeError', async () => {
    const fake: any = {
      listTables: async () => {
        throw new Error('[KMB_ERR_AuthenticationFailed] admin operations require Admin');
      },
    };
    const admin = new AdminNamespace(fake);
    await expect(admin.listTables()).rejects.toMatchObject({
      name: expect.stringMatching(/Authentication/),
    });
  });

  it('surfaces tenant-already-exists as a ServerError with the right code', async () => {
    const fake: any = {
      tenantCreate: async () => {
        throw new Error('[KMB_ERR_TenantAlreadyExists] tenant already registered');
      },
    };
    const admin = new AdminNamespace(fake);
    try {
      await admin.createTenant(1n, 'acme');
      throw new Error('expected rejection');
    } catch (e: any) {
      expect(e.code).toBe('TenantAlreadyExists');
    }
  });

  it('surfaces api-key-not-found through revoke', async () => {
    const fake: any = {
      apiKeyRevoke: async () => {
        throw new Error('[KMB_ERR_ApiKeyNotFound] API key not found');
      },
    };
    const admin = new AdminNamespace(fake);
    try {
      await admin.revokeApiKey('kmb_live_bogus');
      throw new Error('expected rejection');
    } catch (e: any) {
      expect(e.code).toBe('ApiKeyNotFound');
    }
  });
});
