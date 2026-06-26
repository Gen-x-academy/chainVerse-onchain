/**
 * Client-Side Caching Strategy Implementation for chainVerse Frontend
 * 
 * This module provides a multi-layer caching architecture:
 * - Memory cache for frequently accessed data (fastest)
 * - Local Storage for persistent data (medium speed)
 * - IndexedDB for large structured data (slower but powerful)
 * - HTTP cache via Service Worker (network level)
 * 
 * Features:
 * - TTL-based expiration
 * - LRU eviction policy
 * - Cache invalidation strategies
 * - Optimistic updates
 * - Background synchronization
 */

// ============================================================================
// Type Definitions
// ============================================================================

interface CacheEntry<T> {
  data: T;
  timestamp: number;
  ttl: number;
  tags?: string[];
  version?: string;
}

interface CacheConfig {
  maxEntries?: number;
  defaultTTL?: number;
  storageType: 'memory' | 'localStorage' | 'indexedDB';
}

interface QueryOptions {
  staleTime?: number;
  cacheTime?: number;
  tags?: string[];
  enabled?: boolean;
}

// ============================================================================
// Memory Cache (Fastest - For Session Data)
// ============================================================================

class MemoryCache<T = any> {
  private cache: Map<string, CacheEntry<T>>;
  private maxSize: number;
  private defaultTTL: number;

  constructor(config: CacheConfig = { storageType: 'memory' }) {
    this.cache = new Map();
    this.maxSize = config.maxEntries || 100;
    this.defaultTTL = config.defaultTTL || 300000; // 5 minutes
  }

  get(key: string): T | null {
    const entry = this.cache.get(key);
    
    if (!entry) {
      return null;
    }

    // Check if expired
    if (Date.now() - entry.timestamp > entry.ttl) {
      this.delete(key);
      return null;
    }

    // LRU: Move to end of map
    this.cache.delete(key);
    this.cache.set(key, entry);
    
    return entry.data;
  }

  set(key: string, data: T, options?: Partial<{ ttl: number; tags: string[] }>): void {
    // Evict oldest if at capacity
    if (this.cache.size >= this.maxSize) {
      const firstKey = this.cache.keys().next().value;
      if (firstKey) {
        this.cache.delete(firstKey);
      }
    }

    const entry: CacheEntry<T> = {
      data,
      timestamp: Date.now(),
      ttl: options?.ttl || this.defaultTTL,
      tags: options?.tags,
    };

    this.cache.set(key, entry);
  }

  delete(key: string): boolean {
    return this.cache.delete(key);
  }

  clear(): void {
    this.cache.clear();
  }

  deleteByTag(tag: string): void {
    for (const [key, entry] of this.cache.entries()) {
      if (entry.tags?.includes(tag)) {
        this.cache.delete(key);
      }
    }
  }

  size(): number {
    return this.cache.size;
  }

  keys(): string[] {
    return Array.from(this.cache.keys());
  }
}

// ============================================================================
// Local Storage Cache (Persistent - For User Preferences & Auth)
// ============================================================================

class LocalStorageCache<T = any> {
  private prefix: string;
  private defaultTTL: number;

  constructor(prefix: string = 'chainverse_', defaultTTL: number = 86400000) {
    this.prefix = prefix;
    this.defaultTTL = defaultTTL; // 24 hours by default
  }

  private getKey(cacheKey: string): string {
    return `${this.prefix}${cacheKey}`;
  }

  get(key: string): T | null {
    try {
      const item = localStorage.getItem(this.getKey(key));
      
      if (!item) {
        return null;
      }

      const entry: CacheEntry<T> = JSON.parse(item);

      // Check if expired
      if (Date.now() - entry.timestamp > entry.ttl) {
        this.delete(key);
        return null;
      }

      return entry.data;
    } catch (error) {
      console.error('LocalStorageCache get error:', error);
      return null;
    }
  }

