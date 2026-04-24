# FE-147: Bundle Size Monitoring

## Overview

This document outlines the approach for adding bundle size monitoring to the ChainVerse frontend. The goal is to track JavaScript bundle sizes over time, catch regressions early, and keep the app performant for users on slower connections.

## Goals

- Measure and report bundle sizes on every build
- Set size budgets and fail CI when budgets are exceeded
- Visualize bundle composition to identify large dependencies
- Track size trends over time

## Tooling

### Recommended Tools

| Tool | Purpose |
|------|---------|
| `@next/bundle-analyzer` (or `rollup-plugin-visualizer` for Vite) | Visual bundle composition map |
| `bundlesize` or `size-limit` | Enforce size budgets in CI |
| GitHub Actions | Automate checks on PRs |

### Installation

For Next.js:
```bash
npm install --save-dev @next/bundle-analyzer
```

For Vite:
```bash
npm install --save-dev rollup-plugin-visualizer
```

For size budgets:
```bash
npm install --save-dev size-limit @size-limit/file
```

## Configuration

### Bundle Analyzer (Next.js)

```js
// next.config.js
const withBundleAnalyzer = require('@next/bundle-analyzer')({
  enabled: process.env.ANALYZE === 'true',
});

module.exports = withBundleAnalyzer({
  // existing config
});
```

Run with:
```bash
ANALYZE=true npm run build
```

### Bundle Analyzer (Vite)

```js
// vite.config.ts
import { visualizer } from 'rollup-plugin-visualizer';

export default {
  plugins: [
    visualizer({ open: true, filename: 'bundle-stats.html' }),
  ],
};
```

### Size Budgets with size-limit

```json
// package.json
"size-limit": [
  {
    "path": ".next/static/chunks/main-*.js",
    "limit": "150 kB"
  },
  {
    "path": ".next/static/chunks/pages/index-*.js",
    "limit": "80 kB"
  }
]
```

Add script:
```json
"scripts": {
  "size": "size-limit"
}
```

## CI Integration

Add a GitHub Actions step to check bundle size on every PR:

```yaml
# .github/workflows/bundle-size.yml
name: Bundle Size Check

on:
  pull_request:
    branches: [main, develop]

jobs:
  bundle-size:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm
      - run: npm ci
      - run: npm run build
      - run: npm run size
```

## Size Budgets (Recommended Starting Points)

| Chunk | Budget |
|-------|--------|
| Main bundle | 150 kB (gzipped) |
| Per-page bundle | 80 kB (gzipped) |
| Vendor/third-party | 200 kB (gzipped) |
| Total initial load | 400 kB (gzipped) |

Adjust these based on baseline measurements from the first run.

## Optimization Strategies

When budgets are exceeded, common fixes include:

- **Dynamic imports** — lazy load heavy components and pages
- **Tree shaking** — ensure imports are named, not default where possible
- **Replace heavy deps** — e.g. swap `moment` for `date-fns`, `lodash` for native methods
- **Image optimization** — use Next.js `<Image>` or equivalent
- **Code splitting** — split routes and large features into separate chunks

## Acceptance Criteria

- [ ] Bundle analyzer is configured and can be run locally with a single command
- [ ] Size budgets are defined in `package.json`
- [ ] CI fails on PRs that exceed size budgets
- [ ] Bundle stats report is generated on each build
- [ ] README documents how to run the analyzer locally
