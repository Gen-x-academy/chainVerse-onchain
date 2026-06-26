# Accessibility Audit Framework for chainVerse Frontend

## 🎯 Accessibility Goals

### WCAG 2.1 AA Compliance Targets
- **Perceivable**: All users must be able to perceive information presented
- **Operable**: All users must be able to operate the interface
- **Understandable**: All users must be able to understand information and operation
- **Robust**: Content must be interpretable by assistive technologies

### Success Criteria (AA Level)
1. **Color Contrast**: Minimum 4.5:1 for normal text, 3:1 for large text
2. **Keyboard Navigation**: All functionality available via keyboard
3. **Focus Indicators**: Visible focus indicators on all interactive elements
4. **Error Identification**: Clear error messages with suggestions
5. **Labels & Instructions**: All form fields have associated labels
6. **Alt Text**: All images have meaningful alternative text
7. **Heading Structure**: Logical heading hierarchy (H1 → H2 → H3)
8. **ARIA Landmarks**: Proper landmark regions defined

---

## 📊 Accessibility Audit Checklist

### 1. Perceivable Content

#### Text Alternatives ✅
- [ ] All images have alt text
- [ ] Decorative images have empty alt (`alt=""`)
- [ ] Icons have aria-label or aria-hidden
- [ ] Form inputs have associated labels
- [ ] Error messages are clearly associated with inputs
- [ ] Charts/graphs have text descriptions

**Implementation:**
```jsx
// ❌ Bad: Missing alt text
<img src="course-thumbnail.jpg" />

// ✅ Good: Descriptive alt text
<img 
  src="course-thumbnail.jpg" 
  alt="Introduction to Stellar Blockchain - Course thumbnail showing blockchain network diagram" 
/>

// ✅ Good: Decorative image
<img 
  src="decorative-pattern.svg" 
  alt="" 
  role="presentation" 
/>
```

#### Color & Contrast ✅
- [ ] Text has minimum 4.5:1 contrast ratio
- [ ] Large text (18px+) has 3:1 contrast ratio
- [ ] Information not conveyed by color alone
- [ ] Interactive elements have 3:1 contrast against background

**Tools:**
- Chrome DevTools Accessibility Inspector
- WebAIM Contrast Checker
- Stark Plugin (Figma/Sketch)

**Implementation:**
```jsx
// ❌ Bad: Low contrast (gray on gray)
<p style={{ color: '#999' }}>Important information</p>

// ✅ Good: Sufficient contrast
<p style={{ color: '#333' }}>Important information</p>

// ❌ Bad: Color-only distinction
<span className="error">Error occurred</span> // Only red color indicates error

// ✅ Good: Multiple indicators
<span className="error" role="alert">
  <IconError aria-hidden="true" />
  Error occurred - Please try again
</span>
```

### 2. Operable Interface

#### Keyboard Navigation ✅
- [ ] All interactive elements are keyboard accessible
- [ ] Logical tab order (matches visual order)
- [ ] No keyboard traps
- [ ] Custom components support keyboard interaction
- [ ] Focus visible on all interactive elements

**Implementation:**
```jsx
// ❌ Bad: Not keyboard accessible
<div onClick={handleClick}>Click me</div>

// ✅ Good: Keyboard accessible
<button 
  onClick={handleClick}
  onKeyDown={(e) => {
    if (e.key === 'Enter' || e.key === ' ') {
      handleClick();
    }
  }}
  type="button"
>
  Click me
</button>

// ✅ Good: Custom component with keyboard support
const CustomButton = ({ onClick, children }) => (
  <div
    role="button"
    tabIndex={0}
    onClick={onClick}
    onKeyDown={(e) => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        onClick();
      }
    }}
    aria-pressed={false}
  >
    {children}
  </div>
);
```

#### Focus Management ✅
- [ ] Focus indicator visible (minimum 3px outline)
- [ ] Focus order is logical
- [ ] Focus moved to error messages on form submission
- [ ] Modal traps focus correctly
- [ ] Focus returned appropriately after actions

