# FE-146: Add Frontend Performance Metrics

## Overview

This document outlines the approach for adding frontend performance metrics to ChainVerse. Measuring real-user performance helps identify bottlenecks, track regressions, and ensure a fast experience for users on all devices and connections.

## Goals

- Capture Core Web Vitals and custom metrics
- Report metrics to an analytics or monitoring service
- Surface metrics in the developer debugging panel (dev mode)
- Set performance budgets and alert on regressions

## Metrics to Track

### Core Web Vitals

| Metric | Description | Good Threshold |
|--------|-------------|----------------|
| LCP (Largest Contentful Paint) | Load time of main content | < 2.5s |
| FID / INP (Interaction to Next Paint) | Responsiveness to input | < 200ms |
| CLS (Cumulative Layout Shift) | Visual stability | < 0.1 |

### Custom Metrics

| Metric | Description |
|--------|-------------|
| Time to Wallet Connected | Time from page load to wallet ready |
| Contract Call Duration | Time for each Soroban contract call |
| API Response Time | Time for each REST/GraphQL call |
| Page Transition Time | Time between route changes |
| Bundle Load Time | Time to load JS chunks |

## Implementation

### Core Web Vitals with `web-vitals`

```bash
npm install web-vitals
```

```ts
// lib/metrics.ts
import { onCLS, onINP, onLCP, onFCP, onTTFB } from 'web-vitals';

const reportMetric = ({ name, value, rating }: Metric) => {
  // Send to analytics (e.g. Vercel Analytics, Datadog, custom endpoint)
  console.info(`[Metric] ${name}: ${value} (${rating})`);

  // Example: send to a custom endpoint
  if (process.env.NODE_ENV === 'production') {
    fetch('/api/metrics', {
      method: 'POST',
      body: JSON.stringify({ name, value, rating }),
      headers: { 'Content-Type': 'application/json' },
    });
  }
};

export const initMetrics = () => {
  onCLS(reportMetric);
  onINP(reportMetric);
  onLCP(reportMetric);
  onFCP(reportMetric);
  onTTFB(reportMetric);
};
```

Call `initMetrics()` in the root layout or `_app.tsx`.

### Custom Performance Marks

Use the Performance API to measure custom timings:

```ts
// utils/perf.ts
export const perfMark = (name: string) => {
  if (typeof performance !== 'undefined') {
    performance.mark(name);
  }
};

export const perfMeasure = (name: string, start: string, end: string) => {
  if (typeof performance !== 'undefined') {
    performance.measure(name, start, end);
    const [entry] = performance.getEntriesByName(name);
    return entry?.duration ?? 0;
  }
  return 0;
};
```

Usage in contract calls:

```ts
perfMark('contract:mint:start');
await mintCertificate(args);
perfMark('contract:mint:end');
const duration = perfMeasure('contract:mint', 'contract:mint:start', 'contract:mint:end');
log.info(`Mint took ${duration}ms`);
```

### Next.js Built-in Analytics

If using Vercel, enable the built-in analytics:

```tsx
// app/layout.tsx
import { Analytics } from '@vercel/analytics/react';
import { SpeedInsights } from '@vercel/speed-insights/next';

export default function RootLayout({ children }) {
  return (
    <html>
      <body>
        {children}
        <Analytics />
        <SpeedInsights />
      </body>
    </html>
  );
}
```

### Dev Panel Integration

In development, surface metrics in the debug panel (see FE-149):
- Show latest Core Web Vital scores with colour-coded ratings (good/needs improvement/poor)
- Show last 10 contract call durations
- Show last 10 API response times

## Performance Budgets

Set budgets in CI to catch regressions (see FE-147 for bundle size budgets):

| Metric | Budget |
|--------|--------|
| LCP | < 2.5s |
| CLS | < 0.1 |
| INP | < 200ms |
| TTFB | < 800ms |

## Acceptance Criteria

- [ ] Core Web Vitals captured via `web-vitals` library
- [ ] Metrics reported to analytics/monitoring service in production
- [ ] Custom timing utility (`perfMark`/`perfMeasure`) available for contract and API calls
- [ ] Metrics visible in dev panel during development
- [ ] Performance budgets documented and monitored
