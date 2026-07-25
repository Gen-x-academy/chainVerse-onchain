import { useCallback } from 'react';

type ErrorHandler = (error: unknown) => void;

/**
 * Returns a stable error handler that normalises unknown errors into
 * a human-readable message and forwards them to a callback (e.g. toast).
 */
export function useErrorHandler(onError: ErrorHandler) {
  return useCallback(
    (error: unknown) => {
      const message =
        error instanceof Error
          ? error.message
          : typeof error === 'string'
          ? error
          : 'An unexpected error occurred.';

      onError(new Error(message));
    },
    [onError],
  );
}
