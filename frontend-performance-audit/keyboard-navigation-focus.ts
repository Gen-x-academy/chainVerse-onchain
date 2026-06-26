/**
 * Keyboard Navigation & Focus Management Utilities
 * 
 * This module provides utilities for managing keyboard navigation and focus
 * to ensure your application is fully accessible via keyboard only.
 */

// ============================================================================
// Focus Trap Hook
// ============================================================================

import { useEffect, useRef } from 'react';

interface UseFocusTrapOptions {
  enabled?: boolean;
  initialFocus?: number;
  onEscape?: () => void;
}

export function useFocusTrap(
  containerRef: React.RefObject<HTMLElement>,
  options: UseFocusTrapOptions = {}
) {
  const { enabled = true, initialFocus = 0, onEscape } = options;
  const previousActiveElement = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!enabled || !containerRef.current) return;

    // Store currently focused element
    previousActiveElement.current = document.activeElement as HTMLElement;

    const container = containerRef.current;
    
    // Get all focusable elements
    const focusableElements = getFocusableElements(container);
    
    // Focus initial element
    if (focusableElements.length > 0) {
      focusableElements[initialFocus]?.focus();
    }

    // Handle tab key
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Tab') {
        const currentFocusableElements = getFocusableElements(container);
        
        if (currentFocusableElements.length === 0) return;

        const firstElement = currentFocusableElements[0];
        const lastElement = currentFocusableElements[currentFocusableElements.length - 1];

        if (event.shiftKey) {
          // Shift + Tab
          if (document.activeElement === firstElement) {
            event.preventDefault();
            lastElement.focus();
          }
        } else {
          // Tab
          if (document.activeElement === lastElement) {
            event.preventDefault();
            firstElement.focus();
          }
        }
      }

      // Handle escape key
      if (event.key === 'Escape' && onEscape) {
        onEscape();
      }
    };

    document.addEventListener('keydown', handleKeyDown);

    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      
      // Restore focus to previous element
      if (previousActiveElement.current) {
        previousActiveElement.current.focus();
      }
    };
  }, [enabled, containerRef, initialFocus, onEscape]);
}

// ============================================================================
// Get Focusable Elements Utility
// ============================================================================

export function getFocusableElements(container: HTMLElement): HTMLElement[] {
  const focusableSelectors = [
    'button:not([disabled])',
    'a[href]',
    'input:not([disabled])',
    'select:not([disabled])',
    'textarea:not([disabled])',
    '[tabindex]:not([tabindex="-1"])',
    'audio[controls]',
    'video[controls]',
    '[contenteditable]:not([contenteditable="false"])',
    'details>summary:first-of-type',
    'details',
  ].join(', ');

  return Array.from(container.querySelectorAll(focusableSelectors));
}

// ============================================================================
// Focus Manager Class
// ============================================================================

export class FocusManager {
  private static instance: FocusManager;
  private focusHistory: HTMLElement[] = [];

  static getInstance(): FocusManager {
    if (!FocusManager.instance) {
      FocusManager.instance = new FocusManager();
    }
    return FocusManager.instance;
  }

  /**
   * Move focus to next logical element
   */
  moveForward(): void {
    const focusableElements = getFocusableElements(document.body);
    const currentIndex = focusableElements.indexOf(document.activeElement as HTMLElement);
    
    if (currentIndex < focusableElements.length - 1) {
      focusableElements[currentIndex + 1].focus();
    }
  }

  /**
   * Move focus to previous logical element
   */
  moveBackward(): void {
    const focusableElements = getFocusableElements(document.body);
    const currentIndex = focusableElements.indexOf(document.activeElement as HTMLElement);
    
    if (currentIndex > 0) {
      focusableElements[currentIndex - 1].focus();
    }
  }

  /**
   * Save current focus for later restoration
   */
  saveFocus(): void {
    const activeElement = document.activeElement as HTMLElement;
    if (activeElement) {
      this.focusHistory.push(activeElement);
    }
  }

  /**
   * Restore focus to last saved position
   */
  restoreFocus(): void {
    const lastFocused = this.focusHistory.pop();
    if (lastFocused) {
      lastFocused.focus();
    }
  }

  /**
   * Focus first error in form
   */
  focusFirstError(container: HTMLElement = document.body): void {
    const errorElements = container.querySelectorAll('[aria-invalid="true"], .error');
    
    if (errorElements.length > 0) {
      (errorElements[0] as HTMLElement).focus();
    }
  }

