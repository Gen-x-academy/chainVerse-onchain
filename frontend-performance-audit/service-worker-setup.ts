/**
 * Service Worker & Caching Strategies for chainVerse Frontend
 * 
 * Implements:
 * - Workbox service worker with multiple caching strategies
 * - Offline support for critical app shell
 * - Smart cache invalidation
 * - Background sync for blockchain transactions
 */

import { precacheAndRoute, cleanupOutdatedCaches } from 'workbox-precaching';
import { registerRoute } from 'workbox-routing';
import { 
  CacheFirst, 
  NetworkFirst, 
  StaleWhileRevalidate 
} from 'workbox-strategies';
import { ExpirationPlugin } from 'workbox-expiration';
import { BackgroundSyncPlugin } from 'workbox-background-sync';
import { cacheNames } from 'workbox-core';

// ============================================================================
// Cache Configuration
// ============================================================================

const CACHE_VERSION = 'v1';
const STATIC_CACHE = `static-${CACHE_VERSION}`;
const DYNAMIC_CACHE = `dynamic-${CACHE_VERSION}`;
const IMAGE_CACHE = `images-${CACHE_VERSION}`;
const API_CACHE = `api-${CACHE_VERSION}`;

// Cache duration constants
const CACHE_DURATIONS = {
  IMAGES: 7 * 24 * 60 * 60, // 7 days
  API: 5 * 60, // 5 minutes
  FONTS: 30 * 24 * 60 * 60, // 30 days
};

// ============================================================================
// Precaching - App Shell
// ============================================================================

// Precache critical assets during service worker installation
precacheAndRoute(self.__WB_MANIFEST);

// Clean up old caches
cleanupOutdatedCaches();

// ============================================================================
// Image Caching Strategy (Cache First)
// ============================================================================

registerRoute(
  ({ request, url }) => {
    return request.destination === 'image' || 
           url.pathname.endsWith('.webp') ||
           url.pathname.endsWith('.avif');
  },
  new CacheFirst({
    cacheName: IMAGE_CACHE,
    plugins: [
      new ExpirationPlugin({
        maxEntries: 100,
        maxAgeSeconds: CACHE_DURATIONS.IMAGES,
        purgeOnQuotaError: true,
      }),
    ],
  }),
  'GET'
);

// ============================================================================
// Font Caching Strategy (Cache First)
// ============================================================================

registerRoute(
  ({ request, url }) => {
    return request.destination === 'font' ||
           url.pathname.endsWith('.woff2') ||
           url.pathname.endsWith('.woff');
  },
  new CacheFirst({
    cacheName: `fonts-${CACHE_VERSION}`,
    plugins: [
      new ExpirationPlugin({
        maxEntries: 20,
        maxAgeSeconds: CACHE_DURATIONS.FONTS,
      }),
    ],
  }),
  'GET'
);

// ============================================================================
// API Caching Strategy (Network First with Fallback)
// ============================================================================

const bgSyncPlugin = new BackgroundSyncPlugin('transactionQueue', {
  maxRetentionTime: 24 * 60, // Retry for 24 hours
});

registerRoute(
  ({ url }) => url.pathname.startsWith('/api/'),
  new NetworkFirst({
    cacheName: API_CACHE,
    networkTimeoutSeconds: 3,
    plugins: [
      new ExpirationPlugin({
        maxEntries: 50,
        maxAgeSeconds: CACHE_DURATIONS.API,
        purgeOnQuotaError: true,
      }),
      {
        // Custom plugin for background sync
        fetchDidFail: async ({ request }) => {
          if (request.method === 'POST' || request.method === 'PUT') {
            bgSyncPlugin.pushRequest({ request });
          }
        },
      },
    ],
  }),
  'GET'
);

// ============================================================================
// Blockchain RPC Calls (Stale While Revalidate)
// ============================================================================

registerRoute(
  ({ url }) => {
    // Soroban RPC endpoints
    return url.hostname.includes('soroban') || 
           url.pathname.includes('/rpc');
  },
  new StaleWhileRevalidate({
    cacheName: `blockchain-rpc-${CACHE_VERSION}`,
    plugins: [
      new ExpirationPlugin({
        maxEntries: 30,
        maxAgeSeconds: 60, // 1 minute for blockchain data
      }),
    ],
  }),
  'POST'
);

// ============================================================================
// Static Assets (Cache First)
// ============================================================================

registerRoute(
  ({ request }) => {
    return [
      'script',
      'style',
    ].includes(request.destination);
  },
  new CacheFirst({
    cacheName: STATIC_CACHE,
    plugins: [
      new ExpirationPlugin({
        maxEntries: 60,
        maxAgeSeconds: 30 * 24 * 60 * 60, // 30 days
      }),
    ],
  }),
  'GET'
);

// ============================================================================
// HTML Pages (Network First)
// ============================================================================

registerRoute(
  ({ request }) => request.mode === 'navigate',
  new NetworkFirst({
    cacheName: `pages-${CACHE_VERSION}`,
    networkTimeoutSeconds: 2,
    plugins: [
      new ExpirationPlugin({
        maxEntries: 20,
        maxAgeSeconds: 24 * 60 * 60, // 24 hours
      }),
    ],
  }),
  'GET'
);

// ============================================================================
// Smart Contract State Caching
// ============================================================================

interface ContractState {
  address: string;
  data: any;
  timestamp: number;
  blockHeight?: number;
}

class ContractStateCache {
  private cacheName = `contract-state-${CACHE_VERSION}`;
  private defaultTTL = 30000; // 30 seconds