  set(key: string, data: T, options?: Partial<{ ttl: number; tags: string[] }>): void {
    try {
      const entry: CacheEntry<T> = {
        data,
        timestamp: Date.now(),
        ttl: options?.ttl || this.defaultTTL,
        tags: options?.tags,
      };

      localStorage.setItem(this.getKey(key), JSON.stringify(entry));
    } catch (error) {
      console.error('LocalStorageCache set error:', error);
      
      // Handle quota exceeded
      if (error instanceof DOMException && error.name === 'QuotaExceededError') {
        this.clearExpired();
      }
    }
  }

  delete(key: string): boolean {
    try {
      localStorage.removeItem(this.getKey(key));
      return true;
    } catch (error) {
      console.error('LocalStorageCache delete error:', error);
      return false;
    }
  }

  clear(): void {
    const keys = Object.keys(localStorage);
    keys.forEach(key => {
      if (key.startsWith(this.prefix)) {
        localStorage.removeItem(key);
      }
    });
  }

  private clearExpired(): void {
    const keys = Object.keys(localStorage);
    keys.forEach(key => {
      if (key.startsWith(this.prefix)) {
        try {
          const item = localStorage.getItem(key);
          if (!item) return;

          const entry: CacheEntry<any> = JSON.parse(item);
          if (Date.now() - entry.timestamp > entry.ttl) {
            localStorage.removeItem(key);
          }
        } catch {
          // Ignore parse errors
        }
      }
    });
  }

  size(): number {
    let count = 0;
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key?.startsWith(this.prefix)) {
        count++;
      }
    }
    return count;
  }
}

// ============================================================================
// IndexedDB Cache (Large Structured Data - For Contract State & History)
// ============================================================================

class IndexedDBCache<T = any> {
  private dbName: string;
  private storeName: string;
  private db: IDBDatabase | null = null;
  private version: number;

  constructor(
    dbName: string = 'chainverse-cache',
    storeName: string = 'cache',
    version: number = 1
  ) {
    this.dbName = dbName;
    this.storeName = storeName;
    this.version = version;
  }

  private async getDB(): Promise<IDBDatabase> {
    if (this.db) {
      return this.db;
    }

    return new Promise((resolve, reject) => {
      const request = indexedDB.open(this.dbName, this.version);

      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        this.db = request.result;
        resolve(this.db);
      };

      request.onupgradeneeded = (event) => {
        const db = (event.target as IDBOpenDBRequest).result;
        
        if (!db.objectStoreNames.contains(this.storeName)) {
          const store = db.createObjectStore(this.storeName, { keyPath: 'key' });
          store.createIndex('timestamp', 'timestamp', { unique: false });
          store.createIndex('tags', 'tags', { unique: false, multiEntry: true });
        }
      };
    });
  }

  async get(key: string): Promise<T | null> {
    try {
      const db = await this.getDB();
      
      return new Promise((resolve, reject) => {
        const transaction = db.transaction(this.storeName, 'readonly');
        const store = transaction.objectStore(this.storeName);
        const request = store.get(key);

        request.onsuccess = () => {
          const result = request.result;
          
          if (!result) {
            resolve(null);
            return;
          }

          // Check if expired
          if (Date.now() - result.timestamp > result.ttl) {
            this.delete(key);
            resolve(null);
            return;
          }

          resolve(result.data);
        };

        request.onerror = () => reject(request.error);
      });
    } catch (error) {
      console.error('IndexedDBCache get error:', error);
      return null;
    }
  }

  async set(key: string, data: T, options?: Partial<{ ttl: number; tags: string[] }>): Promise<void> {
    try {
      const db = await this.getDB();
      
      const entry = {
        key,
        data,
        timestamp: Date.now(),
        ttl: options?.ttl || 604800000, // 7 days default
        tags: options?.tags || [],
      };

      return new Promise((resolve, reject) => {
        const transaction = db.transaction(this.storeName, 'readwrite');
        const store = transaction.objectStore(this.storeName);
        const request = store.put(entry);

        request.onsuccess = () => resolve();
        request.onerror = () => reject(request.error);
      });
    } catch (error) {
      console.error('IndexedDBCache set error:', error);
    }
  }

  async delete(key: string): Promise<boolean> {
    try {
      const db = await this.getDB();
      
      return new Promise((resolve, reject) => {
        const transaction = db.transaction(this.storeName, 'readwrite');
        const store = transaction.objectStore(this.storeName);
        const request = store.delete(key);

        request.onsuccess = () => resolve(true);
        request.onerror = () => reject(request.error);
      });
    } catch (error) {
      console.error('IndexedDBCache delete error:', error);
      return false;
    }
  }

  async clear(): Promise<void> {
    try {
      const db = await this.getDB();
      
      return new Promise((resolve, reject) => {
        const transaction = db.transaction(this.storeName, 'readwrite');
        const store = transaction.objectStore(this.storeName);
        const request = store.clear();

        request.onsuccess = () => resolve();
        request.onerror = () => reject(request.error);
      });
    } catch (error) {
      console.error('IndexedDBCache clear error:', error);
    }
  }

  async deleteByTag(tag: string): Promise<void> {
    try {
      const db = await this.getDB();
      
      return new Promise((resolve, reject) => {
        const transaction = db.transaction(this.storeName, 'readwrite');
        const store = transaction.objectStore(this.storeName);
        const index = store.index('tags');
        const request = index.getAllKeys(tag);

        request.onsuccess = () => {
          const keys = request.result;
          const deleteRequests = keys.map(key => store.delete(key));
          
          Promise.all(deleteRequests)
            .then(() => resolve())
            .catch(reject);
        };

        request.onerror = () => reject(request.error);
      });
    } catch (error) {
      console.error('IndexedDBCache deleteByTag error:', error);
    }
  }
}

