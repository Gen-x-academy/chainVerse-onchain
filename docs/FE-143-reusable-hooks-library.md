# FE-143: Create Reusable Hooks Library

## Overview

This document describes the design of a reusable custom hooks library for the ChainVerse frontend. Centralising hooks avoids duplication, enforces consistent patterns, and makes logic easier to test.

## Goals

- Collect all reusable hooks in a single `hooks/` directory
- Cover common categories: data fetching, wallet, UI state, utilities
- Each hook should be typed, documented, and independently testable

## Hook Categories

### 1. Wallet Hooks

| Hook | Purpose |
|------|---------|
| `useWallet` | Access wallet address, balance, connection status |
| `useWalletConnect` | Trigger wallet connection flow |
| `useNetwork` | Get current network/chain info |

```ts
// hooks/useWallet.ts
import { useWalletStore } from '@/store/walletStore';

export const useWallet = () => {
  const { address, balance, isConnected } = useWalletStore();
  return { address, balance, isConnected };
};
```

### 2. Data Fetching Hooks

| Hook | Purpose |
|------|---------|
| `useCourses` | Fetch course list |
| `useCourse(id)` | Fetch single course |
| `useCertificates(address)` | Fetch certificates for a wallet |
| `useUserProfile(address)` | Fetch user profile |

```ts
// hooks/useCourses.ts
import { useQuery } from '@tanstack/react-query';
import { fetchCourses } from '@/services/api/courses';

export const useCourses = () =>
  useQuery({ queryKey: ['courses'], queryFn: fetchCourses });
```

### 3. UI / Utility Hooks

| Hook | Purpose |
|------|---------|
| `useDebounce(value, delay)` | Debounce a value |
| `useLocalStorage(key, initial)` | Persist state to localStorage |
| `useMediaQuery(query)` | Respond to CSS media queries |
| `useClickOutside(ref, handler)` | Detect clicks outside an element |
| `useCopyToClipboard` | Copy text with feedback state |
| `useMinLoadTime(isLoading, ms)` | Enforce minimum loading duration |
| `useKeyboardShortcut(combo, cb)` | Register keyboard shortcuts |

```ts
// hooks/useDebounce.ts
import { useState, useEffect } from 'react';

export const useDebounce = <T>(value: T, delay = 300): T => {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(timer);
  }, [value, delay]);
  return debounced;
};
```

```ts
// hooks/useLocalStorage.ts
import { useState } from 'react';

export const useLocalStorage = <T>(key: string, initial: T) => {
  const [value, setValue] = useState<T>(() => {
    try {
      const item = localStorage.getItem(key);
      return item ? JSON.parse(item) : initial;
    } catch {
      return initial;
    }
  });

  const set = (val: T) => {
    setValue(val);
    localStorage.setItem(key, JSON.stringify(val));
  };

  return [value, set] as const;
};
```

### 4. Contract Hooks

| Hook | Purpose |
|------|---------|
| `useMintCertificate` | Trigger certificate mint |
| `useEnrollCourse` | Enroll in a course via contract |
| `useTokenBalance(address)` | Get CHV token balance |

```ts
// hooks/useMintCertificate.ts
import { useMutation } from '@tanstack/react-query';
import { mintCertificate } from '@/services/contracts/certificates';

export const useMintCertificate = () =>
  useMutation({ mutationFn: mintCertificate });
```

## Directory Structure

```
hooks/
  wallet/
    useWallet.ts
    useWalletConnect.ts
    useNetwork.ts
  data/
    useCourses.ts
    useCourse.ts
    useCertificates.ts
    useUserProfile.ts
  contracts/
    useMintCertificate.ts
    useEnrollCourse.ts
    useTokenBalance.ts
  ui/
    useDebounce.ts
    useLocalStorage.ts
    useMediaQuery.ts
    useClickOutside.ts
    useCopyToClipboard.ts
    useMinLoadTime.ts
    useKeyboardShortcut.ts
  index.ts   # re-exports all hooks
```

## Acceptance Criteria

- [ ] All hooks live under `hooks/` with category subfolders
- [ ] `hooks/index.ts` re-exports all hooks for clean imports
- [ ] Each hook is fully typed with TypeScript
- [ ] Hooks are unit tested
- [ ] No business logic duplicated outside of hooks
