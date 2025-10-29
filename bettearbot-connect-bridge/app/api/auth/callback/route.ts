import { NextRequest, NextResponse } from 'next/server';
import { verifyEd25519Signature } from '@/lib/crypto';
import { saveXConnection } from '@/lib/db';

interface XTokenResponse {
  access_token: string;
  token_type: string;
}

interface XUserResponse {
  data: {
    id: string;
    name: string;
    username: string;
  };
}

export async function GET(request: NextRequest) {
  const searchParams = request.nextUrl.searchParams;
  const code = searchParams.get('code');
  const state = searchParams.get('state');

  const codeVerifier = request.cookies.get('code_verifier')?.value;
  const savedState = request.cookies.get('oauth_state')?.value;
  const token = request.cookies.get('token')?.value;

  if (!code || !state || !codeVerifier || !savedState || !token) {
    return new NextResponse('Missing required parameters', { status: 400 });
  }

  if (state !== savedState) {
    return new NextResponse('Invalid state parameter', { status: 400 });
  }

  const clientId = process.env.X_CLIENT_ID;
  const clientSecret = process.env.X_CLIENT_SECRET;
  const publicKey = process.env.PUBLIC_KEY;

  if (!clientId || !clientSecret || !publicKey) {
    return new NextResponse('Server misconfiguration', { status: 500 });
  }

  const origin = request.nextUrl.origin;
  const callbackUrl = `${origin}/api/auth/callback`;

  try {
    const tokenResponse = await fetch('https://api.x.com/2/oauth2/token', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/x-www-form-urlencoded',
        Authorization: `Basic ${Buffer.from(`${clientId}:${clientSecret}`).toString('base64')}`,
      },
      body: new URLSearchParams({
        code,
        grant_type: 'authorization_code',
        redirect_uri: callbackUrl,
        code_verifier: codeVerifier,
      }),
    });

    if (!tokenResponse.ok) {
      const errorText = await tokenResponse.text();
      console.error('Token exchange failed:', errorText);
      return new NextResponse('Token exchange failed', { status: 500 });
    }

    const tokenData: XTokenResponse = await tokenResponse.json();

    const userResponse = await fetch('https://api.x.com/2/users/me', {
      headers: {
        Authorization: `Bearer ${tokenData.access_token}`,
      },
    });

    if (!userResponse.ok) {
      const errorText = await userResponse.text();
      console.error('User fetch failed:', errorText);
      return new NextResponse('Failed to fetch user data', { status: 500 });
    }

    const userData: XUserResponse = await userResponse.json();

    // Re-verify token signature before saving to database
    const decoded = Buffer.from(token, 'base64').toString('utf-8');
    const firstComma = decoded.indexOf(',');
    const signature = decoded.slice(0, firstComma);
    const rest = decoded.slice(firstComma + 1);
    const secondComma = rest.indexOf(',');
    const userId = rest.slice(0, secondComma);

    const isValid = verifyEd25519Signature(publicKey, userId, signature);
    if (!isValid) {
      return new NextResponse('Invalid token signature', { status: 401 });
    }

    saveXConnection(userId, userData.data.id);

    const homeUrl = new URL('/', request.url);
    homeUrl.searchParams.set('token', token);

    const response = NextResponse.redirect(homeUrl);

    response.cookies.delete('code_verifier');
    response.cookies.delete('oauth_state');
    response.cookies.delete('token');

    return response;
  } catch (error) {
    console.error('OAuth callback error:', error);
    return new NextResponse('Authentication failed', { status: 500 });
  }
}
