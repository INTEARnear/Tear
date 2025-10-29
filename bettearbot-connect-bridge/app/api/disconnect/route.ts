import { NextRequest, NextResponse } from 'next/server';
import { verifyEd25519Signature } from '@/lib/crypto';
import { deleteXConnection, deleteNearConnection } from '@/lib/db';

export async function POST(request: NextRequest) {
  try {
    const body = await request.json();
    const { token, type } = body;

    if (!token) {
      return NextResponse.json({ error: 'Missing token' }, { status: 400 });
    }

    if (!type || !['x', 'near'].includes(type)) {
      return NextResponse.json({ error: 'Invalid or missing type parameter' }, { status: 400 });
    }

    const decoded = Buffer.from(token, 'base64').toString('utf-8');
    const firstComma = decoded.indexOf(',');
    if (firstComma === -1) {
      return NextResponse.json({ error: 'Invalid token format' }, { status: 400 });
    }

    const signature = decoded.slice(0, firstComma);
    const rest = decoded.slice(firstComma + 1);
    const secondComma = rest.indexOf(',');
    if (secondComma === -1) {
      return NextResponse.json({ error: 'Invalid token format' }, { status: 400 });
    }

    const userId = rest.slice(0, secondComma);

    const publicKey = process.env.PUBLIC_KEY;
    if (!publicKey) {
      return NextResponse.json({ error: 'Server misconfiguration' }, { status: 500 });
    }

    // Verify signature
    const isValid = verifyEd25519Signature(publicKey, userId, signature);
    if (!isValid) {
      return NextResponse.json({ error: 'Invalid signature' }, { status: 401 });
    }

    if (type === 'x') {
      deleteXConnection(userId);
    } else if (type === 'near') {
      deleteNearConnection(userId);
    }

    return NextResponse.json({ success: true });
  } catch (error) {
    console.error('Disconnect error:', error);
    return NextResponse.json({ error: 'Failed to disconnect' }, { status: 500 });
  }
}
