'use client';

import type { ReactNode } from 'react';

interface EmptyStateProps {
  /** SVG icon or illustration */
  icon?: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
  className?: string;
}

/**
 * Base empty state component. Compose feature-specific empty states
 * on top of this (e.g. NoCourses, NoTransactions).
 */
export function EmptyState({ icon, title, description, action, className = '' }: EmptyStateProps) {
  return (
    <div
      role="status"
      aria-label={title}
      className={`flex flex-col items-center justify-center py-16 px-6 text-center ${className}`}
    >
      {icon && <div className="mb-4 text-gray-300">{icon}</div>}
      <p className="text-base font-semibold text-gray-700">{title}</p>
      {description && <p className="mt-1 text-sm text-gray-500 max-w-xs">{description}</p>}
      {action && <div className="mt-6">{action}</div>}
    </div>
  );
}