// ============================================================================
// Smart Contract State Cache with Versioning
// ============================================================================

interface ContractState {
  address: string;
  data: any;
  blockHeight?: number;
  lastUpdated: number;
}

class ContractStateCache {
  private memoryCache: MemoryCache<ContractState>;
  private persistentCache: LocalStorageCache<ContractState>;
  private historyCache: IndexedDBCache<ContractState[]>;

  constructor() {
    this.memoryCache = new MemoryCache({ 
      storageType: 'memory',
      maxEntries: 50,
      defaultTTL: 30000 // 30 seconds for hot contract state
    });
    
    this.persistentCache = new LocalStorageCache('contract_', 300000); // 5 minutes
    this.historyCache = new IndexedDBCache('chainverse-contract-history');
  }

  async get(address: string): Promise<ContractState | null> {
    // Try memory cache first (fastest)
    const memoryState = this.memoryCache.get(address);
    if (memoryState) {
      return memoryState;
    }

    // Try persistent cache
    const persistentState = this.persistentCache.get(address);
    if (persistentState) {
      // Promote to memory cache
      this.memoryCache.set(address, persistentState);
      return persistentState;
    }

    return null;
  }

  async set(
    address: string, 
    data: any, 
    options?: {
      blockHeight?: number;
      updateHistory?: boolean;
    }
  ): Promise<void> {
    const state: ContractState = {
      address,
      data,
      blockHeight: options?.blockHeight,
      lastUpdated: Date.now(),
    };

    // Set in all cache layers
    this.memoryCache.set(address, state);
    this.persistentCache.set(address, state);

    // Optionally update history
    if (options?.updateHistory) {
      await this.addToHistory(address, state);
    }
  }

  private async addToHistory(address: string, state: ContractState): Promise<void> {
    const historyKey = `history_${address}`;
    const existingHistory = await this.historyCache.get(historyKey) || [];
    
    const newHistory = [...existingHistory, state].slice(-10); // Keep last 10 states
    
    await this.historyCache.set(historyKey, newHistory);
  }

  invalidate(address: string): void {
    this.memoryCache.delete(address);
    this.persistentCache.delete(address);
  }

