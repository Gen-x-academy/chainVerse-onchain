/**
 * Automated Accessibility Testing Setup for chainVerse Frontend
 * 
 * This file configures automated accessibility testing using:
 * - jest-axe: axe-core integration with Jest
 * - @axe-core/react: Runtime accessibility checking
 * - Lighthouse CI: Automated auditing in CI/CD
 */

// ============================================================================
// Jest AXE Configuration
// ============================================================================

import { axe, toHaveNoViolations } from 'jest-axe';
import React from 'react';
import { render } from '@testing-library/react';

// Extend Jest expect with axe matchers
expect.extend(toHaveNoViolations);

// ============================================================================
// Test Utilities
// ============================================================================

/**
 * Custom render function with accessibility checking
 */
export function renderWithAccessibilityCheck(ui: React.ReactElement, options?: any) {
  const renderResult = render(ui, options);
  
  return {
    ...renderResult,
    
    // Check for accessibility violations
    async checkAccessibility() {
      const results = await axe(renderResult.container);
      expect(results).toHaveNoViolations();
      return results;
    },
  };
}

// ============================================================================
// Component Test Examples
// ============================================================================

describe('Accessibility Tests', () => {
  
  test('Button component should be accessible', async () => {
    const { container, checkAccessibility } = renderWithAccessibilityCheck(
      <Button onClick={() => {}}>Click me</Button>
    );
    
    await checkAccessibility();
    
    // Additional button-specific tests
    const button = container.querySelector('button');
    expect(button).toHaveAttribute('type');
  });

  test('Input component should have proper labels', async () => {
    const { container, checkAccessibility } = renderWithAccessibilityCheck(
      <Input 
        label="Email Address" 
        type="email" 
        required 
        error="Invalid email format"
      />
    );
    
    await checkAccessibility();
    
    // Verify label association
    const input = container.querySelector('input');
    const label = container.querySelector('label');
    
    expect(input).toHaveAttribute('aria-required', 'true');
    expect(input).toHaveAttribute('aria-invalid', 'true');
    expect(input).toHaveAttribute('aria-describedby');
    expect(label).toHaveAttribute('for', input?.id);
  });

  test('Modal component should trap focus', async () => {
    const { container, checkAccessibility } = renderWithAccessibilityCheck(
      <Modal isOpen={true} onClose={() => {}} title="Test Modal">
        <p>Modal content</p>
        <button>Close</button>
      </Modal>
    );
    
    await checkAccessibility();
    
    // Verify modal attributes
    const dialog = container.querySelector('[role="dialog"]');
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAttribute('aria-labelledby');
  });

  test('Form should announce errors to screen readers', async () => {
    const { container, getByRole } = renderWithAccessibilityCheck(
      <RegistrationForm />
    );
    
    // Submit form to trigger validation
    const submitButton = getByRole('button', { name: /register/i });
    submitButton.click();
    
    // Wait for error announcement
    await waitFor(() => {
      const alert = getByRole('alert');
      expect(alert).toBeInTheDocument();
    });
    
    // Verify error summary
    const errorSummary = container.querySelector('[role="alert"]');
    expect(errorSummary).toHaveAttribute('aria-live', 'assertive');
  });

  test('Tabs component should have proper keyboard navigation', async () => {
    const { container, getAllByRole } = renderWithAccessibilityCheck(
      <Tabs tabs={testTabs} />
    );
    
    const tabs = getAllByRole('tab');
    
    // Verify tab attributes
    tabs.forEach((tab, index) => {
      expect(tab).toHaveAttribute('role', 'tab');
      expect(tab).toHaveAttribute('aria-selected');
      expect(tab).toHaveAttribute('aria-controls');
    });
    
    // Test keyboard navigation
    tabs[0].focus();
    fireEvent.keyDown(tabs[0], { key: 'ArrowRight' });
    
    expect(tabs[1]).toHaveFocus();
  });

  test('Alert component should use appropriate ARIA roles', async () => {
    const { container, rerender } = renderWithAccessibilityCheck(
      <Alert type="info">Information message</Alert>
    );
    
    let alert = container.querySelector('[role="status"]');
    expect(alert).toHaveAttribute('aria-live', 'polite');
    
    // Test error alert
    rerender(<Alert type="error">Error message</Alert>);
    
    alert = container.querySelector('[role="alert"]');
    expect(alert).toHaveAttribute('aria-live', 'assertive');
  });
});

