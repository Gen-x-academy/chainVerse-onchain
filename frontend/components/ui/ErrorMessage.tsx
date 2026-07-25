'use client';

interface ErrorMessageProps {
  title?: string;
  message?: string;
  onRetry?: () => void;
}

/**
 * Inline error message component for async/query errors.
 * Use inside React Query error states or form validation.
 */
export function ErrorMessage({
  title = 'Something went wrong',
  message,
  onRetry,
}: ErrorMessageProps) {
  return (
    <div
      role="alert"
      aria-live="polite"
      className="rounded-lg border border-red-200 bg-red-50 p-4 text-sm text-red-700"
    >
      <p className="font-semibold">{title}</p>
      {message && <p className="mt-1 text-red-600">{message}</p>}
      {onRetry && (
        <button
          onClick={onRetry}
          className="mt-3 text-xs underline hover:no-underline"
        >
          Retry
        </button>
      )}
    </div>
  );
}
