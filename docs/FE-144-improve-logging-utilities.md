# FE-144: Improve Logging Utilities

## Overview

This document describes improvements to the frontend logging utilities in ChainVerse. The goal is to replace ad-hoc `console.log` calls with a structured, configurable logging system that supports log levels, context tagging, and production-safe output.

## Goals

- Centralise all logging through a single utility
- Support log levels (debug, info, warn, error)
- Suppress debug/info logs in production
- Tag logs with context (component name, module, wallet address)
- Integrate with error monitoring (e.g. Sentry) for error-level logs

## Log Levels

| Level | When to Use |
|-------|-------------|
| `debug` | Verbose dev-only info (state changes, renders) |
| `info` | General app events (page views, wallet connected) |
| `warn` | Non-critical issues (deprecated usage, slow queries) |
| `error` | Failures that affect the user (contract errors, API failures) |

## Logger Implementation

```ts
// utils/logger.ts
type LogLevel = 'debug' | 'info' | 'warn' | 'error';

const LOG_LEVELS: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3,
};

const MIN_LEVEL: LogLevel =
  process.env.NODE_ENV === 'production' ? 'warn' : 'debug';

const shouldLog = (level: LogLevel) =>
  LOG_LEVELS[level] >= LOG_LEVELS[MIN_LEVEL];

const formatMessage = (level: LogLevel, context: string, message: string) =>
  `[${level.toUpperCase()}] [${context}] ${message}`;

export const createLogger = (context: string) => ({
  debug: (msg: string, data?: unknown) => {
    if (shouldLog('debug')) console.debug(formatMessage('debug', context, msg), data ?? '');
  },
  info: (msg: string, data?: unknown) => {
    if (shouldLog('info')) console.info(formatMessage('info', context, msg), data ?? '');
  },
  warn: (msg: string, data?: unknown) => {
    if (shouldLog('warn')) console.warn(formatMessage('warn', context, msg), data ?? '');
  },
  error: (msg: string, error?: unknown) => {
    if (shouldLog('error')) {
      console.error(formatMessage('error', context, msg), error ?? '');
      // forward to Sentry or other monitoring in production
      if (process.env.NODE_ENV === 'production') {
        // Sentry.captureException(error);
      }
    }
  },
});
```

## Usage

```ts
// In any component or service
import { createLogger } from '@/utils/logger';

const log = createLogger('CourseRegistry');

log.info('Fetching courses');
log.error('Contract call failed', err);
```

## Replacing Existing console.log Calls

Run this to find all raw console calls to migrate:

```bash
grep -r "console\." src/ --include="*.ts" --include="*.tsx"
```

Replace each with the appropriate `createLogger` call scoped to the file/module.

## Production Behaviour

- `debug` and `info` logs are suppressed in production builds
- `warn` and `error` logs remain active in production
- `error` logs are forwarded to the error monitoring service

## Acceptance Criteria

- [ ] `createLogger` utility exists in `utils/logger.ts`
- [ ] All log levels implemented and level filtering works
- [ ] Debug/info logs suppressed in production
- [ ] Existing `console.log` calls replaced with logger
- [ ] Error-level logs forwarded to monitoring service in production
- [ ] Logger is typed and documented
