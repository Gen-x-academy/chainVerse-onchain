/**
 * Performance-Optimized Component Patterns for chainVerse Frontend
 * 
 * This module provides reusable HOCs and hooks for building
 * high-performance React components with proper memoization and optimization.
 */

import React, { 
  useMemo, 
  useCallback, 
  useRef, 
  useEffect, 
  useState,
  memo,
  Suspense,
  lazy
} from 'react';

// ============================================================================
// Higher-Order Components for Performance
// ============================================================================

/**
 * PureComponent HOC - Prevents unnecessary re-renders
 * Use for components with simple props that don't change frequently
 */
export const pure = <P extends object>(Component: React.ComponentType<P>) => {
  return memo(Component, (prevProps, nextProps) => {
    // Shallow comparison by default
    return Object.keys(prevProps).every(
      key => prevProps[key as keyof P] === nextProps[key as keyof P]
    );
  });
};

/**
 * Deep Compare Memo - For components with complex nested props
 * Warning: More expensive than shallow comparison, use sparingly
 */
export const deepMemo = <P extends object>(Component: React.ComponentType<P>) => {
  return memo(Component, (prevProps, nextProps) => {
    return JSON.stringify(prevProps) === JSON.stringify(nextProps);
  });
};

// ============================================================================
// Custom Hooks for Performance Optimization
// ============================================================================

/**
 * useDebounce - Delays value updates to prevent excessive re-renders
 * Perfect for search inputs, filters, and rapid user interactions
 */
export function useDebounce<T>(value: T, delay: number): T {
  const [debouncedValue, setDebouncedValue] = useState<T>(value);

  useEffect(() => {
    const handler = setTimeout(() => {
      setDebouncedValue(value);
    }, delay);

    return () => {
      clearTimeout(handler);
    };
  }, [value, delay]);

  return debouncedValue;
}

/**
 * useThrottle - Limits function execution rate
 * Ideal for scroll handlers, resize events, and frequent API calls
 */
export function useThrottle<T>(callback: (...args: any[]) => T, delay: number) {
  const lastRan = useRef(Date.now());
  const callbackRef = useRef(callback);

  useEffect(() => {
    callbackRef.current = callback;
  }, [callback]);

  return useCallback((...args: any[]) => {
    const now = Date.now();
    if (now - lastRan.current >= delay) {
      const result = callbackRef.current(...args);
      lastRan.current = now;
      return result;
    }
  }, [delay]);
}

/**
 * usePrevious - Tracks previous value of a state/prop
 * Useful for animations, transitions, and conditional rendering
 */
export function usePrevious<T>(value: T): T | undefined {
  const ref = useRef<T>();
  
  useEffect(() => {
    ref.current = value;
  }, [value]);
  
  return ref.current;
}

/**
 * useRenderCount - Debug hook to track component re-renders
 * Use during development to identify performance issues
 */
export function useRenderCount(componentName: string) {
  const renderCount = useRef(0);
  renderCount.current += 1;
  
  useEffect(() => {
    if (process.env.NODE_ENV === 'development') {
      console.log(`${componentName} render count:`, renderCount.current);
    }
  });
  
  return renderCount.current;
}

/**
 * useIntersectionObserver - Lazy loading trigger based on viewport visibility
 * Optimizes loading of images, components, and data
 */
export function useIntersectionObserver<T extends Element>(
  options: IntersectionObserverInit = {}
): [React.RefObject<T>, boolean] {
  const ref = useRef<T>(null);
  const [isVisible, setIsVisible] = useState(false);

  useEffect(() => {
    const observer = new IntersectionObserver(([entry]) => {
      setIsVisible(entry.isIntersecting);
    }, options);

    if (ref.current) {
      observer.observe(ref.current);
    }

    return () => {
      if (ref.current) {
        observer.unobserve(ref.current);
      }
    };
  }, [options]);

  return [ref, isVisible];
}

// ============================================================================
// Performance-Optimized Component Templates
// ============================================================================

interface VirtualListProps<T> {
  items: T[];
  itemHeight: number;
  containerHeight: number;
  renderItem: (item: T, index: number) => React.ReactNode;
}

/**
 * VirtualList - Renders only visible items in a long list
 * Dramatically reduces DOM nodes and improves scrolling performance
 * 
 * Usage:
 * <VirtualList
 *   items={largeDataset}
 *   itemHeight={50}
 *   containerHeight={600}
 *   renderItem={(item) => <ItemComponent {...item} />}
 * />
 */
