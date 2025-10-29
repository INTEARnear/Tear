import { NextRequest, NextResponse } from 'next/server';
import { verifyEd25519Signature } from '@/lib/crypto';
import { getConnection } from '@/lib/db';

export async function GET(request: NextRequest) {
  const searchParams = request.nextUrl.searchParams;
  const id = searchParams.get('id');
  const signature = searchParams.get('signature');

  if (!id || !signature) {
    return NextResponse.json({ error: 'Missing parameters' }, { status: 400 });
  }

  const publicKey = process.env.PUBLIC_KEY;
  if (!publicKey) {
    return NextResponse.json({ error: 'Server misconfiguration' }, { status: 500 });
  }

  const isValid = verifyEd25519Signature(publicKey, id, signature);
  if (!isValid) {
    return NextResponse.json({ error: 'Invalid signature' }, { status: 401 });
  }

  const connection = getConnection(id);
  if (!connection.x_user_id && !connection.near_account_id) {
    return NextResponse.json({ error: 'Not connected' }, { status: 404 });
  }

  return NextResponse.json({
    x: connection.x_user_id,
    near: connection.near_account_id,
  });
}
