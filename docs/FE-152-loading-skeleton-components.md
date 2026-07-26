# FE-152: Improve Loading Skeleton Components

## Overview

This document covers improvements to the loading skeleton components used across the ChainVerse frontend. The goal is to make skeletons more consistent, accessible, and representative of the actual content they replace.

## Current Problems

- Skeletons don't match the shape/layout of the real content they represent
- No consistent base skeleton component — each feature rolls its own
- Missing accessibility attributes (`aria-busy`, `aria-label`)
- No animation or animation is jarring
- Skeletons sometimes flash briefly even on fast connections (no minimum display time)

## Improved Skeleton Design

### Base Skeleton Component

A single reusable base that all skeletons build on:

```tsx
// components/ui/Skeleton.tsx
interface SkeletonProps {
  className?: string;
  width?: string | number;
  height?: string | number;
  rounded?: 'sm' | 'md' | 'lg' | 'full';
}

const Skeleton = ({ className, width, height, rounded = 'md' }: SkeletonProps) => (
  <div
    role="status"
    aria-label="Loading..."
    aria-busy="true"
    className={`animate-pulse bg-gray-200 dark:bg-gray-700 rounded-${rounded} ${className}`}
    style={{ width, height }}
  />
);

export default Skeleton;
```

### Composite Skeletons

Build page/feature-specific skeletons from the base:

```tsx
// components/ui/skeletons/CourseCardSkeleton.tsx
const CourseCardSkeleton = () => (
  <div className="p-4 border rounded-lg space-y-3" aria-busy="true">
    <Skeleton height={160} rounded="md" />         {/* thumbnail */}
    <Skeleton height={20} width="70%" />            {/* title */}
    <Skeleton height={16} width="50%" />            {/* subtitle */}
    <div className="flex gap-2">
      <Skeleton height={32} width={80} rounded="full" />  {/* tag */}
      <Skeleton height={32} width={80} rounded="full" />
    </div>
  </div>
);
```

### Skeleton List Helper

For rendering N skeleton items in a list:

```tsx
// components/ui/skeletons/SkeletonList.tsx
const SkeletonList = ({ count = 3, children }: { count?: number; children: React.ReactNode }) => (
  <>
    {Array.from({ length: count }).map((_, i) => (
      <React.Fragment key={i}>{children}</React.Fragment>
    ))}
  </>
);
```

Usage:
```tsx
<SkeletonList count={6}>
  <CourseCardSkeleton />
</SkeletonList>
```

## Animation

Use a smooth shimmer animation instead of a flat pulse:

```css
/* globals.css or tailwind plugin */
@keyframes shimmer {
  0% { background-position: -200% 0; }
  100% { background-position: 200% 0; }
}

.skeleton-shimmer {
  background: linear-gradient(
    90deg,
    #e5e7eb 25%,
    #f3f4f6 50%,
    #e5e7eb 75%
  );
  background-size: 200% 100%;
  animation: shimmer 1.5s infinite;
}
```

## Minimum Display Time

Prevent skeleton flash on fast connections by enforcing a minimum display duration:

```ts
// hooks/useMinLoadTime.ts
const useMinLoadTime = (isLoading: boolean, minMs = 300) => {
  const [show, setShow] = useState(isLoading);

  useEffect(() => {
    if (!isLoading) {
      const timer = setTimeout(() => setShow(false), minMs);
      return () => clearTimeout(timer);
    }
    setShow(true);
  }, [isLoading, minMs]);

  return show;
};
```

## Skeletons to Create / Update

| Component | Status |
|-----------|--------|
| `CourseCardSkeleton` | Create |
| `ProfileSkeleton` | Create |
| `CertificateCardSkeleton` | Create |
| `TransactionHistorySkeleton` | Create |
| `DashboardStatsSkeleton` | Create |
| `NavbarSkeleton` | Update existing |

## Accessibility Requirements

- All skeleton containers must have `role="status"` and `aria-busy="true"`
- Wrap skeleton groups in a container with `aria-label="Loading content"`
- When content loads, ensure focus is managed correctly if the skeleton replaced a focusable element
- Respect `prefers-reduced-motion` — disable animation for users who prefer it:

```css
@media (prefers-reduced-motion: reduce) {
  .animate-pulse, .skeleton-shimmer {
    animation: none;
  }
}
```

## Acceptance Criteria

- [ ] A single base `Skeleton` component exists and is used by all skeleton variants
- [ ] Skeletons visually match the layout of the content they replace
- [ ] Shimmer animation is smooth and respects `prefers-reduced-motion`
- [ ] All skeletons have correct ARIA attributes
- [ ] Minimum display time hook prevents skeleton flash
- [ ] Skeleton components are documented with usage examples