  /**
   * Focus first element with specific role
   */
  focusByRole(role: string, container: HTMLElement = document.body): void {
    const element = container.querySelector(`[role="${role}"]`) as HTMLElement;
    
    if (element) {
      element.focus();
    }
  }
}

// ============================================================================
// Keyboard Event Handler
// ============================================================================

interface KeyHandler {
  key: string;
  handler: (event: KeyboardEvent) => void;
  shiftKey?: boolean;
  ctrlKey?: boolean;
  altKey?: boolean;
}

export class KeyboardNavigationManager {
  private handlers: Map<string, KeyHandler[]> = new Map();

  /**
   * Register keyboard shortcut
   */
  registerShortcut(options: KeyHandler): void {
    const { key } = options;
    
    if (!this.handlers.has(key)) {
      this.handlers.set(key, []);
    }
    
    this.handlers.get(key)?.push(options);
  }

  /**
   * Unregister keyboard shortcut
   */
  unregisterShortcut(key: string, handler?: (event: KeyboardEvent) => void): void {
    if (!handler) {
      this.handlers.delete(key);
    } else {
      const handlers = this.handlers.get(key);
      if (handlers) {
        const filtered = handlers.filter(h => h.handler !== handler);
        if (filtered.length === 0) {
          this.handlers.delete(key);
        } else {
          this.handlers.set(key, filtered);
        }
      }
    }
  }

  /**
   * Attach to document
   */
  attach(): void {
    const handleKeyDown = (event: KeyboardEvent) => {
      const handlers = this.handlers.get(event.key);
      
      if (!handlers) return;

      handlers.forEach(handler => {
        if (
          (handler.shiftKey === undefined || handler.shiftKey === event.shiftKey) &&
          (handler.ctrlKey === undefined || handler.ctrlKey === event.ctrlKey) &&
          (handler.altKey === undefined || handler.altKey === event.altKey)
        ) {
          handler.handler(event);
        }
      });
    };

    document.addEventListener('keydown', handleKeyDown);
  }

  /**
   * Common navigation shortcuts
   */
  setupCommonShortcuts(): void {
    // Skip to main content (Alt + S)
    this.registerShortcut({
      key: 's',
      altKey: true,
      handler: (e) => {
        e.preventDefault();
        const mainContent = document.getElementById('main-content');
        mainContent?.focus();
      },
    });

    // Skip to navigation (Alt + N)
    this.registerShortcut({
      key: 'n',
      altKey: true,
      handler: (e) => {
        e.preventDefault();
        const nav = document.querySelector('nav');
        (nav as HTMLElement)?.focus();
      },
    });

    // Close modal/dropdown (Escape)
    this.registerShortcut({
      key: 'Escape',
      handler: (e) => {
        const openModal = document.querySelector('[role="dialog"]:not([hidden])');
        const openDropdown = document.querySelector('[role="menu"]:not([hidden])');
        
        if (openModal || openDropdown) {
          e.preventDefault();
          // Dispatch custom event for components to handle
          window.dispatchEvent(new CustomEvent('close-overlay'));
        }
      },
    });

    // Focus search (Ctrl + K or /)
    this.registerShortcut({
      key: 'k',
      ctrlKey: true,
      handler: (e) => {
        e.preventDefault();
        const searchInput = document.querySelector('input[type="search"]');
        (searchInput as HTMLElement)?.focus();
      },
    });

    this.registerShortcut({
      key: '/',
      handler: (e) => {
        // Only if not in an input field
        const activeElement = document.activeElement;
        if (
          activeElement?.tagName !== 'INPUT' &&
          activeElement?.tagName !== 'TEXTAREA'
        ) {
          e.preventDefault();
          const searchInput = document.querySelector('input[type="search"]');
          (searchInput as HTMLElement)?.focus();
        }
      },
    });
  }
}

// ============================================================================
// Roving Tab Index Hook
// ============================================================================