  async get(key: string): Promise<ContractState | null> {
    const cache = await caches.open(this.cacheName);
    const response = await cache.match(key);
    
    if (!response) return null;
    
    const data: ContractState = await response.json();
    const isExpired = Date.now() - data.timestamp > this.defaultTTL;
    
    if (isExpired) {
      await cache.delete(key);
      return null;
    }
    
    return data;
  }

  async set(key: string, data: any, blockHeight?: number) {
    const cache = await caches.open(this.cacheName);
    const state: ContractState = {
      address: key,
      data,
      timestamp: Date.now(),
      blockHeight,
    };
    
    await cache.put(key, new Response(JSON.stringify(state), {
      headers: { 'Content-Type': 'application/json' },
    }));
  }

  async invalidate(address: string) {
    const cache = await caches.open(this.cacheName);
    await cache.delete(address);
  }

  async clearAll() {
    await caches.delete(this.cacheName);
  }
}

export const contractCache = new ContractStateCache();

// ============================================================================
// Service Worker Lifecycle Management
// ============================================================================

// Skip waiting and activate immediately
self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((cacheNames) => {
      return Promise.all(
        cacheNames
          .filter((name) => name.startsWith('chainverse-'))
          .filter((name) => !name.includes(CACHE_VERSION))
          .map((name) => caches.delete(name))
      );
    })
  );
  
  self.clients.claim();
});

// Handle messages from clients
self.addEventListener('message', (event) => {
  if (event.data && event.data.type === 'SKIP_WAITING') {
    self.skipWaiting();
  }
  
  if (event.data && event.data.type === 'CLEAR_CACHE') {
    event.data.cacheNames.forEach((cacheName: string) => {
      caches.delete(cacheName);
    });
  }
  
  if (event.data && event.data.type === 'REFRESH_CONTRACT_STATE') {
    contractCache.invalidate(event.data.address);
  }
});

// ============================================================================
// Offline Detection & Recovery
// ============================================================================

let isOnline = true;

self.addEventListener('offline', () => {
  isOnline = false;
  self.clients.matchAll().then((clients) => {
    clients.forEach((client) => {
      client.postMessage({ type: 'OFFLINE' });
    });
  });
});

self.addEventListener('online', () => {
  isOnline = true;
  self.clients.matchAll().then((clients) => {
    clients.forEach((client) => {
      client.postMessage({ type: 'ONLINE' });
    });
  });
});

// ============================================================================
// Performance Monitoring
// ============================================================================

self.addEventListener('fetch', (event) => {
  const start = Date.now();
  
  event.respondWith(
    fetch(event.request).then((response) => {
      const duration = Date.now() - start;
      
      // Log slow requests
      if (duration > 3000) {
        console.warn(`Slow request: ${event.request.url} (${duration}ms)`);
      }
      
      return response;
    }).catch((error) => {
      console.error(`Fetch failed: ${event.request.url}`, error);
      throw error;
    })
  );
});

// ============================================================================
// React Hook for Service Worker Registration
// ============================================================================

export function registerServiceWorker(): Promise<ServiceWorkerRegistration | null> {
  if ('serviceWorker' in navigator) {
    try {
      return navigator.serviceWorker.register('/sw.js', {
        scope: '/',
      }).then((registration) => {
        console.log('Service Worker registered:', registration.scope);
        
        // Check for updates
        registration.addEventListener('updatefound', () => {
          const newWorker = registration.installing;
          
          if (newWorker) {
            newWorker.addEventListener('statechange', () => {
              if (newWorker.state === 'installed' && navigator.serviceWorker.controller) {
                // New content available
                console.log('New content available, refresh to update.');
                
                // Dispatch custom event for UI update
                window.dispatchEvent(new CustomEvent('swUpdate'));
              }
            });
          }
        });
        
        return registration;
      });
    } catch (error) {
      console.error('Service Worker registration failed:', error);
      return Promise.resolve(null);
    }
  }
  
  return Promise.resolve(null);
}

// ============================================================================
// Usage Example in React App
// ============================================================================

/*
// In your main app file (e.g., index.tsx or App.tsx):

import { useEffect, useState } from 'react';
import { registerServiceWorker, contractCache } from './service-worker';

function App() {
  const [isOnline, setIsOnline] = useState(navigator.onLine);
  const [swUpdate, setSwUpdate] = useState(false);

  useEffect(() => {
    // Register service worker
    registerServiceWorker();

    // Listen for online/offline events
    const handleOnline = () => setIsOnline(true);
    const handleOffline = () => setIsOnline(false);
    
    window.addEventListener('online', handleOnline);
    window.addEventListener('offline', handleOffline);

    // Listen for SW updates
    const handleSwUpdate = () => setSwUpdate(true);
    window.addEventListener('swUpdate', handleSwUpdate);

    return () => {
      window.removeEventListener('online', handleOnline);
      window.removeEventListener('offline', handleOffline);
      window.removeEventListener('swUpdate', handleSwUpdate);
    };
  }, []);

  const refreshApp = () => {
    if ('serviceWorker' in navigator) {
      navigator.serviceWorker.getRegistration().then(reg => {
        reg?.waiting?.postMessage({ type: 'SKIP_WAITING' });
        window.location.reload();
      });
    }
  };

  return (
    <div className="app">
      {!isOnline && (
        <div className="offline-banner">
          You're offline. Some features may be limited.
        </div>
      )}
      
      {swUpdate && (
        <div className="update-banner">
          <span>New version available!</span>
          <button onClick={refreshApp}>Refresh</button>
        </div>
      )}
      
      {/\* Rest of your app \*/}
    </div>
  );
}
*/
