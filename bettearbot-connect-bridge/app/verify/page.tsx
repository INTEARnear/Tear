'use client';

import { useSearchParams } from 'next/navigation';
import { useEffect, useState, Suspense } from 'react';

interface PnlRecord {
  id: string;
  timestamp: string;
  address: string;
  telegram_username: string | null;
  token_id: string;
  price_open: number;
  price_close: number;
}

function formatPrice(price: number): string {
  if (price === 0) return '$0';
  if (price < 0.0001) return `$${price.toExponential(4)}`;
  if (price < 1) return `$${price.toPrecision(4)}`;
  return `$${price.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 6 })}`;
}

function StatusIcon({ pnl }: { pnl: number }) {
  if (pnl > 300) {
    return (
      <div className="flex items-center gap-2">
        <svg viewBox="0 0 24 24" className="w-8 h-8" fill="none" stroke="#DAA520" strokeWidth="2.5">
          <path d="M20 6L9 17l-5-5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
        <span className="text-sm font-medium" style={{ color: '#DAA520' }}>Legendary</span>
      </div>
    );
  }
  if (pnl > 50) {
    return (
      <div className="flex items-center gap-2">
        <svg viewBox="0 0 24 24" className="w-8 h-8" fill="none" stroke="#22c55e" strokeWidth="2.5">
          <path d="M20 6L9 17l-5-5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
        <span className="text-sm font-medium text-green-500">Great Trade</span>
      </div>
    );
  }
  if (pnl < -90) {
    return (
      <div className="flex items-center gap-2">
        <span className="text-3xl">😭</span>
        <span className="text-sm font-medium text-red-400">Rekt</span>
      </div>
    );
  }
  return null;
}

function VerifyContent() {
  const searchParams = useSearchParams();
  const id = searchParams.get('id');
  const [record, setRecord] = useState<PnlRecord | null>(null);
  const [error, setError] = useState<string | null>(!id ? 'Missing verification ID' : null);
  const [loading, setLoading] = useState(!!id);

  useEffect(() => {
    if (!id) return;

    fetch(`/api/pnl?id=${encodeURIComponent(id)}`)
      .then((res) => {
        if (!res.ok) throw new Error('Not found');
        return res.json();
      })
      .then((data: PnlRecord) => {
        setRecord(data);
        setLoading(false);
      })
      .catch(() => {
        setError('PnL record not found');
        setLoading(false);
      });
  }, [id]);

  if (loading) {
    return (
      <main className="flex min-h-screen flex-col items-center justify-center p-4 md:p-24">
        <div className="text-xl md:text-2xl font-semibold">Loading...</div>
      </main>
    );
  }

  if (error || !record) {
    return (
      <main className="flex min-h-screen flex-col items-center justify-center p-4 md:p-24">
        <div className="bg-white dark:bg-zinc-900 rounded-lg shadow-lg p-6 md:p-12 max-w-md w-full mx-4 text-center">
          <div className="text-red-500 text-4xl md:text-5xl mb-4 md:mb-6">⚠️</div>
          <h1 className="text-xl md:text-2xl font-bold text-gray-900 dark:text-gray-100 mb-3 md:mb-4">
            Not Found
          </h1>
          <p className="text-sm md:text-base text-gray-600 dark:text-gray-400">
            {error || 'This PnL record does not exist.'}
          </p>
        </div>
      </main>
    );
  }

  const pnlPercent = record.price_open === 0 ? 0 : ((record.price_close / record.price_open) - 1) * 100;
  const isProfit = pnlPercent >= 0;
  const pnlFormatted = (isProfit ? '+' : '') + pnlPercent.toFixed(2) + '%';
  const date = new Date(record.timestamp);
  const dateFormatted = date.toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });

  return (
    <main className="flex min-h-screen flex-col items-center justify-center p-4 md:p-24">
      <div className="bg-white dark:bg-zinc-900 rounded-2xl shadow-xl p-6 md:p-10 max-w-lg w-full mx-4 border border-gray-200 dark:border-zinc-700">
        <div className="flex items-center justify-between mb-6">
          <h1 className="text-2xl md:text-3xl font-bold text-gray-900 dark:text-gray-100">
            Verified PnL
          </h1>
          <StatusIcon pnl={pnlPercent} />
        </div>

        <div
          className={`text-center py-6 px-4 rounded-xl mb-6 ${
            isProfit
              ? 'bg-green-50 dark:bg-green-950/30 border border-green-200 dark:border-green-800'
              : 'bg-red-50 dark:bg-red-950/30 border border-red-200 dark:border-red-800'
          }`}
        >
          <p className="text-sm text-gray-500 dark:text-gray-400 mb-1">
            {isProfit ? 'Profit' : 'Loss'}
          </p>
          <p
            className={`text-4xl md:text-5xl font-bold ${
              isProfit ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'
            }`}
          >
            {pnlFormatted}
          </p>
        </div>

        <div className="space-y-4">
          <div className="flex justify-between items-start py-3 border-b border-gray-100 dark:border-zinc-800">
            <span className="text-sm text-gray-500 dark:text-gray-400">Token</span>
            <span className="text-sm font-mono text-gray-900 dark:text-gray-100 text-right break-all max-w-[60%]">
              {record.token_id}
            </span>
          </div>

          <div className="flex justify-between items-center py-3 border-b border-gray-100 dark:border-zinc-800">
            <span className="text-sm text-gray-500 dark:text-gray-400">Entry Price</span>
            <span className="text-sm font-mono text-gray-900 dark:text-gray-100">
              {formatPrice(record.price_open)}
            </span>
          </div>

          <div className="flex justify-between items-center py-3 border-b border-gray-100 dark:border-zinc-800">
            <span className="text-sm text-gray-500 dark:text-gray-400">Exit Price</span>
            <span className="text-sm font-mono text-gray-900 dark:text-gray-100">
              {formatPrice(record.price_close)}
            </span>
          </div>

          <div className="flex justify-between items-start py-3 border-b border-gray-100 dark:border-zinc-800">
            <span className="text-sm text-gray-500 dark:text-gray-400">Address</span>
            <span className="text-sm font-mono text-gray-900 dark:text-gray-100 text-right break-all max-w-[60%]">
              {record.address}
            </span>
          </div>

          {record.telegram_username && (
            <div className="flex justify-between items-center py-3 border-b border-gray-100 dark:border-zinc-800">
              <span className="text-sm text-gray-500 dark:text-gray-400">Telegram</span>
              <span className="text-sm text-gray-900 dark:text-gray-100">
                @{record.telegram_username}
              </span>
            </div>
          )}

          <div className="flex justify-between items-center py-3">
            <span className="text-sm text-gray-500 dark:text-gray-400">Date</span>
            <span className="text-sm text-gray-900 dark:text-gray-100">{dateFormatted}</span>
          </div>
        </div>

        <div className="mt-6 pt-4 border-t border-gray-100 dark:border-zinc-800 text-center">
          <p className="text-xs text-gray-400 dark:text-gray-500">
            Cryptographically signed by BettearBot
          </p>
        </div>
      </div>
    </main>
  );
}

export default function VerifyPage() {
  return (
    <Suspense
      fallback={
        <main className="flex min-h-screen flex-col items-center justify-center p-4 md:p-24">
          <div className="text-xl md:text-2xl font-semibold">Loading...</div>
        </main>
      }
    >
      <VerifyContent />
    </Suspense>
  );
}