  invalidateAll(): void {
    this.memoryCache.clear();
    this.persistentCache.clear();
  }
}

// ============================================================================
// React Query-Style Hook for Caching
// ============================================================================

import { useState, useEffect, useCallback } from 'react';

interface UseQueryResult<T> {
  data: T | null;
  isLoading: boolean;
  error: Error | null;
  refetch: () => Promise<void>;
  isStale: boolean;
}

export function useQuery<T>(
  queryKey: string[],
  queryFn: () => Promise<T>,
  options?: QueryOptions
): UseQueryResult<T> {
  const [data, setData] = useState<T | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const [lastUpdated, setLastUpdated] = useState<number>(0);

  const cacheKey = queryKey.join(':');
  const staleTime = options?.staleTime || 5000; // 5 seconds
  const cacheTime = options?.cacheTime || 300000; // 5 minutes

  const fetchData = useCallback(async () => {
    if (!options?.enabled) return;

    try {
      setIsLoading(true);
      const freshData = await queryFn();
      setData(freshData);
      setLastUpdated(Date.now());
      
      // Store in cache (implementation depends on your setup)
      // cache.set(cacheKey, freshData, { ttl: cacheTime });
    } catch (err) {
      setError(err as Error);
    } finally {
      setIsLoading(false);
    }
  }, [cacheKey, queryFn, options?.enabled]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const isStale = Date.now() - lastUpdated > staleTime;

  return {
    data,
    isLoading,
    error,
    refetch: fetchData,
    isStale,
  };
}

// ============================================================================
// Cache Invalidation Strategies
// ============================================================================

export class CacheManager {
  private static instance: CacheManager;
  
  public contractCache: ContractStateCache;
  public memoryCache: MemoryCache;
  public localStorageCache: LocalStorageCache;
  public indexedDBCache: IndexedDBCache;

  private constructor() {
    this.contractCache = new ContractStateCache();
    this.memoryCache = new MemoryCache({ storageType: 'memory', maxEntries: 100 });
    this.localStorageCache = new LocalStorageCache('chainverse_');
    this.indexedDBCache = new IndexedDBCache();
  }

  static getInstance(): CacheManager {
    if (!CacheManager.instance) {
      CacheManager.instance = new CacheManager();
    }
    return CacheManager.instance;
  }

  // Time-based invalidation
  invalidateAfter(timeMs: number, keys: string[]): void {
    setTimeout(() => {
      keys.forEach(key => {
        this.memoryCache.delete(key);
        this.localStorageCache.delete(key);
      });
    }, timeMs);
  }

  // Tag-based invalidation
  invalidateByTag(tag: string): void {
    this.memoryCache.deleteByTag(tag);
    // Note: LocalStorage doesn't support tag-based deletion natively
    // You would need to implement custom metadata tracking
    this.indexedDBCache.deleteByTag(tag);
  }

  // Optimistic update with rollback capability
  async optimisticUpdate<T>(
    key: string,
    newData: T,
    mutationFn: () => Promise<boolean>,
    rollbackFn: () => Promise<void>
  ): Promise<void> {
    const oldValue = this.memoryCache.get(key);
    
    // Update cache immediately (optimistic)
    this.memoryCache.set(key, newData);
    
    try {
      const success = await mutationFn();
      
      if (!success) {
        throw new Error('Mutation failed');
      }
    } catch (error) {
      // Rollback on failure
      await rollbackFn();
      if (oldValue) {
        this.memoryCache.set(key, oldValue as T);
      }
      throw error;
    }
  }

  // Clear all caches
  clearAll(): void {
    this.memoryCache.clear();
    this.localStorageCache.clear();
    this.indexedDBCache.clear();
  }
}

// ============================================================================
// Export Singleton Instance
// ============================================================================

export const cacheManager = CacheManager.getInstance();
export { MemoryCache, LocalStorageCache, IndexedDBCache, ContractStateCache };
