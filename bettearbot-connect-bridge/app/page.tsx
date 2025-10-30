'use client';

import { NearWalletBase } from '@hot-labs/near-connect';
import { useSearchParams } from 'next/navigation';
import { useEffect, useState, Suspense } from 'react';

interface VerifyResponse {
  valid: boolean;
  userId?: string;
  name?: string;
  error?: string;
}

interface ConnectionResponse {
  x?: string;
  near?: string;
  error?: string;
}

function HomeContent() {
  const searchParams = useSearchParams();
  const token = searchParams.get('token');
  const [state, setState] = useState<
    | 'loading'
    | 'verified'
    | 'error'
    | 'disconnecting-x'
    | 'disconnecting-near'
    | 'connecting-near'
    | 'near-signed-in'
    | 'verifying-near'
  >(() => (token ? 'loading' : 'error'));
  const [data, setData] = useState<VerifyResponse | null>(null);
  const [xUserId, setXUserId] = useState<string | null>(null);
  const [nearAccountId, setNearAccountId] = useState<string | null>(null);
  const [nearWallet, setNearWallet] = useState<NearWalletBase | null>(null);

  const handleDisconnectX = async () => {
    if (!token) return;

    setState('disconnecting-x');
    try {
      const response = await fetch('/api/disconnect', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ token, type: 'x' }),
      });

      if (response.ok) {
        setXUserId(null);
        setState('verified');
      } else {
        setState('verified');
        alert('Failed to disconnect. Please try again.');
      }
    } catch (error) {
      console.error('Disconnect error:', error);
      setState('verified');
      alert('Failed to disconnect. Please try again.');
    }
  };

  const handleDisconnectNear = async () => {
    if (!token) return;

    setState('disconnecting-near');
    try {
      const response = await fetch('/api/disconnect', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ token, type: 'near' }),
      });

      if (response.ok) {
        setNearAccountId(null);
        setState('verified');
      } else {
        setState('verified');
        alert('Failed to disconnect. Please try again.');
      }
    } catch (error) {
      console.error('Disconnect error:', error);
      setState('verified');
      alert('Failed to disconnect. Please try again.');
    }
  };

  const handleConnectNear = async () => {
    if (!token || !data?.userId) return;

    setState('connecting-near');
    try {
      const { NearConnector } = await import('@hot-labs/near-connect');

      const connector = new NearConnector({
        network: 'mainnet',
        features: {
          signMessage: true,
        },
      });

      // Wait for wallet sign in
      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => {
          reject(new Error('Connection timeout'));
        }, 120000); // 2 minute timeout

        connector.on('wallet:signIn', async ({ wallet, accounts }) => {
          clearTimeout(timeout);

          if (!accounts || accounts.length === 0) {
            reject(new Error('No accounts found'));
            return;
          }

          const accountId = accounts[0].accountId;

          setNearWallet(wallet);
          setNearAccountId(accountId);
          setState('near-signed-in');
          resolve();
        });

        connector.connect();
      });
    } catch (error) {
      console.error('NEAR connect error:', error);
      setState('verified');
      alert('Failed to connect NEAR wallet. Please try again.');
    }
  };

  const handleVerifyNear = async () => {
    if (!token || !data?.userId || !nearWallet || !nearAccountId) return;

    setState('verifying-near');
    try {
      // Generate nonce. It's not security sensitive so no nonce validation needed.
      const nonce = crypto.getRandomValues(new Uint8Array(32));
      const nonceBuffer = Buffer.from(nonce);
      const nonceBase64 = nonceBuffer.toString('base64');

      const message = `Connect NEAR account to BettearBot Telegram user: ${data.userId}`;
      const recipient = 'connect.intea.rs';

      const signResult = await nearWallet.signMessage({
        message,
        nonce: nonceBuffer,
        recipient,
      });

      const response = await fetch('/api/near/connect', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          token,
          accountId: signResult.accountId,
          publicKey: signResult.publicKey,
          signature: signResult.signature,
          message,
          nonce: nonceBase64,
          recipient,
        }),
      });

      if (response.ok) {
        setState('verified');
        setNearWallet(null);
      } else {
        setState('near-signed-in');
        alert('Failed to verify NEAR account. Please try again.');
      }
    } catch (error) {
      console.error('NEAR verify error:', error);
      setState('near-signed-in');
      alert('Failed to verify NEAR account. Please try again.');
    }
  };

  useEffect(() => {
    if (!token) return;

    fetch(`/api/verify?token=${encodeURIComponent(token)}`)
      .then((res) => res.json())
      .then(async (verifyData: VerifyResponse) => {
        if (!verifyData.valid) {
          setState('error');
          setData(verifyData);
          return;
        }

        setData(verifyData);

        try {
          const decoded = Buffer.from(token, 'base64').toString('utf-8');
          const firstComma = decoded.indexOf(',');
          const signature = decoded.slice(0, firstComma);
          const userId = verifyData.userId;

          const connRes = await fetch(
            `/api/user?id=${encodeURIComponent(userId!)}&signature=${encodeURIComponent(signature)}`
          );
          const connData: ConnectionResponse = await connRes.json();

          if (connData.x) {
            setXUserId(connData.x);
          }
          if (connData.near) {
            setNearAccountId(connData.near);
          }
          setState('verified');
        } catch (err) {
          console.error('Connection check error:', err);
          setState('verified');
        }
      })
      .catch(() => {
        setState('error');
      });
  }, [token]);

  if (state === 'loading') {
    return (
      <main className="flex min-h-screen flex-col items-center justify-center p-4 md:p-24">
        <div className="text-xl md:text-2xl font-semibold">Loading...</div>
      </main>
    );
  }

  if (state === 'error') {
    return (
      <main className="flex min-h-screen flex-col items-center justify-center p-4 md:p-24 bg-linear-to-b from-gray-50 to-gray-100">
        <div className="bg-white rounded-lg shadow-lg p-6 md:p-12 max-w-md w-full mx-4 text-center">
          <div className="text-red-500 text-4xl md:text-5xl mb-4 md:mb-6">⚠️</div>
          <h1 className="text-xl md:text-2xl font-bold text-gray-900 mb-3 md:mb-4">Error 67</h1>
          <p className="text-sm md:text-base text-gray-600">Go back to the bot and use the button directly.</p>
        </div>
      </main>
    );
  }

  return (
    <main className="flex min-h-screen flex-col items-center justify-center p-4 md:p-24 bg-linear-to-b from-blue-50 to-blue-100">
      <div className="bg-white rounded-lg shadow-xl p-6 md:p-12 max-w-2xl w-full mx-4">
        <h1 className="text-3xl md:text-5xl font-bold text-gray-900 mb-3 md:mb-4 text-center">Hello, {data?.name}</h1>
        <p className="text-sm md:text-base text-gray-600 mb-6 md:mb-8 text-center">Connect your accounts to BettearBot</p>

        {/* X Connection */}
        <div className="mb-4 md:mb-6 p-4 md:p-6 border border-gray-200 rounded-lg">
          <div className="flex items-center justify-between mb-3 md:mb-4">
            <h2 className="text-xl md:text-2xl font-semibold text-gray-900">X</h2>
            {xUserId && <div className="text-green-500 text-xl md:text-2xl">✓</div>}
          </div>
          {xUserId ? (
            <>
              <div className="bg-gray-50 rounded-lg p-3 md:p-4 mb-3 md:mb-4">
                <p className="text-gray-600 text-xs md:text-sm mb-1">X User ID</p>
                <p className="text-sm md:text-lg font-mono text-gray-900 break-all">{xUserId}</p>
              </div>
              <button
                onClick={handleDisconnectX}
                disabled={state === 'disconnecting-x'}
                className="w-full bg-red-600 hover:bg-red-700 disabled:bg-gray-400 text-white font-semibold px-4 md:px-6 py-2.5 md:py-3 rounded-lg transition-colors cursor-pointer disabled:cursor-not-allowed text-sm md:text-base"
              >
                {state === 'disconnecting-x' ? 'Disconnecting...' : 'Disconnect X'}
              </button>
            </>
          ) : (
            <a
              href={`/api/auth?token=${encodeURIComponent(token!)}`}
              className="block w-full text-center bg-black hover:bg-gray-800 text-white font-semibold px-4 md:px-6 py-2.5 md:py-3 rounded-lg transition-colors text-sm md:text-base"
            >
              Connect X
            </a>
          )}
        </div>

        {/* NEAR Connection */}
        <div className="p-4 md:p-6 border border-gray-200 rounded-lg">
          <div className="flex items-center justify-between mb-3 md:mb-4">
            <h2 className="text-xl md:text-2xl font-semibold text-gray-900">NEAR Account</h2>
            {nearAccountId && state === 'verified' && (
              <div className="text-green-500 text-xl md:text-2xl">✓</div>
            )}
          </div>
          {state === 'near-signed-in' || state === 'verifying-near' ? (
            <>
              <div className="bg-blue-50 rounded-lg p-3 md:p-4 mb-3 md:mb-4">
                <p className="text-blue-600 text-xs md:text-sm mb-1">Wallet Connected</p>
                <p className="text-sm md:text-lg font-mono text-gray-900 break-all">{nearAccountId}</p>
                <p className="text-xs text-gray-500 mt-2">
                  Click Verify to complete the connection
                </p>
              </div>
              <button
                onClick={handleVerifyNear}
                disabled={state === 'verifying-near'}
                className="w-full bg-blue-600 hover:bg-blue-700 disabled:bg-gray-400 text-white font-semibold px-4 md:px-6 py-2.5 md:py-3 rounded-lg transition-colors cursor-pointer disabled:cursor-not-allowed text-sm md:text-base"
              >
                {state === 'verifying-near' ? 'Verifying...' : 'Verify Wallet'}
              </button>
            </>
          ) : nearAccountId ? (
            <>
              <div className="bg-gray-50 rounded-lg p-3 md:p-4 mb-3 md:mb-4">
                <p className="text-gray-600 text-xs md:text-sm mb-1">NEAR Account ID</p>
                <p className="text-sm md:text-lg font-mono text-gray-900 break-all">{nearAccountId}</p>
              </div>
              <button
                onClick={handleDisconnectNear}
                disabled={state === 'disconnecting-near'}
                className="w-full bg-red-600 hover:bg-red-700 disabled:bg-gray-400 text-white font-semibold px-4 md:px-6 py-2.5 md:py-3 rounded-lg transition-colors cursor-pointer disabled:cursor-not-allowed text-sm md:text-base"
              >
                {state === 'disconnecting-near' ? 'Disconnecting...' : 'Disconnect NEAR'}
              </button>
            </>
          ) : (
            <button
              onClick={handleConnectNear}
              disabled={state === 'connecting-near'}
              className="w-full bg-black hover:bg-gray-800 disabled:bg-gray-400 text-white font-semibold px-4 md:px-6 py-2.5 md:py-3 rounded-lg transition-colors cursor-pointer disabled:cursor-not-allowed text-sm md:text-base"
            >
              {state === 'connecting-near' ? 'Connecting...' : 'Connect NEAR'}
            </button>
          )}
        </div>
      </div>
    </main>
  );
}

export default function Home() {
  return (
    <Suspense
      fallback={
        <main className="flex min-h-screen flex-col items-center justify-center p-4 md:p-24">
          <div className="text-xl md:text-2xl font-semibold">Loading...</div>
        </main>
      }
    >
      <HomeContent />
    </Suspense>
  );
}
