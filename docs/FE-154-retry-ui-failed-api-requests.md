# FE-154: Implement Retry UI for Failed API Requests

## Overview

This document describes the design and implementation of retry UI for failed API requests in the ChainVerse frontend. When a request fails, users should see a clear error state with the option to retry rather than a broken or empty UI.

## Goals

- Show meaningful error messages when API or contract calls fail
- Provide a retry button that re-triggers the failed request
- Handle loading, error, and empty states consistently across the app
- Support automatic retry with exponential backoff for transient errors

## Error State UI Pattern

Every data-fetching component should handle three states:

| State | UI |
|-------|----|
| Loading | Skeleton component |
| Error | Error message + Retry button |
| Empty | Empty state illustration + message |
| Success | Actual content |

## Reusable Components

### QueryWrapper

A wrapper component that handles all states for React Query results:

```tsx
// components/ui/QueryWrapper.tsx
interface QueryWrapperProps {
  isLoading: boolean;
  isError: boolean;
  error?: Error | null;
  onRetry: () => void;
  skeleton?: React.ReactNode;
  children: React.ReactNode;
}

const QueryWrapper = ({ isLoading, isError, error, onRetry, skeleton, children }) => {
  if (isLoading) return <>{skeleton ?? <DefaultSkeleton />}</>;
  if (isError) return <ErrorState message={error?.message} onRetry={onRetry} />;
  return <>{children}</>;
};
```

### ErrorState Component

```tsx
// components/ui/ErrorState.tsx
interface ErrorStateProps {
  message?: string;
  onRetry?: () => void;
  title?: string;
}

const ErrorState = ({
  title = 'Something went wrong',
  message = 'An unexpected error occurred. Please try again.',
  onRetry,
}: ErrorStateProps) => (
  <div role="alert" className="flex flex-col items-center gap-4 py-12 text-center">
    <ErrorIcon className="h-12 w-12 text-red-500" />
    <h3 className="text-lg font-semibold">{title}</h3>
    <p className="text-sm text-muted-foreground max-w-sm">{message}</p>
    {onRetry && (
      <button onClick={onRetry} className="btn btn-primary">
        Try again
      </button>
    )}
  </div>
);
```

## React Query Integration

React Query's `refetch` function is passed directly as the retry handler:

```tsx
const { data, isLoading, isError, error, refetch } = useQuery({
  queryKey: ['courses'],
  queryFn: fetchCourses,
  retry: 2, // auto-retry twice before showing error UI
});

return (
  <QueryWrapper isLoading={isLoading} isError={isError} error={error} onRetry={refetch}>
    <CourseList courses={data} />
  </QueryWrapper>
);
```

## Automatic Retry with Backoff

Configure React Query globally for sensible retry behaviour:

```ts
// lib/queryClient.ts
import { QueryClient } from '@tanstack/react-query';

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 2,
      retryDelay: (attempt) => Math.min(1000 * 2 ** attempt, 10000), // exponential backoff, max 10s
      staleTime: 1000 * 60, // 1 minute
    },
    mutations: {
      retry: 0, // don't auto-retry mutations
    },
  },
});
```

## Error Classification

Not all errors should show a retry button:

| Error Type | Retry Button | Message |
|------------|-------------|---------|
| Network error | Yes | "Check your connection and try again" |
| 500 Server error | Yes | "Server error. Please try again" |
| 404 Not found | No | "This content could not be found" |
| 401 Unauthorized | No | "Please connect your wallet" |
| 403 Forbidden | No | "You don't have access to this" |

```ts
// utils/errorHelpers.ts
export const isRetryable = (error: Error) => {
  if (error instanceof ApiError) {
    return ![401, 403, 404].includes(error.status);
  }
  return true; // network errors are retryable
};
```

## Toast Notifications for Mutations

For mutations (write operations), show a toast on failure with a retry option:

```ts
useMutation({
  mutationFn: enrollInCourse,
  onError: (error) => {
    toast.error('Enrollment failed', {
      description: error.message,
      action: { label: 'Retry', onClick: () => mutate(variables) },
    });
  },
});
```

## Acceptance Criteria

- [ ] All data-fetching components handle loading, error, and empty states
- [ ] ErrorState component displays a clear message and retry button
- [ ] Retry button re-triggers the failed request
- [ ] React Query configured with automatic retry and exponential backoff
- [ ] Non-retryable errors (401, 403, 404) do not show a retry button
- [ ] Mutation failures surface as toast notifications with retry action
- [ ] QueryWrapper component used consistently across the app
