'use client';

import { EmptyState } from '../../ui/EmptyState';

export function NoCoursesEmpty({ onBrowse }: { onBrowse?: () => void }) {
  return (
    <EmptyState
      icon={
        <svg width="64" height="64" viewBox="0 0 64 64" fill="none" aria-hidden="true">
          <rect x="8" y="12" width="48" height="40" rx="4" stroke="#D1D5DB" strokeWidth="2" />
          <path d="M20 24h24M20 32h16" stroke="#D1D5DB" strokeWidth="2" strokeLinecap="round" />
        </svg>
      }
      title="No courses yet"
      description="You haven't enrolled in any courses. Start learning on-chain today."
      action={
        onBrowse ? (
          <button
            onClick={onBrowse}
            className="px-5 py-2 rounded-md bg-blue-600 text-white text-sm hover:bg-blue-700"
          >
            Browse courses
          </button>
        ) : null
      }
    />
  );
}