// ============================================================================
// Integration Tests
// ============================================================================

describe('Page-Level Accessibility', () => {
  
  test('Homepage should meet WCAG AA standards', async () => {
    const { container } = render(<HomePage />);
    const results = await axe(container, {
      rules: {
        'color-contrast': { enabled: true },
        'label': { enabled: true },
        'link-name': { enabled: true },
        'image-alt': { enabled: true },
      },
    });
    
    expect(results.violations).toHaveLength(0);
  });

  test('Dashboard should be navigable by keyboard', async () => {
    const { container } = render(<Dashboard />);
    
    // Get all interactive elements
    const interactiveElements = container.querySelectorAll(
      'a[href], button, input, select, textarea, [tabindex]'
    );
    
    expect(interactiveElements.length).toBeGreaterThan(0);
    
    // Verify each element is focusable
    interactiveElements.forEach(el => {
      expect(el).not.toHaveAttribute('tabindex', '-1');
    });
  });

  test('Course listing should have proper heading hierarchy', async () => {
    const { container } = render(<CourseList />);
    
    const headings = container.querySelectorAll('h1, h2, h3, h4, h5, h6');
    
    // Verify only one H1
    const h1Count = Array.from(headings).filter(h => h.tagName === 'H1').length;
    expect(h1Count).toBe(1);
    
    // Verify no skipped heading levels
    let previousLevel = 0;
    headings.forEach(heading => {
      const level = parseInt(heading.tagName.charAt(1));
      expect(level).toBeLessThanOrEqual(previousLevel + 1);
      previousLevel = level;
    });
  });
});

// ============================================================================
// E2E Accessibility Tests with Playwright
// ============================================================================

import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

test.describe('E2E Accessibility Tests', () => {
  
  test('Homepage should not have accessibility violations', async ({ page }) => {
    await page.goto('/');
    
    const accessibilityScanResults = await new AxeBuilder({ page }).analyze();
    
    expect(accessibilityScanResults.violations).toEqual([]);
  });

  test('Course registration flow should be accessible', async ({ page }) => {
    await page.goto('/courses');
    
    // Navigate through the flow
    await page.click('[data-testid="course-enroll"]');
    await page.fill('[name="email"]', 'test@example.com');
    await page.click('[type="submit"]');
    
    // Check accessibility at each step
    const results = await new AxeBuilder({ page })
      .withTags(['wcag2a', 'wcag2aa'])
      .analyze();
    
    expect(results.violations).toHaveLength(0);
  });

  test('Modal dialogs should trap focus correctly', async ({ page }) => {
    await page.goto('/dashboard');
    
    // Open modal
    await page.click('[data-testid="open-modal"]');
    
    // Wait for modal to be visible
    await page.waitForSelector('[role="dialog"]');
    
    // Tab through all focusable elements
    const dialog = page.locator('[role="dialog"]');
    const focusableElements = dialog.locator(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
    );
    
    const count = await focusableElements.count();
    
    for (let i = 0; i < count; i++) {
      await page.keyboard.press('Tab');
      const focusedElement = page.locator(':focus');
      expect(await dialog.contains(focusedElement)).toBeTruthy();
    }
    
    // One more tab should cycle back to first element
    await page.keyboard.press('Tab');
    const firstElement = focusableElements.first();
    expect(await firstElement.matches(page.locator(':focus'))).toBeTruthy();
  });

  test.skip('Screen reader compatibility', async ({ page, browserName }) => {
    // Note: Actual screen reader testing requires OS-level integration
    // This is a placeholder for manual testing documentation
    
    console.log(`
      Manual Screen Reader Testing Checklist for ${browserName}:
      
      1. NVDA (Windows):
         - Install NVDA from https://www.nvaccess.org/
         - Test with Firefox for best compatibility
         - Verify all interactive elements are announced
         - Check form labels and error messages
         - Test modal announcements
         
      2. VoiceOver (macOS):
         - Enable with Cmd+F5
         - Use Safari for best compatibility
         - Test rotor navigation
         - Verify landmark regions
         - Check live region announcements
         
      3. JAWS (Windows):
         - Commercial screen reader
         - Test with Chrome or Firefox
         - Verify virtual cursor navigation
         - Check ARIA live regions
    `);
  });
});

