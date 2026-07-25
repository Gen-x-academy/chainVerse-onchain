# FE-157: Add Keyboard Shortcut Support

## Overview

Design and implementation plan for keyboard shortcut support in the ChainVerse frontend.

## Goals

- Provide shortcuts for common navigation and actions
- Show a discoverable shortcut reference modal
- Avoid conflicts with browser/OS defaults
- Support Mac and Windows/Linux bindings

## Shortcut Map

### Global

| Action | Mac | Windows/Linux |
|--------|-----|---------------|
| Open shortcut help | Cmd + / | Ctrl + / |
| Go to Dashboard | Cmd + D | Ctrl + D |
| Open search | Cmd + K | Ctrl + K |
| Toggle dark mode | Cmd + Shift + L | Ctrl + Shift + L |
| Toggle dev panel (dev only) | Cmd + Shift + D | Ctrl + Shift + D |

### Modal / Dialog

| Action | Key |
|--------|-----|
| Close modal | Escape |
| Confirm | Enter |

### Course Page

| Action | Key |
|--------|-----|
| Next lesson | Arrow Right or N |
| Previous lesson | Arrow Left or P |
| Toggle sidebar | S |

## Implementation Plan

### Core Hook

```ts
// hooks/useKeyboardShortcut.ts
const useKeyboardShortcut = (combo, callback) => {
  useEffect(() => {
    const handler = (e) => {
      const tag = e.target.tagName;
      if (['INPUT', 'TEXTAREA', 'SELECT'].includes(tag)) return;
      const match =
        e.key.toLowerCase() === combo.key.toLowerCase() &&
        !!combo.meta === e.metaKey &&
        !!combo.ctrl === e.ctrlKey &&
        !!combo.shift === e.shiftKey;
      if (match) { e.preventDefault(); callback(); }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [combo, callback]);
};
```

### Shortcut Registry

```ts
// lib/shortcuts.ts
export const SHORTCUTS = [
  { label: 'Open shortcut help', mac: 'Cmd + /', win: 'Ctrl + /' },
  { label: 'Go to Dashboard',    mac: 'Cmd + D', win: 'Ctrl + D' },
  { label: 'Open search',        mac: 'Cmd + K', win: 'Ctrl + K' },
  { label: 'Toggle dark mode',   mac: 'Cmd + Shift + L', win: 'Ctrl + Shift + L' },
];
```

### Platform Detection

```ts
// utils/platform.ts
export const isMac = () =>
  typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);
```

### Help Modal

Triggered by Cmd/Ctrl + /, lists all shortcuts with kbd styled key badges.
Must be closeable with Escape and fully keyboard navigable.

## Acceptance Criteria

- [ ] Global shortcuts work for navigation and common actions
- [ ] Cmd/Ctrl + / opens a help modal listing all shortcuts
- [ ] Shortcuts suppressed when focus is inside an input or textarea
- [ ] Platform-correct key labels shown
- [ ] All shortcuts registered in a central registry
- [ ] Modal closeable with Escape
