import { Skeleton } from '../Skeleton';

export function CourseCardSkeleton() {
  return (
    <div className="p-4 border rounded-lg space-y-3" aria-busy="true" aria-label="Loading course">
      <Skeleton height={160} rounded="md" />
      <Skeleton height={20} width="70%" />
      <Skeleton height={16} width="50%" />
      <div className="flex gap-2">
        <Skeleton height={32} width={80} rounded="full" />
        <Skeleton height={32} width={80} rounded="full" />
      </div>
    </div>
  );
}
