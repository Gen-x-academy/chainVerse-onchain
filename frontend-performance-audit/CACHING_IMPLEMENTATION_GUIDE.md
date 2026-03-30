# Client-Side Caching Strategy Implementation Guide

## 📋 Overview

This caching strategy implements a **multi-layer architecture** optimized for the chainVerse dApp:

```
┌─────────────────────────────────────┐
│   Application Layer (React)         │
├─────────────────────────────────────┤
│   React Query Hook (useQuery)       │
├─────────────────────────────────────┤
│   Cache Manager (Coordinator)       │
├──────────┬──────────┬───────────────┤
│  Memory  │ Local    │  IndexedDB    │
│  Cache   │ Storage  │  Cache        │
│  (30s)   │ (5min)   │  (7 days)     │
└──────────┴──────────┴───────────────┘
```

---

## 🎯 Cache Layers Explained

### 1. Memory Cache (Fastest - Hot Data)
**TTL:** 30 seconds  
**Use Cases:**
- Currently viewed contract state
- Active escrow details
- User session data
- Real-time balances

**Example:**
```typescript
import { cacheManager } from './client-side-caching';

// Set with 30 second TTL
cacheManager.memoryCache.set('balance:user123', {
  amount: 1000n,
  token: 'CHV'
}, { ttl: 30000 });

// Get (returns null if expired)
const balance = cacheManager.memoryCache.get('balance:user123');
```

### 2. Local Storage Cache (Persistent - Warm Data)
**TTL:** 5 minutes  
**Use Cases:**
- User preferences
- Recently accessed contracts
- Cached API responses
- Form draft state

**Example:**
```typescript
// Persist user settings
cacheManager.localStorageCache.set('user:settings', {
  theme: 'dark',
  language: 'en',
  notifications: true
}, { ttl: 3600000 }); // 1 hour

// Retrieve
const settings = cacheManager.localStorageCache.get('user:settings');
```

### 3. IndexedDB Cache (Large Structured Data - Cold Data)
**TTL:** 7 days  
**Use Cases:**
- Transaction history
- Course catalog
- Certificate records
- Escrow transaction logs

**Example:**
```typescript
// Store large dataset
await cacheManager.indexedDBCache.set(
  'transactions:user123',
  [/* 1000+ transactions */],
  { 
    ttl: 604800000, // 7 days
    tags: ['transactions', 'user123']
  }
);

// Retrieve
const transactions = await cacheManager.indexedDBCache.get('transactions:user123');
```

---

## 🔧 Smart Contract State Caching

### ContractStateCache Implementation

```typescript
import { cacheManager } from './client-side-caching';

// Get contract state (checks all cache layers)
const escrowState = await cacheManager.contractCache.get('escrow:123');

if (!escrowState) {
  // Fetch from blockchain
  const freshState = await fetchEscrowState('escrow:123');
  
  // Cache with block height tracking
  await cacheManager.contractCache.set('escrow:123', freshState, {
    blockHeight: currentBlock,
    updateHistory: true // Keep historical states
  });
}

// Invalidate after mutation
await cacheManager.contractCache.invalidate('escrow:123');
```

### Automatic Cache Invalidation on Blockchain Events

```typescript
// Listen for blockchain events
sorobanRpc.addEventListener('ledgerUpdated', async (event) => {
  const affectedContracts = event.getAffectedContracts();
  
  // Invalidate cached state for affected contracts
  affectedContracts.forEach(contractId => {
    cacheManager.contractCache.invalidate(contractId);
  });
  
  // Optionally refetch fresh data
  affectedContracts.forEach(async (contractId) => {
    const freshState = await fetchContractState(contractId);
    await cacheManager.contractCache.set(contractId, freshState);
  });
});
```

---

## ⚛️ React Integration with useQuery Hook

### Basic Usage

