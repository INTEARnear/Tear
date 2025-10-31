import { NextRequest, NextResponse } from 'next/server';
import { verifyEd25519Signature } from '@/lib/crypto';
import { getAllConnections } from '@/lib/db';

export async function GET(request: NextRequest) {
  const searchParams = request.nextUrl.searchParams;
  const signature = searchParams.get('signature');

  if (!signature) {
    return NextResponse.json({ error: 'Missing signature' }, { status: 400 });
  }

  const publicKey = process.env.PUBLIC_KEY;
  if (!publicKey) {
    return NextResponse.json({ error: 'Server misconfiguration' }, { status: 500 });
  }

  const isValid = verifyEd25519Signature(publicKey, 'get_all', signature);
  if (!isValid) {
    return NextResponse.json({ error: 'Invalid signature' }, { status: 401 });
  }

  const connections = getAllConnections();
  const result = connections.map((conn) => ({
    user_id: conn.telegram_user_id,
    x: conn.x_user_id,
    near: conn.near_account_id,
  }));

  return NextResponse.json(result);
}

