# FE-150: Frontend Architecture Documentation

## Overview

This document describes the frontend architecture of the ChainVerse Academy platform — a Web3 learning platform built on the Stellar blockchain. It covers project structure, key patterns, data flow, and conventions used across the codebase.

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Framework | Next.js (App Router) |
| Language | TypeScript |
| Styling | Tailwind CSS |
| State Management | Zustand |
| Data Fetching | React Query (TanStack Query) |
| Blockchain | Stellar / Soroban smart contracts |
| Wallet Integration | Freighter Wallet API |
| Testing | Jest + React Testing Library |
| CI/CD | GitHub Actions |

---

## Project Structure

```
chainVerse-onchainfrontend/
├── app/                    # Next.js App Router pages and layouts
│   ├── (auth)/             # Auth-gated routes
│   ├── (public)/           # Public routes
│   ├── layout.tsx          # Root layout
│   └── page.tsx            # Landing page
│
├── components/
│   ├── ui/                 # Base UI primitives (Button, Input, Skeleton, etc.)
│   ├── layout/             # Header, Footer, Sidebar, Nav
│   ├── features/           # Feature-specific components
│   │   ├── courses/
│   │   ├── certificates/
│   │   ├── wallet/
│   │   └── dashboard/
│   └── DevPanel/           # Developer debugging panel (dev only)
│
├── hooks/                  # Custom React hooks
├── lib/                    # Third-party client setup (query client, wallet, etc.)
├── services/               # API and contract call abstractions
│   ├── api/                # REST/GraphQL calls
│   └── contracts/          # Soroban contract wrappers
├── store/                  # Zustand stores
├── types/                  # Shared TypeScript types and interfaces
├── utils/                  # Pure utility functions
├── public/                 # Static assets
└── docs/                   # Architecture and feature documentation
```

---

## Key Architectural Patterns

### 1. Feature-Based Component Organization

Components are grouped by feature under `components/features/`, keeping related UI, hooks, and logic co-located. Base UI primitives live in `components/ui/` and are feature-agnostic.

### 2. Server vs Client Components

The app uses Next.js App Router conventions:
- Pages and layouts default to **Server Components** for better performance and SEO
- Components that need browser APIs, wallet access, or interactivity are marked `"use client"`
- Wallet-dependent UI is always client-side

### 3. Data Fetching

- **Server Components** fetch data directly (no loading states needed)
- **Client Components** use React Query for async data with caching, refetching, and loading/error states
- Contract reads go through `services/contracts/` wrappers
- REST API calls go through `services/api/` wrappers

```
Page (Server Component)
  └── fetches initial data server-side
      └── passes to Client Component
          └── React Query handles subsequent fetches / mutations
```

### 4. Wallet Integration

Wallet state is managed in a Zustand store (`store/walletStore.ts`). The Freighter wallet API is abstracted behind `services/wallet.ts` so the rest of the app doesn't depend on Freighter directly — making it easier to add other wallets later.

```
useWallet() hook
  └── reads from walletStore (Zustand)
      └── walletStore calls services/wallet.ts
          └── services/wallet.ts wraps Freighter API
```

### 5. Contract Interactions

Soroban contract calls are wrapped in `services/contracts/`. Each contract has its own file:

```
services/contracts/
  certificates.ts     # Certificate NFT contract
  courseRegistry.ts   # Course registry contract
  escrow.ts           # Escrow contract
  chvToken.ts         # CHV token contract
```

Each wrapper handles:
- Building the transaction
- Signing via the connected wallet
- Submitting to the network
- Parsing the response

### 6. State Management

Zustand is used for global client state. Stores are kept small and focused:

| Store | Responsibility |
|-------|---------------|
| `walletStore` | Wallet connection, address, balance |
| `authStore` | User auth state |
| `uiStore` | Global UI state (modals, toasts, theme) |
| `debugStore` | Dev panel logs (dev only) |

React Query handles server/async state — Zustand is only for client-side UI state.

---

## Authentication Flow

1. User clicks "Connect Wallet"
2. Freighter extension prompts for approval
3. On approval, wallet address is stored in `walletStore`
4. Address is used to look up user profile from backend
5. Auth state is set in `authStore`
6. Protected routes check `authStore` via middleware

---

## Error Handling

- Contract call errors are caught in `services/contracts/` wrappers and thrown as typed errors
- React Query's `onError` callbacks handle async errors in components
- A global error boundary catches unexpected render errors
- Toast notifications surface user-facing errors via `uiStore`

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `NEXT_PUBLIC_STELLAR_NETWORK` | `testnet` or `mainnet` |
| `NEXT_PUBLIC_CONTRACT_CERTIFICATES` | Certificates contract address |
| `NEXT_PUBLIC_CONTRACT_COURSE_REGISTRY` | Course registry contract address |
| `NEXT_PUBLIC_CONTRACT_ESCROW` | Escrow contract address |
| `NEXT_PUBLIC_CONTRACT_CHV_TOKEN` | CHV token contract address |
| `NEXT_PUBLIC_HORIZON_URL` | Stellar Horizon API URL |

Never commit `.env.local`. Use `.env.example` as a template.

---

## Testing Strategy

- **Unit tests** — utility functions and hooks (`utils/`, `hooks/`)
- **Component tests** — UI components with React Testing Library
- **Integration tests** — page-level flows with mocked contract calls
- **E2E tests** — critical user journeys (Playwright, optional)

Run tests:
```bash
npm run test          # unit + component
npm run test:e2e      # end-to-end
```

---

## CI/CD Pipeline

```
Push / PR
  └── Lint (ESLint + Prettier)
  └── Type check (tsc --noEmit)
  └── Unit + component tests
  └── Bundle size check
  └── Build
      └── Deploy preview (PRs)
      └── Deploy production (main branch)
```

---

## Adding a New Feature

1. Create a folder under `components/features/<feature-name>/`
2. Add contract wrapper in `services/contracts/<feature>.ts` if needed
3. Add API service in `services/api/<feature>.ts` if needed
4. Add Zustand store slice in `store/<feature>Store.ts` if global state is needed
5. Add page under `app/` following App Router conventions
6. Add types to `types/`
7. Write tests alongside components

---

## Conventions

- File names: `kebab-case` for files, `PascalCase` for components
- Exports: named exports preferred over default exports (except pages/layouts)
- Types: prefer `interface` for object shapes, `type` for unions/aliases
- Avoid `any` — use `unknown` and narrow types explicitly
- All async functions should handle errors explicitly
