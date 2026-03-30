# Frontend Performance Audit Framework

## 🎯 Performance Goals

### Core Web Vitals Targets
- **Largest Contentful Paint (LCP)**: < 2.5s
- **First Input Delay (FID)**: < 100ms
- **Cumulative Layout Shift (CLS)**: < 0.1
- **Time to Interactive (TTI)**: < 3.8s
- **Total Blocking Time (TBT)**: < 200ms
- **Speed Index (SI)**: < 3.4s

### Bundle Size Budgets
- **Initial JS Bundle**: < 200KB (gzipped)
- **Total JS Bundle**: < 500KB (gzipped)
- **CSS Bundle**: < 50KB (gzipped)
- **Individual Route**: < 100KB (gzipped)
- **Images per page**: < 300KB

---

## 📊 Performance Audit Checklist

### 1. Code Splitting & Lazy Loading
- [ ] Implement route-based code splitting
- [ ] Lazy load heavy components below the fold
- [ ] Use dynamic imports for large libraries
- [ ] Split vendor chunks from application code
- [ ] Implement progressive image loading

### 2. Bundle Optimization
- [ ] Enable tree-shaking in build configuration
- [ ] Remove unused dependencies
- [ ] Use modern bundle formats (ES modules)
- [ ] Minimize polyfills with browser detection
- [ ] Compress bundles with gzip/Brotli

### 3. Rendering Optimization
- [ ] Use React.memo for pure components
- [ ] Implement useMemo/useCallback for expensive calculations
- [ ] Avoid inline function definitions in JSX
- [ ] Debounce/throttle frequent events
- [ ] Virtualize long lists (react-window)
- [ ] Use CSS containment for isolated components

### 4. Image & Asset Optimization
- [ ] Use modern formats (WebP, AVIF)
- [ ] Implement responsive images (srcset)
- [ ] Lazy load off-screen images
- [ ] Optimize SVG assets
- [ ] Use CDN for static assets
- [ ] Preload critical assets

### 5. Network Optimization
- [ ] Enable HTTP/2 or HTTP/3
- [ ] Implement service worker caching
- [ ] Use stale-while-revalidate strategy
- [ ] Prefetch next-route resources
- [ ] Minimize critical request chain
- [ ] Defer non-critical requests

### 6. Caching Strategies
- [ ] Service Worker with Workbox
- [ ] Local Storage for user preferences
- [ ] IndexedDB for offline data
- [ ] Memory cache for API responses
- [ ] Stale-while-revalidate for static assets
- [ ] Cache-first for immutable assets

### 7. Smart Contract Integration Performance
- [ ] Batch RPC calls where possible
- [ ] Cache contract state locally
- [ ] Use web workers for cryptographic operations
- [ ] Implement optimistic UI updates
- [ ] Debounce wallet interactions
- [ ] Queue transactions for batch processing

---

## 🔧 Performance Monitoring Tools

### Build-Time Analysis
```bash
# Bundle analyzer
npm run build -- --stats
npx webpack-bundle-analyzer dist/stats.json

# Lighthouse CI
npx @lhci/cli@0.11.x autorun

# Source map explorer
npx source-map-explorer 'dist/*.js'
```

### Runtime Monitoring
```javascript
// Performance observer setup
const perfObserver = new PerformanceObserver((entries) => {
  entries.getEntries().forEach((entry) => {
    console.log('Performance entry:', entry);
  });
});

perfObserver.observe({ entryTypes: ['paint', 'largest-contentful-paint'] });
```

### Key Metrics to Track
1. **FCP (First Contentful Paint)**
2. **LCP (Largest Contentful Paint)**
3. **FID (First Input Delay)**
4. **CLS (Cumulative Layout Shift)**
5. **TBT (Total Blocking Time)**
6. **Memory usage**
7. **CPU throttling impact**

---

## 🚀 Performance Optimization Patterns

### Pattern 1: Component Memoization
```typescript
// ❌ Bad: Re-renders on every parent update
const ExpensiveComponent = ({ data }) => {
  return <div>{data.map(item => <Item key={item.id} {...item} />)}</div>;
};

// ✅ Good: Only re-renders when props change
const ExpensiveComponent = React.memo(({ data }) => {
  return <div>{data.map(item => <Item key={item.id} {...item} />)}</div>;
});
```

### Pattern 2: Code Splitting
```typescript
// ❌ Bad: Loads everything upfront
import HeavyComponent from './HeavyComponent';

// ✅ Good: Lazy loads on demand
const HeavyComponent = lazy(() => import('./HeavyComponent'));

function App() {
  return (
    <Suspense fallback={<Loading />}>
      <HeavyComponent />
    </Suspense>
  );
}
```

### Pattern 3: Virtual Scrolling
```typescript
// ❌ Bad: Renders 1000s of DOM nodes
const LongList = ({ items }) => {
  return <div>{items.map(item => <Item key={item.id} {...item} />)}</div>;
};

// ✅ Good: Only renders visible items
import { FixedSizeList } from 'react-window';

const LongList = ({ items }) => {
  return (
    <FixedSizeList height={600} itemCount={items.length} itemSize={50}>
      {({ index, style }) => <Item key={items[index].id} {...items[index]} style={style} />}
    </FixedSizeList>
  );
};
```