**Implementation:**
```jsx
// ✅ Good: Visible focus indicator
<button 
  onClick={handleClick}
  style={{
    ':focus': {
      outline: '3px solid #005fcc',
      outlineOffset: '2px',
    }
  }}
>
  Submit
</button>

// ✅ Good: Focus trap in modal
const Modal = ({ isOpen, onClose, children }) => {
  const modalRef = useRef(null);
  
  useEffect(() => {
    if (isOpen) {
      const focusableElements = modalRef.current.querySelectorAll(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      const firstElement = focusableElements[0];
      const lastElement = focusableElements[focusableElements.length - 1];
      
      const handleTabKey = (e) => {
        if (e.key === 'Tab') {
          if (e.shiftKey && document.activeElement === firstElement) {
            e.preventDefault();
            lastElement.focus();
          } else if (!e.shiftKey && document.activeElement === lastElement) {
            e.preventDefault();
            firstElement.focus();
          }
        }
      };
      
      document.addEventListener('keydown', handleTabKey);
      firstElement.focus();
      
      return () => document.removeEventListener('keydown', handleTabKey);
    }
  }, [isOpen]);
  
  return isOpen ? (
    <div role="dialog" aria-modal="true" ref={modalRef}>
      {children}
    </div>
  ) : null;
};
```

#### Skip Links ✅
- [ ] Skip to main content link provided
- [ ] Skip link is first focusable element
- [ ] Skip link becomes visible on focus

**Implementation:**
```jsx
// ✅ Good: Skip link implementation
function App() {
  return (
    <>
      <a 
        href="#main-content" 
        className="skip-link"
        style={{
          position: 'absolute',
          top: '-40px',
          left: '0',
          background: '#000',
          color: '#fff',
          padding: '8px',
          zIndex: 9999,
          transition: 'top 0.3s',
        }}
        onFocus={(e) => e.target.style.top = '0'}
        onBlur={(e) => e.target.style.top = '-40px'}
      >
        Skip to main content
      </a>
      
      <header>...</header>
      <nav>...</nav>
      
      <main id="main-content" tabIndex="-1">
        {/* Main content */}
      </main>
    </>
  );
}
```

### 3. Understandable Content

#### Readable Text ✅
- [ ] Language of page declared (`<html lang="en">`)
- [ ] Reading level appropriate for audience
- [ ] Abbreviations expanded on first use
- [ ] Unusual words defined or linked to glossary

**Implementation:**
```html
<!-- ✅ Good: Language declaration -->
<html lang="en">
<head>
  <title>chainVerse Academy - Learn Blockchain Development</title>
</head>
<body>
  <p>
    Soroban (Stellar's smart contract platform) enables...
  </p>
</body>
</html>
```

#### Predictable Behavior ✅
- [ ] Consistent navigation across pages
- [ ] Consistent identification of UI components
- [ ] No unexpected changes on input/focus
- [ ] Changes of context initiated by user

**Implementation:**
```jsx
// ❌ Bad: Unexpected change on focus
<input 
  type="text" 
  onFocus={() => navigate('/different-page')} // Don't do this!
/>

// ✅ Good: Predictable behavior
<input 
  type="text" 
  aria-describedby="email-help"
/>
<div id="email-help">
  We'll never share your email with anyone else.
</div>
```

#### Input Assistance ✅
- [ ] Error messages identify the field
- [ ] Error messages suggest correction
- [ ] Error summary at top of form
- [ ] Required fields clearly marked
- [ ] Successful submissions confirmed

