/**
 * POST /api/patients — grant consent + create a patient record.
 *
 * Body: { name: string, consentPurpose: "Marketing" | "Analytics" | ... }
 */

import { NextRequest, NextResponse } from 'next/server';
import { getPool } from '../../kimberlite';

export async function POST(req: NextRequest) {
  const { name, consentPurpose } = (await req.json()) as {
    name: string;
    consentPurpose: string;
  };
  if (!name || !consentPurpose) {
    return NextResponse.json({ error: 'name + consentPurpose required' }, { status: 400 });
  }
  try {
    const pool = await getPool();
    const grant = await pool.withClient((c) =>
      c.compliance.consent.grant(name, consentPurpose as any),
    );
    return NextResponse.json({ id: name, consentId: grant.consentId }, { status: 201 });
  } catch (e: any) {
    return NextResponse.json({ error: e.message }, { status: 500 });
  }
}
