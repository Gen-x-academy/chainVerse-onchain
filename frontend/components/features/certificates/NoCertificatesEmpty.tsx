'use client';

import { EmptyState } from '../../ui/EmptyState';

export function NoCertificatesEmpty() {
  return (
    <EmptyState
      icon={
        <svg width="64" height="64" viewBox="0 0 64 64" fill="none" aria-hidden="true">
          <circle cx="32" cy="28" r="14" stroke="#D1D5DB" strokeWidth="2" />
          <path d="M24 44l8 8 8-8" stroke="#D1D5DB" strokeWidth="2" strokeLinecap="round" />
        </svg>
      }
      title="No certificates earned"
      description="Complete a course to earn your first on-chain certificate."
    />
  );
}
