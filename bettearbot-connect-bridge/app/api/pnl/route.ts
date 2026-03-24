import { NextRequest, NextResponse } from 'next/server';
import { nanoid } from 'nanoid';
import { verifyEd25519Signature } from '@/lib/crypto';
import { savePnlRecord, getPnlRecord } from '@/lib/db';

export async function POST(request: NextRequest) {
  try {
    const body = await request.json();
    const { data, signature } = body;

    if (!data || !signature) {
      return NextResponse.json({ error: 'Missing data or signature' }, { status: 400 });
    }

    const publicKey = process.env.PUBLIC_KEY;
    if (!publicKey) {
      return NextResponse.json({ error: 'Server misconfiguration' }, { status: 500 });
    }

    const isValid = verifyEd25519Signature(publicKey, data, signature);
    if (!isValid) {
      return NextResponse.json({ error: 'Invalid signature' }, { status: 401 });
    }

    const parsed = JSON.parse(data);
    const { timestamp, address, telegram_username, token_id, price_open, price_close } = parsed;

    if (!timestamp || !address || !token_id || price_open == null || price_close == null) {
      return NextResponse.json({ error: 'Missing required fields' }, { status: 400 });
    }

    const id = nanoid();
    savePnlRecord({
      id,
      timestamp,
      address,
      telegram_username: telegram_username ?? null,
      token_id,
      price_open,
      price_close,
    });

    return NextResponse.json({ id });
  } catch (error) {
    console.error('PnL submission error:', error);
    return NextResponse.json({ error: 'Invalid request' }, { status: 400 });
  }
}

export async function GET(request: NextRequest) {
  const id = request.nextUrl.searchParams.get('id');

  if (!id) {
    return NextResponse.json({ error: 'Missing id' }, { status: 400 });
  }

  const record = getPnlRecord(id);
  if (!record) {
    return NextResponse.json({ error: 'Not found' }, { status: 404 });
  }

  return NextResponse.json(record);
}
