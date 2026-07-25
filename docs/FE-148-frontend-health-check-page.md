# FE-148: Add Frontend Health Check Page

## Overview

This document describes the design of a frontend health check page for ChainVerse. The health check page provides a quick status overview of all critical services and dependencies, useful for ops, monitoring, and debugging.

## Goals

- Provide a single page that shows the status of all critical services
- Usable by monitoring tools (uptime checkers, CI pipelines)
- Show both a human-readable UI and a machine-readable JSON endpoint
- Only expose safe, non-sensitive status information

## Route

- UI page: `/health`
- JSON endpoint: `/api/health`

## Services to Check

| Service | Check |
|---------|-------|
| Frontend app | Page renders successfully |
| Stellar Horizon API | HTTP GET to Horizon root endpoint |
| Backend API | HTTP GET to `/api/ping` |
| Smart contracts | Read a lightweight contract query |
| Wallet provider | Freighter extension detectable |
| Environment config | Required env vars present |

## JSON Response Format

```json
{
  "status": "ok",
  "timestamp": "2025-01-01T00:00:00.000Z",
  "version": "1.0.0",
  "checks": {
    "horizon": { "status": "ok", "latency": 120 },
    "api": { "status": "ok", "latency": 45 },
    "contracts": { "status": "ok", "latency": 200 },
    "env": { "status": "ok" }
  }
}
```

If any check fails:

```json
{
  "status": "degraded",
  "checks": {
    "horizon": { "status": "error", "message": "Connection timeout" },
    "api": { "status": "ok", "latency": 45 }
  }
}
```

## API Route Implementation

```ts
// app/api/health/route.ts
import { NextResponse } from 'next/server';

const checkHorizon = async () => {
  const start = Date.now();
  const res = await fetch(process.env.NEXT_PUBLIC_HORIZON_URL);
  return { status: res.ok ? 'ok' : 'error', latency: Date.now() - start };
};

const checkApi = async () => {
  const start = Date.now();
  const res = await fetch(`${process.env.NEXT_PUBLIC_API_URL}/ping`);
  return { status: res.ok ? 'ok' : 'error', latency: Date.now() - start };
};

const checkEnv = () => {
  const required = [
    'NEXT_PUBLIC_STELLAR_NETWORK',
    'NEXT_PUBLIC_HORIZON_URL',
    'NEXT_PUBLIC_CONTRACT_CERTIFICATES',
  ];
  const missing = required.filter((k) => !process.env[k]);
  return missing.length === 0
    ? { status: 'ok' }
    : { status: 'error', message: `Missing: ${missing.join(', ')}` };
};

export async function GET() {
  const [horizon, api] = await Promise.allSettled([checkHorizon(), checkApi()]);
  const env = checkEnv();

  const checks = {
    horizon: horizon.status === 'fulfilled' ? horizon.value : { status: 'error' },
    api: api.status === 'fulfilled' ? api.value : { status: 'error' },
    env,
  };

  const overallStatus = Object.values(checks).every((c) => c.status === 'ok')
    ? 'ok'
    : 'degraded';

  return NextResponse.json({
    status: overallStatus,
    timestamp: new Date().toISOString(),
    version: process.env.npm_package_version ?? 'unknown',
    checks,
  }, { status: overallStatus === 'ok' ? 200 : 503 });
}
```

## UI Page

The `/health` page displays a visual status dashboard:

- Green checkmark for passing checks
- Red X for failing checks
- Latency shown in ms for network checks
- Last checked timestamp
- "Refresh" button to re-run checks

```
components/
  health/
    HealthCheckPage.tsx
    ServiceStatusRow.tsx   # Single row: icon + service name + status + latency
```

## Security Considerations

- Never expose internal IPs, credentials, or stack traces in the response
- The `/api/health` endpoint should not require authentication (needed by uptime monitors)
- Limit the information in error messages to avoid leaking infrastructure details

## Monitoring Integration

Configure an uptime monitoring tool (e.g. UptimeRobot, Better Uptime) to poll `/api/health` every 60 seconds and alert on non-200 responses.

## Acceptance Criteria

- [ ] `/api/health` returns JSON with status of all critical services
- [ ] Returns HTTP 200 when all checks pass, 503 when any fail
- [ ] `/health` page shows a visual status dashboard
- [ ] Environment variable check included
- [ ] No sensitive data exposed in the response
- [ ] Page is accessible without authentication
