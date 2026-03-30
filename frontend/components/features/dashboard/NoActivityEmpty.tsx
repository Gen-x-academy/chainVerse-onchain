'use client';

import { EmptyState } from '../../ui/EmptyState';

export function NoActivityEmpty() {
  return (
    <EmptyState
      icon={
        <svg width="64" height="64" viewBox="0 0 64 64" fill="none" aria-hidden="true">
          <path
            d="M8 48l12-16 10 10 10-20 16 26"
            stroke="#D1D5DB"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      }
      title="No activity yet"
      description="Your transaction and learning activity will appear here."
    />
  );
}