```tsx
import { useQuery } from './client-side-caching';

function EscrowDetails({ escrowId }) {
  const { data, isLoading, error, refetch, isStale } = useQuery(
    ['escrow', escrowId], // Query key
    () => fetchEscrowData(escrowId), // Fetch function
    {
      staleTime: 5000, // Consider stale after 5s
      cacheTime: 30000, // Keep in cache for 30s
      enabled: true, // Conditional fetching
    }
  );

  if (isLoading) return <div>Loading...</div>;
  if (error) return <div>Error: {error.message}</div>;

  return (
    <div>
      <h3>Escrow #{escrowId}</h3>
      {isStale && <span>Data may be outdated</span>}
      <button onClick={refetch}>Refresh</button>
      {/* Render data */}
    </div>
  );
}
```

### Advanced: Optimistic Updates

```tsx
import { cacheManager } from './client-side-caching';

function EscrowActions({ escrowId }) {
  const handleRelease = async () => {
    const oldState = cacheManager.contractCache.get(`escrow:${escrowId}`);
    
    // Optimistic update
    await cacheManager.contractCache.set(`escrow:${escrowId}`, {
      ...oldState,
      status: 'released',
    });

    try {
      // Execute blockchain transaction
      await releaseEscrow(escrowId);
      
      // Success - cache will be updated by event listener
    } catch (error) {
      // Rollback on failure
      await cacheManager.contractCache.set(`escrow:${escrowId}`, oldState);
      throw error;
    }
  };

  return <button onClick={handleRelease}>Release Funds</button>;
}
```

---

## 🏷️ Cache Tagging for Bulk Invalidation

### Tag-Based Management

```typescript
// Cache with tags
cacheManager.memoryCache.set('course:101', courseData, {
  tags: ['courses', 'blockchain', 'beginner']
});

cacheManager.memoryCache.set('course:102', courseData, {
  tags: ['courses', 'defi', 'advanced']
});

// Invalidate all courses
cacheManager.invalidateByTag('courses');

// Invalidate only blockchain courses
cacheManager.invalidateByTag('blockchain');
```

---

## 🔄 Cache Warming Strategies

### Pre-fetching Likely Needed Data

```typescript
// When user navigates to dashboard
async function warmDashboardCache() {
  const prefetchPromises = [
    cacheManager.contractCache.set(
      'user:balance',
      await fetchUserBalance()
    ),
    cacheManager.indexedDBCache.set(
      'recent:escrows',
      await fetchRecentEscrows()
    ),
    cacheManager.localStorageCache.set(
      'user:preferences',
      await fetchUserPreferences()
    ),
  ];

  await Promise.all(prefetchPromises);
}
```

### Background Refresh

```typescript
// Refresh stale caches in background
setInterval(async () => {
  const keys = cacheManager.memoryCache.keys();
  
  keys.forEach(async (key) => {
    const data = cacheManager.memoryCache.get(key);
    
    if (data && isAboutToExpire(data)) {
      // Fetch fresh data without blocking UI
      const freshData = await fetchFreshData(key);
      cacheManager.memoryCache.set(key, freshData);
    }
  });
}, 10000); // Check every 10 seconds
```

---

## 🧹 Cache Cleanup & Maintenance

### Clear Expired Entries

```typescript
// Run on app startup
function initializeCacheCleanup() {
  // Clear expired localStorage entries
  cacheManager.localStorageCache.clearExpired();
  
  // Limit IndexedDB size
  cleanupIndexedDB();
}

async function cleanupIndexedDB() {
  const sevenDaysAgo = Date.now() - (7 * 24 * 60 * 60 * 1000);
  
  // Delete entries older than 7 days
  // Implementation depends on your IndexedDB schema
}
```

### Cache Size Limits

```typescript
// Monitor and enforce limits
function enforceCacheLimits() {
  const memorySize = cacheManager.memoryCache.size();
  
  if (memorySize > 100) {
    // Remove oldest 20%
    const keys = cacheManager.memoryCache.keys();
    const toDelete = keys.slice(0, Math.floor(memorySize * 0.2));
    
    toDelete.forEach(key => cacheManager.memoryCache.delete(key));
  }
}
```

---

## 📊 Performance Benchmarks

