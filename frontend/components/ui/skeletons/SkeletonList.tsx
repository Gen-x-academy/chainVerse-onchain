import { Fragment, type ReactNode } from 'react';

interface SkeletonListProps {
  count?: number;
  children: ReactNode;
}

export function SkeletonList({ count = 3, children }: SkeletonListProps) {
  return (
    <>
      {Array.from({ length: count }).map((_, i) => (
        <Fragment key={i}>{children}</Fragment>
      ))}
    </>
  );
}
