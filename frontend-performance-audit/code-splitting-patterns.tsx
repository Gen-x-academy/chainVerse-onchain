/**
 * Code Splitting & Lazy Loading Implementation Guide
 * 
 * This file demonstrates best practices for:
 * - Route-based code splitting
 * - Component lazy loading
 * - Prefetching strategies
 * - Dynamic imports
 */

import React, { lazy, Suspense } from 'react';
import { useNavigationType, useLocation } from 'react-router-dom';

// ============================================================================
// Route-Based Code Splitting
// ============================================================================

/**
 * Lazy load route components with error handling and loading states
 */
const HomePage = lazy(() => import('@components/pages/HomePage'));
const DashboardPage = lazy(() => import('@components/pages/DashboardPage'));
const CourseRegistryPage = lazy(() => import('@components/pages/CourseRegistryPage'));
const EscrowPage = lazy(() => import('@components/pages/EscrowPage'));
const CertificatePage = lazy(() => import('@components/pages/CertificatePage'));
const RewardsPage = lazy(() => import('@components/pages/RewardsPage'));

interface RouteWithSubRoutesProps {
  routes: Array<{
    path: string;
    component: React.ComponentType<any>;
    exact?: boolean;
  }>;
}

/**
 * Custom route component with sub-routes support
 */
export const RouteWithSubRoutes: React.FC<RouteWithSubRoutesProps> = ({ routes }) => {
  return (
    <>
      {routes.map((route, index) => (
        <route.component key={index} {...route} />
      ))}
    </>
  );
};

// ============================================================================
// Smart Prefetching Strategy
// ============================================================================

/**
 * Prefetch chunks when user hovers over links
 * Improves perceived performance without impacting initial load
 */
export function usePrefetchOnHover() {
  const prefetchTimeoutId = React.useRef<number | null>(null);

  const prefetch = React.useCallback((chunkName: string) => {
    // Clear any existing timeout
    if (prefetchTimeoutId.current) {
      window.clearTimeout(prefetchTimeoutId.current);
    }

    // Set timeout to prefetch after short delay
    prefetchTimeoutId.current = window.setTimeout(() => {
      import(/* webpackChunkName: "[request]" */ `@components/${chunkName}`);
    }, 100); // 100ms hover threshold
  }, []);

  const clearPrefetch = React.useCallback(() => {
    if (prefetchTimeoutId.current) {
      window.clearTimeout(prefetchTimeoutId.current);
    }
  }, []);

  return { prefetch, clearPrefetch };
}

// ============================================================================
// Progressive Loading Components
// ============================================================================

interface ProgressiveImageProps {
  src: string;
  placeholder: string;
  alt: string;
  className?: string;
}

/**
 * Progressive image loading with blur-up effect
 */
export const ProgressiveImage: React.FC<ProgressiveImageProps> = ({
  src,
  placeholder,
  alt,
  className,
}) => {
  const [isLoaded, setIsLoaded] = React.useState(false);
  const [imageSrc, setImageSrc] = React.useState(placeholder);

  React.useEffect(() => {
    const img = new Image();
    img.src = src;
    img.onload = () => {
      setIsLoaded(true);
      setImageSrc(src);
    };
  }, [src]);

  return (
    <img
      src={imageSrc}
      alt={alt}
      className={`${className} ${isLoaded ? 'loaded' : 'loading'}`}
      loading="lazy"
    />
  );
};

// ============================================================================
// Component-Level Code Splitting
// ============================================================================

/**
 * Heavy component loaded only when needed
 */
const HeavyChartComponent = lazy(() => import('@components/charts/HeavyChart'));
const DataTableComponent = lazy(() => import('@components/tables/DataTable'));
const RichTextEditor = lazy(() => import('@components/editors/RichTextEditor'));

interface LazyComponentWrapperProps {
  fallback?: React.ReactNode;
  children: React.ReactNode;
}

/**
 * Wrapper component with error boundary for lazy-loaded components
 */
export class LazyComponentWrapper extends React.Component<
  LazyComponentWrapperProps,
  { hasError: boolean }