### Expected Latency by Cache Layer

| Cache Layer | Read Time | Write Time | Max Size |
|-------------|-----------|------------|----------|
| Memory Cache | < 1ms | < 1ms | 100 entries |
| LocalStorage | 1-5ms | 5-10ms | 5MB |
| IndexedDB | 10-50ms | 50-100ms | Unlimited |
| Network RPC | 100-1000ms | 200-2000ms | N/A |

### Cache Hit Rate Targets

- **Memory Cache:** > 80% for active contract state
- **LocalStorage:** > 60% for user preferences
- **IndexedDB:** > 90% for historical data

---

## ✅ Best Practices

### DO ✅
- Use appropriate TTL based on data volatility
- Invalidate cache after mutations
- Tag related cache entries for bulk operations
- Implement optimistic updates for better UX
- Monitor cache hit rates
- Clear cache on logout

### DON'T ❌
- Cache sensitive data without encryption
- Use infinite TTLs (data will become stale)
- Forget to handle cache misses
- Cache very large objects in memory
- Ignore storage quota limits
- Share cache between different users

---

## 🔒 Security Considerations

### Encrypt Sensitive Data

```typescript
import { encrypt, decrypt } from './crypto-utils';

// Before caching
const encryptedData = await encrypt(sensitiveData, userKey);
cacheManager.localStorageCache.set('user:private', encryptedData);

// After retrieving
const encryptedData = cacheManager.localStorageCache.get('user:private');
const decryptedData = await decrypt(encryptedData, userKey);
```

### Clear Cache on Logout

```typescript
function handleLogout() {
  // Clear all user-specific caches
  cacheManager.clearAll();
  
  // Also clear service worker caches
  if ('caches' in window) {
    caches.keys().then(names => {
      names.forEach(name => caches.delete(name));
    });
  }
}
```

---

## 🎯 Migration Path

### From No Caching → Multi-Layer Caching

**Phase 1: Add Memory Cache (Week 1)**
- Start with hot contract state
- Implement basic TTL
- Add cache invalidation on mutations

**Phase 2: Add LocalStorage (Week 2)**
- Cache user preferences
- Cache recent API responses
- Implement persistence

**Phase 3: Add IndexedDB (Week 3)**
- Migrate large datasets
- Add transaction history caching
- Implement tag-based invalidation

**Phase 4: Optimize (Week 4)**
- Add React Query hooks
- Implement optimistic updates
- Add cache warming strategies
- Monitor and tune TTLs

---

## 📈 Monitoring & Metrics

### Track Cache Performance

```typescript
class CacheMetrics {
  private hits = 0;
  private misses = 0;

  recordHit() {
    this.hits++;
    this.report();
  }

  recordMiss() {
    this.misses++;
    this.report();
  }

  getHitRate() {
    const total = this.hits + this.misses;
    return total > 0 ? (this.hits / total) * 100 : 0;
  }

  private report() {
    console.log('Cache Performance:', {
      hitRate: `${this.getHitRate().toFixed(2)}%`,
      hits: this.hits,
      misses: this.misses,
    });
  }
}

const metrics = new CacheMetrics();

// Wrap cache operations
const originalGet = cacheManager.memoryCache.get;
cacheManager.memoryCache.get = (key) => {
  const result = originalGet.call(cacheManager.memoryCache, key);
  result ? metrics.recordHit() : metrics.recordMiss();
  return result;
};
```

---

## 🛠️ Debugging Tools

### Inspect Cache Contents

```typescript
// Development helper
window.inspectCache = () => {
  console.group('Cache Inspector');
  
  console.log('Memory Cache:', {
    size: cacheManager.memoryCache.size(),
    keys: cacheManager.memoryCache.keys(),
  });
  
  console.log('LocalStorage Cache:', {
    size: cacheManager.localStorageCache.size(),
  });
  
  console.log('Contract Cache:', {
    // Custom inspection logic
  });
  
  console.groupEnd();
};
```

Call `window.inspectCache()` in browser console to view cache state.
