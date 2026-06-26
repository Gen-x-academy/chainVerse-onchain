/**
 * Accessible React Component Patterns for chainVerse Frontend
 * 
 * This module provides reusable accessible components following
 * WAI-ARIA best practices and WCAG 2.1 AA guidelines.
 */

import React, { useState, useEffect, useRef, forwardRef, HTMLAttributes } from 'react';

// ============================================================================
// Utility Functions
// ============================================================================

/**
 * Generate unique ID for accessibility attributes
 */
export function generateId(prefix: string = 'id'): string {
  return `${prefix}-${Math.random().toString(36).substr(2, 9)}`;
}

/**
 * Announce message to screen readers
 */
export function announceToScreenReader(message: string, priority: 'polite' | 'assertive' = 'polite'): void {
  const el = document.createElement('div');
  el.setAttribute('role', 'status');
  el.setAttribute('aria-live', priority);
  el.setAttribute('aria-atomic', 'true');
  el.className = 'sr-only';
  el.textContent = message;
  document.body.appendChild(el);
  
  setTimeout(() => {
    document.body.removeChild(el);
  }, 1000);
}

// ============================================================================
// Screen Reader Only Component
// ============================================================================

interface SrOnlyProps {
  children: React.ReactNode;
}

export const SrOnly: React.FC<SrOnlyProps> = ({ children }) => (
  <span
    style={{
      position: 'absolute',
      width: '1px',
      height: '1px',
      padding: '0',
      margin: '-1px',
      overflow: 'hidden',
      clip: 'rect(0, 0, 0, 0)',
      whiteSpace: 'nowrap',
      border: '0',
    }}
  >
    {children}
  </span>
);

// ============================================================================
// Accessible Button Component
// ============================================================================

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'danger' | 'ghost';
  size?: 'small' | 'medium' | 'large';
  isLoading?: boolean;
  leftIcon?: React.ReactNode;
  rightIcon?: React.ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  (
    {
      children,
      variant = 'primary',
      size = 'medium',
      isLoading = false,
      leftIcon,
      rightIcon,
      disabled,
      ...props
    },
    ref
  ) => {
    const buttonClasses = `btn btn-${variant} btn-${size}`;
    
    return (
      <button
        ref={ref}
        className={buttonClasses}
        disabled={disabled || isLoading}
        aria-busy={isLoading}
        {...props}
      >
        {isLoading && (
          <Spinner size="small" aria-hidden="true" />
        )}
        {!isLoading && leftIcon && (
          <span aria-hidden="true">{leftIcon}</span>
        )}
        {children}
        {!isLoading && rightIcon && (
          <span aria-hidden="true">{rightIcon}</span>
        )}
        {isLoading && <SrOnly>Loading...</SrOnly>}
      </button>
    );
  }
);

Button.displayName = 'Button';

// ============================================================================
// Accessible Input Component
// ============================================================================

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  label: string;
  error?: string;
  hint?: string;
  required?: boolean;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  (
    {
      label,
      error,
      hint,
      required,
      id,
      'aria-describedby': ariaDescribedby,
      ...props
    },
    ref
  ) => {
    const inputId = id || generateId('input');
    const errorId = error ? generateId('error') : undefined;
    const hintId = hint ? generateId('hint') : undefined;
    
    const describedBy = [
      hintId,
      errorId,
      ariaDescribedby,
    ]
      .filter(Boolean)
      .join(' ') || undefined;

    return (
      <div className="form-group">
        <label htmlFor={inputId} className="form-label">
          {label}
          {required && (
            <>
              {' '}
              <span aria-hidden="true">*</span>
              <SrOnly>(required)</SrOnly>
            </>
          )}
        </label>
        
        {hint && !error && (
          <p id={hintId} className="form-hint">
            {hint}
          </p>
        )}
        
        <input
          ref={ref}
          id={inputId}
          aria-required={required}
          aria-invalid={!!error}
          aria-describedby={describedBy}
          {...props}
        />
        
        {error && (
          <p id={errorId} className="form-error" role="alert">
            {error}
          </p>
        )}
      </div>
    );
  }
);

Input.displayName = 'Input';

// ============================================================================
// Accessible Modal/Dialog Component
// ============================================================================

interface ModalProps {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  children: React.ReactNode;
  initialFocusRef?: React.RefObject<HTMLElement>;
}

