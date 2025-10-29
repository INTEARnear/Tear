import { NextRequest, NextResponse } from 'next/server';
import { verifyEd25519Signature } from '@/lib/crypto';

export async function GET(request: NextRequest) {
  const searchParams = request.nextUrl.searchParams;
  const token = searchParams.get('token');

  if (!token) {
    return NextResponse.json({ valid: false, error: 'Missing token' }, { status: 400 });
  }

  try {
    const decoded = Buffer.from(token, 'base64').toString('utf-8');

    const parts = decoded.split(',');
    if (parts.length < 3) {
      return NextResponse.json({ valid: false, error: 'Invalid token format' }, { status: 400 });
    }

    const signature = parts[0];
    const userId = parts[1];
    const name = parts.slice(2).join(','); // Rejoin in case name contains commas

    if (!userId || !name) {
      return NextResponse.json({ valid: false, error: 'Invalid token format' }, { status: 400 });
    }

    // Verify signature
    const publicKey = process.env.PUBLIC_KEY;
    if (!publicKey) {
      return NextResponse.json({ valid: false, error: 'Server misconfiguration' }, { status: 500 });
    }

    const isValid = verifyEd25519Signature(publicKey, userId, signature);

    if (!isValid) {
      return NextResponse.json({ valid: false, error: 'Invalid signature' }, { status: 401 });
    }

    return NextResponse.json({ valid: true, userId, name });
  } catch (error) {
    console.error('Token verification error:', error);
    return NextResponse.json({ valid: false, error: 'Invalid token' }, { status: 400 });
  }
}