### Pattern 4: Debounced API Calls
```typescript
// ❌ Bad: API call on every keystroke
const SearchInput = () => {
  const [query, setQuery] = useState('');
  
  useEffect(() => {
    searchAPI(query); // Called too frequently!
  }, [query]);
  
  return <input onChange={(e) => setQuery(e.target.value)} />;
};

// ✅ Good: Debounced search
const SearchInput = () => {
  const [query, setQuery] = useState('');
  
  const debouncedSearch = useMemo(
    () => debounce((q) => searchAPI(q), 300),
    []
  );
  
  useEffect(() => {
    debouncedSearch(query);
  }, [query, debouncedSearch]);
  
  return <input onChange={(e) => setQuery(e.target.value)} />;
};
```

---

## 📈 Performance Testing Scripts

### Lighthouse Automated Testing
```json
{
  "ci": {
    "collect": {
      "numberOfRuns": 3,
      "settings": {
        "preset": "desktop"
      }
    },
    "assert": {
      "assertions": {
        "first-contentful-paint": ["error", {"maxNumericValue": 1500}],
        "largest-contentful-paint": ["error", {"maxNumericValue": 2500}],
        "cumulative-layout-shift": ["error", {"maxNumericValue": 0.1}]
      }
    }
  }
}
```

### Custom Performance Budget
```javascript
// performance-budget.js
module.exports = {
  initialPageSize: 200 * 1024, // 200KB
  totalPageSize: 500 * 1024,   // 500KB
  maxRequests: 50,
  maxCriticalRequests: 10,
  maxLoadTime: 3000, // 3s
  maxTTI: 3800, // 3.8s
};
```

---

## 🎯 Smart Contract Specific Optimizations

### 1. RPC Call Batching
```typescript
// ❌ Bad: Sequential RPC calls
const balance1 = await contract.getBalance(addr1);
const balance2 = await contract.getBalance(addr2);
const balance3 = await contract.getBalance(addr3);

// ✅ Good: Batched RPC calls
const [balance1, balance2, balance3] = await Promise.all([
  contract.getBalance(addr1),
  contract.getBalance(addr2),
  contract.getBalance(addr3),
]);
```

### 2. State Caching Layer
```typescript
class ContractStateCache {
  private cache = new Map<string, { data: any; timestamp: number }>();
  private TTL = 30000; // 30 seconds
  
  async get(key: string, fetchFn: () => Promise<any>) {
    const cached = this.cache.get(key);
    if (cached && Date.now() - cached.timestamp < this.TTL) {
      return cached.data;
    }
    
    const data = await fetchFn();
    this.cache.set(key, { data, timestamp: Date.now() });
    return data;
  }
}
```

### 3. Web Worker for Crypto Operations
```typescript
// worker.ts
self.addEventListener('message', async (e) => {
  const { type, payload } = e.data;
  
  if (type === 'VERIFY_SIGNATURE') {
    const result = await verifySignature(payload);
    self.postMessage({ type: 'SIGNATURE_VERIFIED', result });
  }
});

// Main thread
const worker = new Worker('./crypto-worker.ts');
worker.postMessage({ type: 'VERIFY_SIGNATURE', payload });
```

---

## 🔍 Performance Audit Schedule

### Continuous Monitoring
- **Every Commit**: Bundle size check
- **Every PR**: Lighthouse score validation
- **Weekly**: Full performance audit
- **Monthly**: Deep dive analysis + optimization sprint

### Pre-Deployment Checklist
- [ ] Lighthouse score > 90 on all metrics
- [ ] Bundle sizes within budget
- [ ] No memory leaks detected
- [ ] All images optimized
- [ ] Service worker caching configured
- [ ] Critical CSS inlined
- [ ] Non-critical JS deferred

---

## 📚 Resources & Tools

### Essential Tools
- **Lighthouse**: Performance auditing
- **WebPageTest**: Advanced performance testing
- **Chrome DevTools**: Runtime analysis
- **BundlePhobia**: Package size checking
- **Webpack Bundle Analyzer**: Visual bundle analysis

### Libraries
- **react-window**: Virtual scrolling
- **workbox**: Service worker management
- **compression-webpack-plugin**: Gzip/Brotli compression
- **purgecss**: Unused CSS removal
- **image-minimizer-webpack-plugin**: Image optimization

---

## 🎓 Team Guidelines

### Code Review Performance Checklist
1. Are there any unnecessary re-renders?
2. Is code splitting implemented for new routes?
3. Are images properly optimized?
4. Is memoization used appropriately?
5. Are API calls debounced/throttled?
6. Is lazy loading implemented where needed?

### Performance Regression Prevention
- Add performance tests to CI/CD
- Set up automated bundle size budgets
- Monitor Core Web Vitals in production
- Regular performance training for team
