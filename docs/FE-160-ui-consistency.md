# FE-160: Improve Overall UI Consistency

## Overview

This document outlines the approach for improving UI consistency across the ChainVerse frontend. Inconsistent spacing, typography, colors, and component styles create a fragmented user experience and slow down development.

## Goals

- Establish a single source of truth for design tokens
- Standardise component variants and usage patterns
- Eliminate one-off styles scattered across the codebase
- Document the component library for the team

## Audit Findings (Common Issues)

- Multiple button styles with no shared base component
- Inconsistent spacing — mix of arbitrary Tailwind values and design tokens
- Typography scale not enforced — font sizes set ad hoc
- Color values hardcoded in some components instead of using tokens
- Card components have varying border radius, shadow, and padding
- Form inputs styled differently across pages

## Design Token Standardisation

All visual values should come from Tailwind config tokens, not arbitrary values.

### Spacing Scale

Use the default Tailwind spacing scale consistently. Avoid arbitrary values like `mt-[13px]` — use `mt-3` or `mt-4` instead.

### Typography Scale

```js
// tailwind.config.js
fontSize: {
  xs:   ['0.75rem',  { lineHeight: '1rem' }],
  sm:   ['0.875rem', { lineHeight: '1.25rem' }],
  base: ['1rem',     { lineHeight: '1.5rem' }],
  lg:   ['1.125rem', { lineHeight: '1.75rem' }],
  xl:   ['1.25rem',  { lineHeight: '1.75rem' }],
  '2xl':['1.5rem',   { lineHeight: '2rem' }],
  '3xl':['1.875rem', { lineHeight: '2.25rem' }],
}
```

### Border Radius

Standardise to three values:
- `rounded` (4px) — inputs, tags
- `rounded-lg` (8px) — cards, modals
- `rounded-full` — avatars, pills

### Shadow Scale

```js
boxShadow: {
  sm:  '0 1px 2px 0 rgb(0 0 0 / 0.05)',
  md:  '0 4px 6px -1px rgb(0 0 0 / 0.1)',
  lg:  '0 10px 15px -3px rgb(0 0 0 / 0.1)',
}
```

## Core Component Standardisation

### Button

Single `Button` component with variants:

```tsx
// components/ui/Button.tsx
type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
type ButtonSize = 'sm' | 'md' | 'lg';

const variantStyles = {
  primary:   'bg-primary text-white hover:bg-primary/90',
  secondary: 'bg-muted text-foreground hover:bg-muted/80',
  ghost:     'bg-transparent hover:bg-muted',
  danger:    'bg-red-600 text-white hover:bg-red-700',
};

const sizeStyles = {
  sm: 'px-3 py-1.5 text-sm',
  md: 'px-4 py-2 text-base',
  lg: 'px-6 py-3 text-lg',
};
```

### Card

```tsx
// components/ui/Card.tsx
const Card = ({ children, className }) => (
  <div className={`bg-background border border-border rounded-lg shadow-sm p-4 ${className}`}>
    {children}
  </div>
);
```

### Input

```tsx
// components/ui/Input.tsx
const Input = ({ className, ...props }) => (
  <input
    className={`w-full rounded border border-border bg-background px-3 py-2 text-sm
      focus:outline-none focus:ring-2 focus:ring-primary ${className}`}
    {...props}
  />
);
```

## Component Checklist

| Component | Status |
|-----------|--------|
| Button | Standardise variants |
| Card | Create shared base |
| Input | Standardise across forms |
| Badge / Tag | Create shared component |
| Modal | Standardise padding and radius |
| Toast / Alert | Unify styles |
| Avatar | Standardise sizes |
| Divider | Create shared component |

## Migration Strategy

1. Create/update base components in `components/ui/`
2. Replace one-off implementations page by page
3. Remove unused custom styles
4. Add ESLint rule to flag arbitrary Tailwind values where tokens exist

## Acceptance Criteria

- [ ] All design tokens defined in Tailwind config
- [ ] Core UI components (Button, Card, Input) have standardised variants
- [ ] No hardcoded color values outside of token definitions
- [ ] Arbitrary Tailwind spacing/sizing values replaced with scale values
- [ ] Component usage documented with examples
