# FE-149: Developer Debugging Panel

## Overview

This document describes the design and implementation plan for a developer debugging panel in the ChainVerse frontend. The panel provides real-time visibility into app state, wallet connections, contract calls, and transaction history during development.

## Goals

- Give developers a quick way to inspect app state without opening browser devtools
- Surface wallet and blockchain-specific debug info (network, address, balances)
- Log contract interactions and their results in one place
- Only visible in development mode — zero impact on production builds

## Panel Features

### 1. Wallet Info
- Connected wallet address
- Network / chain ID
- Native token balance
- Connection status (connected, disconnected, connecting)

### 2. Contract Call Log
- Timestamp of each call
- Contract name and method invoked
- Arguments passed
- Response or error returned
- Transaction hash (if applicable)

### 3. App State Inspector
- Current route
- Auth state
- Feature flags active
- Environment variables (non-sensitive)

### 4. Performance Metrics
- Page load time
- Component render counts (dev mode only)
- API response times

### 5. Error Log
- Caught and uncaught errors
- Stack traces
- Timestamps

## Implementation Plan

### Component Structure

```
components/
  DevPanel/
    index.tsx          # Main panel wrapper (conditionally rendered)
    WalletInfo.tsx     # Wallet connection details
    ContractLog.tsx    # Contract call history
    StateInspector.tsx # App state snapshot
    ErrorLog.tsx       # Error history
    PerfMetrics.tsx    # Performance data
```

### Conditional Rendering

The panel should only mount in development:

```tsx
// components/DevPanel/index.tsx
const DevPanel = () => {
  if (process.env.NODE_ENV !== 'development') return null;
  // render panel
};
```

### Toggle Shortcut

Use a keyboard shortcut (e.g. `Ctrl + Shift + D`) to show/hide the panel so it doesn't obstruct the UI during normal dev work.

### State Management

Use a lightweight context or Zustand slice to store:
- Contract call history (capped at last 50 entries)
- Error log (capped at last 20 entries)
- Performance snapshots

### Contract Call Interception

Wrap contract call utilities to automatically push entries to the debug log:

```ts
// utils/debugLogger.ts
export const logContractCall = (method: string, args: unknown, result: unknown) => {
  if (process.env.NODE_ENV !== 'development') return;
  // push to debug store
};
```

## UI Design Notes

- Fixed position, bottom-right corner by default
- Collapsible to a small icon when not in use
- Tabs for each section (Wallet, Contracts, State, Errors, Perf)
- Dark theme to contrast with light app backgrounds
- Scrollable log areas with timestamps

## Security Considerations

- Never expose private keys or seed phrases
- Strip sensitive env vars from the state inspector
- Ensure `NODE_ENV` guard prevents any panel code from shipping to production bundles (tree-shaken out)

## Acceptance Criteria

- [ ] Panel renders only when `NODE_ENV === 'development'`
- [ ] Wallet address, network, and balance are displayed when connected
- [ ] Each contract call is logged with method name, args, and result
- [ ] Errors are captured and displayed with stack traces
- [ ] Panel can be toggled via keyboard shortcut
- [ ] No panel code or data leaks into production builds