**Implementation:**
```jsx
// ✅ Good: Accessible form with error handling
function RegistrationForm() {
  const [errors, setErrors] = useState({});
  const emailInputRef = useRef(null);
  
  const handleSubmit = async (e) => {
    e.preventDefault();
    
    // Validate
    const newErrors = {};
    if (!email.includes('@')) {
      newErrors.email = 'Please enter a valid email address (e.g., user@example.com)';
    }
    
    if (Object.keys(newErrors).length > 0) {
      setErrors(newErrors);
      
      // Focus first error
      emailInputRef.current?.focus();
      
      // Announce error to screen readers
      announceToScreenReader(`Form has ${Object.keys(newErrors).length} error(s)`);
    }
  };
  
  return (
    <form onSubmit={handleSubmit} noValidate>
      {Object.keys(errors).length > 0 && (
        <div 
          role="alert" 
          aria-live="assertive"
          className="error-summary"
        >
          <h2>Please correct the following errors:</h2>
          <ul>
            {Object.entries(errors).map(([field, message]) => (
              <li key={field}>
                <a href={`#${field}`}>{message}</a>
              </li>
            ))}
          </ul>
        </div>
      )}
      
      <div>
        <label htmlFor="email">
          Email Address <span aria-hidden="true">*</span>
          <span className="sr-only">(required)</span>
        </label>
        <input
          type="email"
          id="email"
          name="email"
          ref={emailInputRef}
          required
          aria-required="true"
          aria-invalid={!!errors.email}
          aria-describedby={errors.email ? 'email-error' : undefined}
        />
        {errors.email && (
          <span id="email-error" role="alert">
            {errors.email}
          </span>
        )}
      </div>
      
      <button type="submit">Register</button>
    </form>
  );
}
```

### 4. Robust Technology

#### HTML Validity ✅
- [ ] Valid HTML markup
- [ ] Proper nesting of elements
- [ ] Unique IDs
- [ ] No duplicate attributes

#### ARIA Best Practices ✅
- [ ] ARIA roles used correctly
- [ ] ARIA properties up-to-date
- [ ] Dynamic content announced
- [ ] Live regions implemented

**Implementation:**
```jsx
// ✅ Good: ARIA live region for dynamic content
function NotificationSystem() {
  return (
    <div 
      aria-live="polite" 
      aria-atomic="true"
      role="status"
    >
      {notification && (
        <div className="notification">
          {notification.message}
        </div>
      )}
    </div>
  );
}

// ✅ Good: Loading state announcement
function AsyncComponent() {
  const [loading, setLoading] = useState(true);
  
  return (
    <>
      <div 
        role="status" 
        aria-live="polite" 
        aria-busy={loading}
      >
        {loading && <span>Loading content...</span>}
        {!loading && <Content />}
      </div>
    </>
  );
}
```

---

## 🔧 Automated Testing Tools

### ESLint Plugin JSX-A11Y
```javascript
// .eslintrc.js
module.exports = {
  extends: [
    'plugin:jsx-a11y/recommended',
  ],
  plugins: ['jsx-a11y'],
  rules: {
    'jsx-a11y/anchor-is-valid': 'error',
    'jsx-a11y/click-events-have-key-events': 'error',
    'jsx-a11y/no-static-element-interactions': 'error',
    'jsx-a11y/alt-text': 'error',
    'jsx-a11y/img-redundant-alt': 'error',
    'jsx-a11y/label-has-associated-control': 'error',
  },
};
```

### Jest AXE for Testing
```javascript
// __tests__/accessibility.test.js
import { axe, toHaveNoViolations } from 'jest-axe';
expect.extend(toHaveNoViolations);

test('homepage should not have accessibility violations', async () => {
  const { container } = render(<HomePage />);
  const results = await axe(container);
  expect(results).toHaveNoViolations();
});
```

### Lighthouse CI Integration
```yaml
# .github/workflows/lighthouse.yml
name: Lighthouse Accessibility Audit
on: [push, pull_request]
jobs:
  lighthouse:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - name: Run Lighthouse
        uses: treosh/lighthouse-ci-action@v8
        with:
          urls: |
            http://localhost:3000/
            http://localhost:3000/courses
            http://localhost:3000/dashboard
          settings: |
            {
              "extends": "lighthouse:default",
              "categories": ["accessibility"]
            }
          uploadArtifacts: true
          temporaryPublicStorage: true