export const Modal: React.FC<ModalProps> = ({
  isOpen,
  onClose,
  title,
  children,
  initialFocusRef,
}) => {
  const modalRef = useRef<HTMLDivElement>(null);
  const previousActiveElement = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (isOpen) {
      // Store currently focused element
      previousActiveElement.current = document.activeElement as HTMLElement;
      
      // Focus first focusable element or modal itself
      const focusableElements = modalRef.current?.querySelectorAll(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      
      const elementToFocus = initialFocusRef?.current || focusableElements?.[0];
      elementToFocus?.focus();
      
      // Handle escape key
      const handleEscape = (e: KeyboardEvent) => {
        if (e.key === 'Escape') {
          onClose();
        }
      };
      
      // Trap focus
      const handleTabKey = (e: KeyboardEvent) => {
        if (e.key !== 'Tab') return;
        
        const focusableElementsList = Array.from(
          modalRef.current?.querySelectorAll(
            'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
          ) || []
        );
        
        if (focusableElementsList.length === 0) return;
        
        const firstElement = focusableElementsList[0] as HTMLElement;
        const lastElement = focusableElementsList[focusableElementsList.length - 1] as HTMLElement;
        
        if (e.shiftKey) {
          if (document.activeElement === firstElement) {
            e.preventDefault();
            lastElement.focus();
          }
        } else {
          if (document.activeElement === lastElement) {
            e.preventDefault();
            firstElement.focus();
          }
        }
      };
      
      document.addEventListener('keydown', handleEscape);
      document.addEventListener('keydown', handleTabKey);
      
      return () => {
        document.removeEventListener('keydown', handleEscape);
        document.removeEventListener('keydown', handleTabKey);
        
        // Restore focus when modal closes
        if (previousActiveElement.current) {
          previousActiveElement.current.focus();
        }
      };
    }
  }, [isOpen, onClose, initialFocusRef]);

  if (!isOpen) return null;

  return (
    <>
      {/* Backdrop */}
      <div
        className="modal-backdrop"
        onClick={onClose}
        aria-hidden="true"
      />
      
      {/* Modal Dialog */}
      <div
        ref={modalRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="modal-title"
        className="modal"
      >
        <div className="modal-header">
          <h2 id="modal-title" className="modal-title">
            {title}
          </h2>
          <button
            onClick={onClose}
            className="modal-close"
            aria-label="Close dialog"
          >
            <span aria-hidden="true">&times;</span>
          </button>
        </div>
        
        <div className="modal-content">
          {children}
        </div>
      </div>
    </>
  );
};

// ============================================================================
// Accessible Dropdown Menu
// ============================================================================

interface DropdownMenuProps {
  trigger: React.ReactNode;
  children: React.ReactNode;
  label: string;
}

export const DropdownMenu: React.FC<DropdownMenuProps> = ({
  trigger,
  children,
  label,
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuItemsRef = useRef<(HTMLButtonElement | null)[]>([]);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        isOpen &&
        menuRef.current &&
        !menuRef.current.contains(event.target as Node) &&
        !triggerRef.current?.contains(event.target as Node)
      ) {
        setIsOpen(false);
      }
    };

    const handleEscape = (event: KeyboardEvent) => {
      if (isOpen && event.key === 'Escape') {
        setIsOpen(false);
        triggerRef.current?.focus();
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('keydown', handleEscape);

    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleEscape);
    };
  }, [isOpen]);

  const handleKeyDown = (event: React.KeyboardEvent) => {
    switch (event.key) {
      case 'Enter':
      case ' ':
      case 'ArrowDown':
        event.preventDefault();
        setIsOpen(true);
        break;
      case 'Escape':
        setIsOpen(false);
        break;
    }
  };

  return (
    <div className="dropdown">
      <button
        ref={triggerRef}
        onClick={() => setIsOpen(!isOpen)}
        onKeyDown={handleKeyDown}
        aria-expanded={isOpen}
        aria-haspopup="menu"
        aria-label={label}
        className="dropdown-trigger"
      >
        {trigger}
      </button>
      
      {isOpen && (
        <div
          ref={menuRef}
          role="menu"
          className="dropdown-menu"
          aria-label={label}
        >
          {React.Children.map(children, (child, index) => {
            if (React.isValidElement(child)) {
              return React.cloneElement(child as any, {
                ref: (el: HTMLButtonElement) => {
                  menuItemsRef.current[index] = el;
                },
                role: 'menuitem',
                onKeyDown: (e: React.KeyboardEvent) => {
                  if (e.key === 'ArrowDown' && index < React.Children.count(children) - 1) {
                    e.preventDefault();
                    menuItemsRef.current[index + 1]?.focus();
                  } else if (e.key === 'ArrowUp' && index > 0) {
                    e.preventDefault();
                    menuItemsRef.current[index - 1]?.focus();
                  }
                },
              });
            }
            return child;
          })}
        </div>
      )}
    </div>
  );
};

// ============================================================================
// Accessible Tabs Component
// ============================================================================

interface Tab {
  id: string;
  label: string;
  panel: React.ReactNode;
}

interface TabsProps {
  tabs: Tab[];
  defaultTab?: number;
}

