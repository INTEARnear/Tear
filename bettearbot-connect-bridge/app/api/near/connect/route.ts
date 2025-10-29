import { NextRequest, NextResponse } from 'next/server';
import { verifyEd25519Signature } from '@/lib/crypto';
import { verifySignature } from '@/lib/near';
import { saveNearConnection } from '@/lib/db';

interface ConnectRequestBody {
  token: string;
  accountId: string;
  publicKey: string;
  signature: string;
  message: string;
  nonce: string;
  recipient: string;
  callbackUrl?: string;
}

export async function POST(request: NextRequest) {
  try {
    const body: ConnectRequestBody = await request.json();
    const { token, accountId, publicKey, signature, message, nonce, recipient } = body;

    if (!token || !accountId || !publicKey || !signature || !message || !nonce || !recipient) {
      return NextResponse.json({ error: 'Missing required parameters' }, { status: 400 });
    }

    const botPublicKey = process.env.PUBLIC_KEY;
    if (!botPublicKey) {
      return NextResponse.json({ error: 'Server misconfiguration' }, { status: 500 });
    }

    // Verify bot signature first
    const decoded = Buffer.from(token, 'base64').toString('utf-8');
    const firstComma = decoded.indexOf(',');
    if (firstComma === -1) {
      return NextResponse.json({ error: 'Invalid token format' }, { status: 400 });
    }

    const tokenSignature = decoded.slice(0, firstComma);
    const rest = decoded.slice(firstComma + 1);
    const secondComma = rest.indexOf(',');
    if (secondComma === -1) {
      return NextResponse.json({ error: 'Invalid token format' }, { status: 400 });
    }

    const userId = rest.slice(0, secondComma);

    const isTokenValid = verifyEd25519Signature(botPublicKey, userId, tokenSignature);
    if (!isTokenValid) {
      return NextResponse.json({ error: 'Invalid token signature' }, { status: 401 });
    }

    if (message != `Connect NEAR account to BettearBot Telegram user: ${userId}`) {
      return NextResponse.json({ error: 'Message does not match user ID' }, { status: 401 });
    }

    // Verify NEP-413 signature
    const nonceBuffer = Buffer.from(nonce, 'base64');
    const isValidSignature = verifySignature({
      publicKey,
      signature,
      message,
      nonce: new Uint8Array(nonceBuffer),
      recipient,
    });

    if (!isValidSignature) {
      return NextResponse.json({ error: 'Invalid NEAR signature' }, { status: 401 });
    }

    saveNearConnection(userId, accountId);

    return NextResponse.json({ success: true, accountId });
  } catch (error) {
    console.error('NEAR connect error:', error);
    return NextResponse.json({ error: 'Failed to connect NEAR account' }, { status: 500 });
  }
}