```

---

## 📈 Accessibility Testing Schedule

### Continuous Testing
- **Every Commit**: ESLint jsx-a11y rules
- **Every PR**: Automated axe-core testing
- **Weekly**: Full Lighthouse audit
- **Monthly**: Manual testing with screen readers

### Pre-Deployment Checklist
- [ ] Zero critical accessibility violations
- [ ] All forms have proper labels and error handling
- [ ] All images have appropriate alt text
- [ ] Keyboard navigation works perfectly
- [ ] Screen reader testing completed
- [ ] Color contrast meets WCAG AA
- [ ] Focus management implemented correctly
- [ ] ARIA live regions working properly

---

## 🎯 Smart Contract Specific Accessibility

### Blockchain Transaction Feedback
```jsx
// ✅ Good: Accessible transaction status
function TransactionStatus({ txHash, status }) {
  return (
    <div 
      role="status" 
      aria-live="polite"
      aria-busy={status === 'pending'}
    >
      {status === 'pending' && (
        <>
          <Spinner aria-hidden="true" />
          <span>Transaction pending... This may take a few moments.</span>
        </>
      )}
      
      {status === 'confirmed' && (
        <>
          <IconCheck aria-hidden="true" />
          <span>Transaction confirmed!</span>
          <a 
            href={`https://stellar.expert/explorer/testnet/tx/${txHash}`}
            target="_blank"
            rel="noopener noreferrer"
          >
            View on Stellar Expert 
            <span className="sr-only">(opens in new tab)</span>
          </a>
        </>
      )}
      
      {status === 'failed' && (
        <>
          <IconError aria-hidden="true" />
          <span role="alert">Transaction failed. Please try again.</span>
        </>
      )}
    </div>
  );
}
```

### Wallet Connection Status
```jsx
// ✅ Good: Accessible wallet connection
function WalletConnect() {
  const { isConnected, connect, disconnect, address } = useWallet();
  
  return (
    <div>
      {!isConnected ? (
        <button 
          onClick={connect}
          aria-label="Connect wallet to access your account"
        >
          Connect Wallet
        </button>
      ) : (
        <div>
          <span aria-label="Connected wallet address">
            {address.slice(0, 6)}...{address.slice(-4)}
          </span>
          <button 
            onClick={disconnect}
            aria-label="Disconnect wallet"
          >
            Disconnect
          </button>
        </div>
      )}
    </div>
  );
}
```

---

## 🛠️ Resources & Tools

### Testing Tools
- **axe DevTools**: Browser extension for automated testing
- **WAVE**: Web accessibility evaluation tool
- **Lighthouse**: Built-in Chrome accessibility auditing
- **NVDA**: Free screen reader for Windows testing
- **VoiceOver**: Built-in screen reader for Mac testing

### Development Tools
- **eslint-plugin-jsx-a11y**: Static analysis for React
- **react-aria**: Adobe's accessibility hooks
- **@reach/ui**: Accessible React components
- **downshift**: Accessible autocomplete components

### Documentation
- [WCAG 2.1 Guidelines](https://www.w3.org/WAI/WCAG21/quickref/)
- [WAI-ARIA Authoring Practices](https://www.w3.org/WAI/ARIA/apg/)
- [Inclusive Components](https://inclusive-components.design/)
- [A11y Project Checklist](https://www.a11yproject.com/checklist/)

---

## 📚 Team Training Resources

### Required Reading
1. Web Content Accessibility Guidelines (WCAG) 2.1
2. WAI-ARIA Authoring Practices
3. Inclusive Design Principles

### Practical Exercises
1. Navigate your app using only keyboard
2. Test with screen reader (NVDA/VoiceOver)
3. Use browser with 200% zoom
4. Grayscale mode testing (no color reliance)

### Code Review Checklist
- [ ] Semantic HTML used appropriately
- [ ] ARIA attributes correct and necessary
- [ ] Keyboard navigation tested
- [ ] Focus management implemented
- [ ] Error messages accessible
- [ ] Images have alt text
- [ ] Forms properly labeled
- [ ] Live regions for dynamic content
