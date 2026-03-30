# FE-156: Implement Dark Mode Support

## Overview

This document covers the design and implementation of dark mode support for the ChainVerse frontend. Dark mode reduces eye strain in low-light environments and is a widely expected feature in modern web apps.

## Goals

- Support light and dark themes across all pages and components
- Persist user preference across sessions
- Respect the OS-level preference by default
- Allow manual toggle via UI and keyboard shortcut

## Approach

Use Tailwind CSS dark mode with the `class` strategy, combined with `next-themes` for theme management.

### Why `next-themes`

- Handles SSR without flash of unstyled content (FOUC)
- Persists preference to localStorage automatically
- Respects `prefers-color-scheme` media query by default
- Works seamlessly with Tailwind's `dark:` variant

## Setup

### Tailwind Config

```js
// tailwind.config.js
module.exports = {
  darkMode: 'class',
  // ...
};
```

### Install next-themes

```bash
npm install next-themes
```

### Theme Provider

```tsx
// app/layout.tsx
import { ThemeProvider } from 'next-themes';

export default function RootLayout({ children }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body>
        <ThemeProvider attribute="class" defaultTheme="system" enableSystem>
          {children}
        </ThemeProvider>
      </body>
    </html>
  );
}
```

## Theme Toggle Component

```tsx
// components/ui/ThemeToggle.tsx
import { useTheme } from 'next-themes';

const ThemeToggle = () => {
  const { theme, setTheme } = useTheme();
  return (
    <button
      onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
      aria-label="Toggle dark mode"
    >
      {theme === 'dark' ? 'Light Mode' : 'Dark Mode'}
    </button>
  );
};
```

Place the toggle in the navbar and wire it to the `Ctrl/Cmd + Shift + L` keyboard shortcut (see FE-157).

## Color Tokens

Define semantic color tokens in Tailwind config to keep dark/light variants consistent:

```js
// tailwind.config.js
theme: {
  extend: {
    colors: {
      background: 'hsl(var(--background))',
      foreground: 'hsl(var(--foreground))',
      primary: 'hsl(var(--primary))',
      muted: 'hsl(var(--muted))',
      border: 'hsl(var(--border))',
    },
  },
}
```

Define CSS variables in globals.css:

```css
:root {
  --background: 0 0% 100%;
  --foreground: 222 47% 11%;
  --primary: 221 83% 53%;
  --muted: 210 40% 96%;
  --border: 214 32% 91%;
}

.dark {
  --background: 222 47% 11%;
  --foreground: 210 40% 98%;
  --primary: 217 91% 60%;
  --muted: 217 33% 17%;
  --border: 217 33% 25%;
}
```

## Component Guidelines

- Use `dark:` Tailwind variants for all color utilities
- Never hardcode colors — always use semantic tokens
- Test every component in both themes before merging

Example:
```tsx
<div className="bg-background text-foreground border border-border">
  <p className="text-muted-foreground">Some text</p>
</div>
```

## Acceptance Criteria

- [ ] Dark mode works across all pages and components
- [ ] Theme preference persists across sessions via localStorage
- [ ] OS preference respected on first visit
- [ ] Toggle available in navbar
- [ ] No flash of unstyled content on page load
- [ ] All components use semantic color tokens, no hardcoded colors