export const Tabs: React.FC<TabsProps> = ({ tabs, defaultTab = 0 }) => {
  const [activeTabIndex, setActiveTabIndex] = useState(defaultTab);
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  const handleKeyDown = (
    event: React.KeyboardEvent,
    index: number
  ) => {
    let newIndex = index;

    switch (event.key) {
      case 'ArrowLeft':
        newIndex = index > 0 ? index - 1 : tabs.length - 1;
        break;
      case 'ArrowRight':
        newIndex = index < tabs.length - 1 ? index + 1 : 0;
        break;
      case 'Home':
        newIndex = 0;
        break;
      case 'End':
        newIndex = tabs.length - 1;
        break;
      default:
        return;
    }

    event.preventDefault();
    setActiveTabIndex(newIndex);
    tabRefs.current[newIndex]?.focus();
  };

  return (
    <div className="tabs">
      <div
        role="tablist"
        aria-label="Content sections"
        className="tabs-list"
      >
        {tabs.map((tab, index) => (
          <button
            key={tab.id}
            ref={(el) => (tabRefs.current[index] = el)}
            role="tab"
            id={`tab-${tab.id}`}
            aria-selected={activeTabIndex === index}
            aria-controls={`panel-${tab.id}`}
            tabIndex={activeTabIndex === index ? 0 : -1}
            onClick={() => setActiveTabIndex(index)}
            onKeyDown={(e) => handleKeyDown(e, index)}
            className="tab-button"
          >
            {tab.label}
          </button>
        ))}
      </div>
      
      {tabs.map((tab, index) => (
        <div
          key={tab.id}
          role="tabpanel"
          id={`panel-${tab.id}`}
          aria-labelledby={`tab-${tab.id}`}
          tabIndex={0}
          hidden={activeTabIndex !== index}
          className="tab-panel"
        >
          {activeTabIndex === index && tab.panel}
        </div>
      ))}
    </div>
  );
};

// ============================================================================
// Accessible Alert Component
// ============================================================================

interface AlertProps {
  type: 'info' | 'success' | 'warning' | 'error';
  children: React.ReactNode;
  onDismiss?: () => void;
}

export const Alert: React.FC<AlertProps> = ({
  type,
  children,
  onDismiss,
}) => {
  const roleMap = {
    info: 'status',
    success: 'status',
    warning: 'alert',
    error: 'alert',
  };

  const iconMap = {
    info: 'ℹ️',
    success: '✅',
    warning: '⚠️',
    error: '❌',
  };

  return (
    <div
      role={roleMap[type]}
      aria-live={type === 'error' || type === 'warning' ? 'assertive' : 'polite'}
      className={`alert alert-${type}`}
    >
      <span aria-hidden="true">{iconMap[type]}</span>
      <span>{children}</span>
      {onDismiss && (
        <button
          onClick={onDismiss}
          className="alert-dismiss"
          aria-label={`Dismiss ${type} message`}
        >
          <span aria-hidden="true">&times;</span>
        </button>
      )}
    </div>
  );
};

// ============================================================================
// Spinner Component
// ============================================================================

interface SpinnerProps {
  size?: 'small' | 'medium' | 'large';
  label?: string;
}

export const Spinner: React.FC<SpinnerProps> = ({
  size = 'medium',
  label = 'Loading',
}) => {
  return (
    <div
      role="status"
      aria-label={label}
      className={`spinner spinner-${size}`}
    >
      <div className="spinner-circle" aria-hidden="true" />
      <SrOnly>{label}</SrOnly>
    </div>
  );
};

// ============================================================================
// Skip Link Component
// ============================================================================

interface SkipLinkProps {
  targetId: string;
  label?: string;
}

export const SkipLink: React.FC<SkipLinkProps> = ({
  targetId,
  label = 'Skip to main content',
}) => {
  return (
    <a
      href={`#${targetId}`}
      className="skip-link"
      style={{
        position: 'absolute',
        top: '-40px',
        left: '0',
        background: '#000',
        color: '#fff',
        padding: '8px 16px',
        zIndex: 9999,
        transition: 'top 0.3s',
      }}
      onFocus={(e) => (e.currentTarget.style.top = '0')}
      onBlur={(e) => (e.currentTarget.style.top = '-40px')}
    >
      {label}
    </a>
  );
};

// ============================================================================
// Form Field Group Component
// ============================================================================

interface FieldGroupProps {
  legend: string;
  children: React.ReactNode;
  error?: string;
  hint?: string;
}

export const FieldGroup: React.FC<FieldGroupProps> = ({
  legend,
  children,
  error,
  hint,
}) => {
  const hintId = hint ? generateId('hint') : undefined;
  const errorId = error ? generateId('error') : undefined;

  return (
    <fieldset
      aria-describedby={[hintId, errorId].filter(Boolean).join(' ') || undefined}
      className="field-group"
    >
      <legend>{legend}</legend>
      
      {hint && !error && (
        <p id={hintId} className="field-hint">
          {hint}
        </p>
      )}
      
      {children}
      
      {error && (
        <p id={errorId} className="field-error" role="alert">
          {error}
        </p>
      )}
    </fieldset>
  );
};

// Export all components
export {
  Button,
  Input,
  Modal,
  DropdownMenu,
  Tabs,
  Alert,
  Spinner,
  SkipLink,
  FieldGroup,
  SrOnly,
};
