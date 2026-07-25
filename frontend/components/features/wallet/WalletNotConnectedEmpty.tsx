'use client';

import { EmptyState } from '../../ui/EmptyState';

export function WalletNotConnectedEmpty({ onConnect }: { onConnect?: () => void }) {
  return (
    <EmptyState
      icon={
        <svg width="64" height="64" viewBox="0 0 64 64" fill="none" aria-hidden="true">
          <rect x="8" y="20" width="48" height="28" rx="4" stroke="#D1D5DB" strokeWidth="2" />
          <circle cx="44" cy="34" r="4" fill="#D1D5DB" />
        </svg>
      }
      title="Wallet not connected"
      description="Connect your Freighter wallet to access your dashboard and courses."
      action={
        onConnect ? (
          <button
            onClick={onConnect}
            className="px-5 py-2 rounded-md bg-blue-600 text-white text-sm hover:bg-blue-700"
          >
            Connect wallet
          </button>
        ) : null
      }
    />
  );
}
