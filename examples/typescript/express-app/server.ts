/**
 * Express.js integration example.
 *
 * Exercises: connect (pooled) + query + consent + audit.
 *
 * Run:
 *     npm install
 *     npm start
 *
 * Endpoints:
 *     GET  /health
 *     POST /patients       { "name": "...", "consentPurpose": "Analytics" }
 *     GET  /patients/:id
 *     GET  /info
 */

import express from 'express';
import { Pool, ConsentPurpose } from '@kimberlitedb/client';

async function main(): Promise<void> {
  const pool = await Pool.create({
    address: '127.0.0.1:5432',
    tenantId: 1n,
    maxSize: 8,
  });

  const app = express();
  app.use(express.json());

  app.get('/health', (_req, res) => {
    res.send('ok');
  });

  app.get('/info', async (_req, res) => {
    try {
      const info = await pool.withClient((client) => client.admin.serverInfo());
      res.json({
        version: info.buildVersion,
        uptimeSecs: info.uptimeSecs.toString(),
        capabilities: info.capabilities,
      });
    } catch (e: any) {
      res.status(500).send(e.message);
    }
  });

  app.post('/patients', async (req, res) => {
    const { name, consentPurpose } = req.body as {
      name: string;
      consentPurpose: ConsentPurpose;
    };
    if (!name || !consentPurpose) {
      res.status(400).send('name + consentPurpose required');
      return;
    }
    try {
      const grant = await pool.withClient((client) =>
        client.compliance.consent.grant(name, consentPurpose),
      );
      res.status(201).json({ id: name, consentId: grant.consentId });
    } catch (e: any) {
      res.status(500).send(e.message);
    }
  });

  app.get('/patients/:id', async (req, res) => {
    try {
      const hasConsent = await pool.withClient((client) =>
        client.compliance.consent.check(req.params.id, 'Analytics'),
      );
      res.json({ id: req.params.id, analyticsConsent: hasConsent });
    } catch (e: any) {
      res.status(500).send(e.message);
    }
  });

  const port = process.env.PORT ?? 3002;
  app.listen(port, () => {
    console.log(`express example listening on http://0.0.0.0:${port}`);
  });
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