// ============================================================================
// Lighthouse CI Configuration
// ============================================================================

/**
 * lighthouserc.js
 * 
 * Add this file to your project root for automated accessibility auditing
 */
export const lighthouseConfig = {
  ci: {
    collect: {
      numberOfRuns: 3,
      settings: {
        onlyCategories: ['accessibility'],
        skipAudits: ['uses-http2'], // Skip non-accessibility audits
      },
    },
    assert: {
      assertions: {
        'accessibility': ['error', { minScore: 0.95 }],
        'aria-allowed-attr': 'error',
        'aria-hidden-body': 'error',
        'aria-required-attr': 'error',
        'aria-roles': 'error',
        'aria-valid-attr-value': 'error',
        'aria-valid-attr': 'error',
        'button-name': 'error',
        'color-contrast': 'error',
        'document-title': 'error',
        'duplicate-id-aria': 'error',
        'form-field-multiple-labels': 'error',
        'html-has-lang': 'error',
        'image-alt': 'error',
        'input-image-alt': 'error',
        'label': 'error',
        'landmark-one-main': 'error',
        'link-name': 'error',
        'list': 'error',
        'listitem': 'error',
        'meta-viewport': 'error',
        'region': 'error',
      },
    },
    upload: {
      target: 'temporary-public-storage',
    },
  },
};

// ============================================================================
// GitHub Actions Workflow
// ============================================================================

/**
 * .github/workflows/accessibility.yml
 * 
 * Automated accessibility testing in CI/CD
 */
export const githubWorkflow = `
name: Accessibility Audit

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]

jobs:
  axe-tests:
    runs-on: ubuntu-latest
    
    steps:
      - uses: actions/checkout@v3
      
      - name: Setup Node.js
        uses: actions/setup-node@v3
        with:
          node-version: '18'
      
      - name: Install dependencies
        run: npm ci
      
      - name: Run accessibility tests
        run: npm run test:a11y
      
      - name: Upload test results
        uses: actions/upload-artifact@v3
        if: failure()
        with:
          name: a11y-test-results
          path: test-results/

  lighthouse:
    runs-on: ubuntu-latest
    
    steps:
      - uses: actions/checkout@v3
      
      - name: Build application
        run: |
          npm ci
          npm run build
      
      - name: Serve built site
        run: npx serve -s dist -l 3000 &
      
      - name: Wait for server
        run: sleep 5
      
      - name: Run Lighthouse
        uses: treosh/lighthouse-ci-action@v10
        with:
          urls: |
            http://localhost:3000/
            http://localhost:3000/courses
            http://localhost:3000/dashboard
          settings: |
            {
              "extends": "lighthouse:default",
              "categories": ["accessibility"],
              "skipAudits": ["uses-http2"]
            }
          uploadArtifacts: true
          temporaryPublicStorage: true
      
      - name: Upload Lighthouse results
        uses: actions/upload-artifact@v3
        with:
          name: lighthouse-reports
          path: .lighthouseci/
`;

// ============================================================================
// Package.json Scripts
// ============================================================================

/**
 * Add these scripts to your package.json
 */
export const packageJsonScripts = {
  "test:a11y": "jest --testPathPattern=accessibility",
  "test:a11y:watch": "jest --testPathPattern=accessibility --watch",
  "lighthouse": "lhci autorun",
  "axe": "axe-chrome",
  "pa11y": "pa11y-ci",
};

// ============================================================================
// Accessibility Report Generator
// ============================================================================

interface AccessibilityReport {
  url: string;
  timestamp: string;
  violations: number;
  passes: number;
  incomplete: number;
  inapplicable: number;
  details: any[];
}

export function generateAccessibilityReport(results: any): AccessibilityReport {
  return {
    url: window.location.href,
    timestamp: new Date().toISOString(),
    violations: results.violations.length,
    passes: results.passes.length,
    incomplete: results.incomplete.length,
    inapplicable: results.inapplicable.length,
    details: results.violations.map(violation => ({
      id: violation.id,
      impact: violation.impact,
      description: violation.description,
      helpUrl: violation.helpUrl,
      nodes: violation.nodes.length,
    })),
  };
}

export function saveReportToFile(report: AccessibilityReport, filename: string): void {
  const blob = new Blob([JSON.stringify(report, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}