> {
  constructor(props: LazyComponentWrapperProps) {
    super(props);
    this.state = { hasError: false };
  }

  static getDerivedStateFromError() {
    return { hasError: true };
  }

  render() {
    if (this.state.hasError) {
      return <div>Failed to load component</div>;
    }

    return (
      <Suspense fallback={this.props.fallback || <div>Loading...</div>}>
        {this.props.children}
      </Suspense>
    );
  }
}

// ============================================================================
// Conditional Loading Based on User Actions
// ============================================================================

/**
 * Load heavy components only when user indicates intent
 */
export const IntentBasedLoader: React.FC<{
  trigger: React.ReactNode;
  componentToLoad: () => Promise<{ default: React.ComponentType<any> }>;
  placeholder?: React.ReactNode;
}> = ({ trigger, componentToLoad, placeholder }) => {
  const [Component, setComponent] = React.useState<React.ComponentType<any> | null>(null);
  const [isLoading, setIsLoading] = React.useState(false);

  const handleInteraction = React.useCallback(() => {
    if (!Component && !isLoading) {
      setIsLoading(true);
      componentToLoad().then((module) => {
        setComponent(() => module.default);
        setIsLoading(false);
      });
    }
  }, [Component, isLoading, componentToLoad]);

  return (
    <div onMouseEnter={handleInteraction} onFocus={handleInteraction}>
      {React.cloneElement(trigger as React.ReactElement, {
        onClick: handleInteraction,
      })}
      {(isLoading || Component) && (
        <LazyComponentWrapper fallback={placeholder}>
          {Component && <Component />}
        </LazyComponentWrapper>
      )}
    </div>
  );
};

// ============================================================================
// Resource Hints (Preload, Prefetch, Preconnect)
// ============================================================================

/**
 * Inject resource hints dynamically based on current route
 */
export function useResourceHints() {
  const location = useLocation();
  const navigationType = useNavigationType();

  React.useEffect(() => {
    // Preconnect to API server
    const preconnectLink = document.createElement('link');
    preconnectLink.rel = 'preconnect';
    preconnectLink.href = process.env.REACT_APP_API_URL || '';
    preconnectLink.crossOrigin = 'anonymous';
    document.head.appendChild(preconnectLink);

    // Prefetch next likely route based on current location
    const prefetchLinks = getPrefetchLinks(location.pathname);
    prefetchLinks.forEach((chunkName) => {
      const link = document.createElement('link');
      link.rel = 'prefetch';
      link.as = 'script';
      link.href = `/js/${chunkName}.chunk.js`;
      document.head.appendChild(link);
    });

    return () => {
      // Cleanup if needed
    };
  }, [location.pathname, navigationType]);
}

function getPrefetchLinks(pathname: string): string[] {
  // Customize based on your app's navigation patterns
  const prefetchMap: Record<string, string[]> = {
    '/': ['HomePage', 'FeaturesSection'],
    '/dashboard': ['DashboardCharts', 'RecentActivity'],
    '/courses': ['CourseList', 'CourseFilters'],
    '/escrow': ['EscrowList', 'CreateEscrowForm'],
  };

  return prefetchMap[pathname] || [];
}

// ============================================================================
// Example Usage in Routes
// ============================================================================

export const AppRoutes: React.FC = () => {
  useResourceHints();

  return (
    <Suspense fallback={<div>Loading application...</div>}>
      <Routes>
        <Route path="/" element={<HomePage />} />
        <Route path="/dashboard" element={<DashboardPage />} />
        <Route path="/courses" element={<CourseRegistryPage />} />
        <Route path="/escrow" element={<EscrowPage />} />
        <Route path="/certificates" element={<CertificatePage />} />
        <Route path="/rewards" element={<RewardsPage />} />
      </Routes>
    </Suspense>
  );
};

// ============================================================================
// Dynamic Import Helper with Error Handling
// ============================================================================

/**
 * Safe dynamic import with retry logic
 */
export async function safeImport<T>(
  importFn: () => Promise<{ default: T }>,
  retries = 3,
  delay = 1000
): Promise<T> {
  try {
    const module = await importFn();
    return module.default;
  } catch (error) {
    if (retries === 0) {
      throw error;
    }
    
    // Wait before retrying
    await new Promise(resolve => setTimeout(resolve, delay));
    
    // Retry with exponential backoff
    return safeImport(importFn, retries - 1, delay * 2);
  }
}

// Usage example:
// const Module = await safeImport(() => import('./HeavyModule'));