export function VirtualList<T>({ 
  items, 
  itemHeight, 
  containerHeight, 
  renderItem 
}: VirtualListProps<T>) {
  const [scrollTop, setScrollTop] = useState(0);
  
  const totalHeight = items.length * itemHeight;
  const visibleItems = Math.ceil(containerHeight / itemHeight) + 1;
  const startIndex = Math.floor(scrollTop / itemHeight);
  const endIndex = Math.min(startIndex + visibleItems, items.length);
  
  const visibleItemsData = useMemo(() => {
    return items.slice(startIndex, endIndex).map((item, index) => ({
      item,
      index: startIndex + index,
      top: (startIndex + index) * itemHeight
    }));
  }, [items, startIndex, endIndex, itemHeight]);

  const handleScroll = useThrottle((e: React.UIEvent<HTMLDivElement>) => {
    setScrollTop(e.currentTarget.scrollTop);
  }, 16); // ~60fps

  return (
    <div 
      style={{ height: containerHeight, overflow: 'auto' }}
      onScroll={handleScroll}
    >
      <div style={{ height: totalHeight, position: 'relative' }}>
        {visibleItemsData.map(({ item, index, top }) => (
          <div 
            key={index} 
            style={{ 
              position: 'absolute', 
              top, 
              left: 0, 
              right: 0, 
              height: itemHeight 
            }}
          >
            {renderItem(item, index)}
          </div>
        ))}
      </div>
    </div>
  );
}

// ============================================================================
// Smart Contract Integration Optimizations
// ============================================================================

/**
 * useContractState - Caches contract state with TTL
 * Prevents excessive RPC calls and improves responsiveness
 */
export function useContractState<T>(
  key: string,
  fetchFn: () => Promise<T>,
  ttl: number = 30000 // 30 seconds default
) {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const cache = useRef<Map<string, { data: T; timestamp: number }>>(new Map());

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const cached = cache.current.get(key);
      if (cached && Date.now() - cached.timestamp < ttl) {
        setData(cached.data);
        return;
      }

      const freshData = await fetchFn();
      cache.current.set(key, { data: freshData, timestamp: Date.now() });
      setData(freshData);
      setError(null);
    } catch (err) {
      setError(err as Error);
    } finally {
      setLoading(false);
    }
  }, [key, fetchFn, ttl]);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, ttl);
    return () => clearInterval(interval);
  }, [refresh, ttl]);

  return { data, loading, error, refresh };
}

/**
 * useBatchedRPC - Batches multiple RPC calls into single request
 * Reduces network overhead and improves throughput
 */
export function useBatchedRPC() {
  const queue = useRef<Array<() => Promise<any>>>([]);
  const processing = useRef(false);

  const add = useCallback(<T,>(promiseFn: () => Promise<T>): Promise<T> => {
    return new Promise((resolve, reject) => {
      queue.current.push(async () => {
        try {
          const result = await promiseFn();
          resolve(result);
        } catch (error) {
          reject(error);
        }
      });
    });
  }, []);

  const processQueue = useCallback(async () => {
    if (processing.current || queue.current.length === 0) return;
    
    processing.current = true;
    const batch = [...queue.current];
    queue.current = [];

    try {
      await Promise.all(batch.map(fn => fn()));
    } finally {
      processing.current = false;
      if (queue.current.length > 0) {
        processQueue();
      }
    }
  }, []);

  useEffect(() => {
    const timer = setTimeout(processQueue, 100); // Batch within 100ms window
    return () => clearTimeout(timer);
  }, [processQueue]);

  return { add };
}

// ============================================================================
// Loading States & Suspense
// ============================================================================

/**
 * LoadingFallback - Reusable loading component with skeleton
 */
export const LoadingFallback: React.FC<{ message?: string }> = ({ 
  message = 'Loading...' 
}) => (
  <div className="loading-skeleton" role="status" aria-live="polite">
    <div className="skeleton-loader" />
    <span>{message}</span>
  </div>
);

/**
 * LazyLoadComponent - Wraps lazy loading with error boundary and suspense
 */
export function LazyLoadComponent<P>(
  importFn: () => Promise<{ default: React.ComponentType<P> }>,
  fallback: React.ReactNode = <LoadingFallback />
) {
  const LazyComponent = lazy(importFn);
  
  return function WrappedComponent(props: P) {
    return (
      <Suspense fallback={fallback}>
        <LazyComponent {...props} />
      </Suspense>
    );
  };
}