export function useRovingTabIndex<T extends HTMLElement>(
  itemCount: number,
  orientation: 'horizontal' | 'vertical' = 'horizontal'
) {
  const [focusedIndex, setFocusedIndex] = useState(0);
  const itemRefs = useRef<(T | null)[]>([]);

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent, index: number) => {
      let nextIndex = focusedIndex;

      const moveNext = () => {
        setFocusedIndex(prev => (prev + 1) % itemCount);
      };

      const movePrevious = () => {
        setFocusedIndex(prev => (prev - 1 + itemCount) % itemCount);
      };

      switch (event.key) {
        case 'ArrowRight':
          if (orientation === 'horizontal') {
            event.preventDefault();
            moveNext();
          }
          break;
        case 'ArrowLeft':
          if (orientation === 'horizontal') {
            event.preventDefault();
            movePrevious();
          }
          break;
        case 'ArrowDown':
          if (orientation === 'vertical') {
            event.preventDefault();
            moveNext();
          }
          break;
        case 'ArrowUp':
          if (orientation === 'vertical') {
            event.preventDefault();
            movePrevious();
          }
          break;
        case 'Home':
          event.preventDefault();
          setFocusedIndex(0);
          break;
        case 'End':
          event.preventDefault();
          setFocusedIndex(itemCount - 1);
          break;
        case 'Enter':
        case ' ':
          event.preventDefault();
          itemRefs.current[index]?.click();
          break;
      }
    },
    [focusedIndex, itemCount, orientation]
  );

  useEffect(() => {
    itemRefs.current[focusedIndex]?.focus();
  }, [focusedIndex]);

  return {
    focusedIndex,
    itemRefs,
    handleKeyDown,
    getItemProps: (index: number) => ({
      ref: (el: T | null) => {
        itemRefs.current[index] = el;
      },
      tabIndex: index === focusedIndex ? 0 : -1,
      onKeyDown: (e: React.KeyboardEvent) => handleKeyDown(e, index),
    }),
  };
}

// ============================================================================
// Focus Visible Polyfill
// ============================================================================

export function initFocusVisible() {
  useEffect(() => {
    // Add class when user navigates with keyboard
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Tab') {
        document.body.classList.add('keyboard-navigation');
      }
    };

    // Remove class when user uses mouse
    const handleMouseDown = () => {
      document.body.classList.remove('keyboard-navigation');
    };

    document.addEventListener('keydown', handleKeyDown);
    document.addEventListener('mousedown', handleMouseDown);

    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      document.removeEventListener('mousedown', handleMouseDown);
    };
  }, []);
}

// CSS to use with focus-visible polyfill
export const focusVisibleStyles = `
/* Hide focus outline by default */
*:focus {
  outline: none;
}

/* Show focus outline only during keyboard navigation */
.keyboard-navigation *:focus {
  outline: 3px solid #005fcc;
  outline-offset: 2px;
}

/* Or use :focus-visible where supported */
@supports selector(:focus-visible) {
  *:focus:not(:focus-visible) {
    outline: none;
  }
  
  .keyboard-navigation *:focus-visible {
    outline: 3px solid #005fcc;
    outline-offset: 2px;
  }
}
`;

// ============================================================================
// Live Region Announcer
// ============================================================================

export class Announcer {
  private politeRegion: HTMLElement | null = null;
  private assertiveRegion: HTMLElement | null = null;

  constructor() {
    this.init();
  }

  private init() {
    // Create live regions
    this.politeRegion = document.createElement('div');
    this.politeRegion.setAttribute('role', 'status');
    this.politeRegion.setAttribute('aria-live', 'polite');
    this.politeRegion.setAttribute('aria-atomic', 'true');
    this.politeRegion.className = 'sr-only';
    
    this.assertiveRegion = document.createElement('div');
    this.assertiveRegion.setAttribute('role', 'alert');
    this.assertiveRegion.setAttribute('aria-live', 'assertive');
    this.assertiveRegion.setAttribute('aria-atomic', 'true');
    this.assertiveRegion.className = 'sr-only';

    document.body.appendChild(this.politeRegion);
    document.body.appendChild(this.assertiveRegion);
  }

  announce(message: string, priority: 'polite' | 'assertive' = 'polite'): void {
    const region = priority === 'assertive' ? this.assertiveRegion : this.politeRegion;
    
    if (region) {
      // Clear previous message
      region.textContent = '';
      
      // Set new message after brief delay
      setTimeout(() => {
        region.textContent = message;
      }, 100);
    }
  }

  destroy() {
    if (this.politeRegion) {
      document.body.removeChild(this.politeRegion);
    }
    if (this.assertiveRegion) {
      document.body.removeChild(this.assertiveRegion);
    }
  }
}

// Singleton instance
export const announcer = new Announcer();
