'use client';

interface SkeletonProps {
  className?: string;
  width?: string | number;
  height?: string | number;
  rounded?: 'sm' | 'md' | 'lg' | 'full';
}

export function Skeleton({ className = '', width, height, rounded = 'md' }: SkeletonProps) {
  return (
    <div
      role="status"
      aria-label="Loading..."
      aria-busy="true"
      className={`skeleton-shimmer rounded-${rounded} ${className}`}
      style={{ width, height }}
    />
  );
}
