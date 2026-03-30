# Screen Reader Compatibility Guide for chainVerse Frontend

## 📖 Understanding Screen Readers

Screen readers are assistive technologies that convert digital text into synthesized speech or Braille output. They help users who are blind, visually impaired, or have learning disabilities access web content.

### Common Screen Readers

| Screen Reader | Platform | Browser | Cost |
|---------------|----------|---------|------|
| **NVDA** | Windows | Firefox, Chrome | Free |
| **JAWS** | Windows | Chrome, Firefox, Edge | Paid |
| **VoiceOver** | macOS, iOS | Safari | Free (built-in) |
| **TalkBack** | Android | Chrome | Free (built-in) |

---

## 🎯 Key Screen Reader Requirements

### 1. Semantic HTML

Screen readers rely on HTML semantics to understand page structure and convey meaning to users.

#### Headings Hierarchy
```html
<!-- ✅ Good: Logical heading structure -->
<h1>Main Page Title</h1>
  <h2>Section Title</h2>
    <h3>Subsection</h3>
  <h2>Another Section</h2>

<!-- ❌ Bad: Skipped heading levels -->
<h1>Main Title</h1>
<h4>Random Subheading</h4> <!-- Don't do this! -->
```

#### Landmark Regions
```html
<!-- ✅ Good: Define page regions -->
<header role="banner">...</header>
<nav role="navigation" aria-label="Main navigation">...</nav>
<main role="main" id="main-content">...</main>
<aside role="complementary">...</aside>
<footer role="contentinfo">...</footer>

<!-- Screen reader navigation shortcuts:
     - H: Next heading
     - Shift+H: Previous heading
     - R: Next landmark
     - Shift+R: Previous landmark
-->
```

### 2. ARIA Live Regions

Live regions announce dynamic content changes to screen reader users.

#### Polite vs Assertive
```jsx
// ✅ Good: Polite announcement (waits for pause in speech)
<div 
  role="status" 
  aria-live="polite" 
  aria-atomic="true"
>
  {isLoading && 'Loading results...'}
</div>

// ✅ Good: Assertive announcement (interrupts current speech)
<div 
  role="alert" 
  aria-live="assertive" 
  aria-atomic="true"
>
  {error && `Error: ${error.message}`}
</div>
```

#### Transaction Status Example
```jsx
function TransactionStatus({ status }) {
  return (
    <div role="status" aria-live="polite" aria-atomic="true">
      {status === 'pending' && (
        <>
          <Spinner aria-hidden="true" />
          <span>Transaction pending... This may take a few moments.</span>
        </>
      )}
      
      {status === 'confirmed' && (
        <>
          <IconCheck aria-hidden="true" />
          <span>Transaction confirmed on Stellar blockchain!</span>
        </>
      )}
      
      {status === 'failed' && (
        <div role="alert" aria-live="assertive">
          <IconError aria-hidden="true" />
          <span>Transaction failed. Please try again.</span>
        </div>
      )}
    </div>
  );
}
```

### 3. Form Accessibility

Forms are critical interaction points that must be fully accessible.

#### Complete Form Pattern
```jsx
<form aria-labelledby="form-title">
  <h2 id="form-title">Course Registration</h2>
  
  {/* Error Summary */}
  {errors.length > 0 && (
    <div 
      role="alert" 
      aria-live="assertive"
      className="error-summary"
    >
      <h3>Please correct the following errors:</h3>
      <ul>
        {errors.map(error => (
          <li key={error.field}>
            <a href={`#${error.field}`}>{error.message}</a>
          </li>
        ))}
      </ul>
    </div>
  )}
  
  {/* Input Field */}
  <div>
    <label htmlFor="email">
      Email Address
      <span aria-hidden="true">*</span>
      <span className="sr-only">(required)</span>
    </label>
    
    <input
      type="email"
      id="email"
      name="email"
      required
      aria-required="true"
      aria-invalid={!!errors.email}
      aria-describedby={errors.email ? 'email-error' : 'email-hint'}
    />
    
    <p id="email-hint">
      We'll send you a confirmation email with course details.
    </p>
    
    {errors.email && (
      <p id="email-error" role="alert">
        {errors.email}
      </p>
    )}
  </div>
  
  <button type="submit">Register Now</button>
</form>
```

### 4. Link & Button Distinction

Screen readers announce links and buttons differently.

```html
<!-- ✅ Good: Clear distinction -->
<a href="/courses">Browse Courses</a>
<button onClick={handleEnroll}>Enroll Now</button>

<!-- ❌ Bad: Using div as button -->
<div onclick={handleEnroll}>Enroll Now</div>
<!-- Screen reader won't know this is interactive! -->

<!-- ✅ Good: Custom component with proper role -->
<div
  role="button"
  tabindex="0"
  aria-pressed={isPressed}
  onclick={handleClick}
  onkeydown={handleKeyDown}
>
  Toggle Option
</div>
```

### 5. Image Accessibility

Different images require different alt text strategies.

```jsx
// ✅ Good: Informative image with descriptive alt
<img 
  src="blockchain-diagram.png" 
  alt="Diagram showing Stellar blockchain consensus mechanism with 5 nodes validating transactions"
/>

// ✅ Good: Decorative image with empty alt
<img 
  src="decorative-border.png" 
  alt="" 
  role="presentation"
/>

// ✅ Good: Functional image (icon button)
<button aria-label="Close dialog">
  <img src="close-icon.svg" alt="" aria-hidden="true" />
</button>

// ✅ Good: Complex infographic with long description
<img 
  src="ecosystem-map.png" 
  alt="ChainVerse ecosystem overview"
  aria-describedby="ecosystem-description"
/>
<p id="ecosystem-description" hidden>
  The ChainVerse ecosystem consists of three main components: 
  Students who create accounts and enroll in courses, Instructors 
  who develop and teach courses, and Administrators who manage 
  the platform. These interact through the central blockchain 
  layer which records all certificates, escrow transactions, 
  and reward distributions immutably.
</p>
```

---

## 🔊 Screen Reader Announcements

### Common Scenarios

#### Loading Content
```jsx
function LoadingContent() {
  return (
    <div role="status" aria-live="polite">
      <span className="sr-only">Loading course list, please wait...</span>
      <Spinner aria-hidden="true" />
    </div>
  );
}
```

#### Form Submission Success
```jsx
function SubmissionSuccess() {
  useEffect(() => {
    announceToScreenReader(
      'Registration successful! Confirmation email sent to your inbox.',
      'assertive'
    );
  }, []);
  
  return (
    <div role="status" aria-live="assertive">
      <h2>Registration Complete!</h2>
      <p>We've sent a confirmation email to your inbox.</p>
    </div>
  );
}
```

#### Modal Dialog Opening
```jsx
function Modal({ isOpen, onClose, title, children }) {
  useEffect(() => {
    if (isOpen) {
      announceToScreenReader(`${title} dialog opened`, 'assertive');
    }
  }, [isOpen, title]);
  
  return (
    <div 
      role="dialog" 
      aria-modal="true" 
      aria-labelledby="modal-title"
    >
      <h2 id="modal-title">{title}</h2>
      {children}
    </div>
  );
}
```

#### Tab Panel Changes
```jsx
function Tabs({ tabs, activeTab }) {
  const handleTabChange = (newTab) => {
    setActiveTab(newTab);
    announceToScreenReader(`${tabs[newTab].label} tab selected`, 'polite');
  };
  
  return (
    <div>
      <div role="tablist">
        {tabs.map((tab, index) => (
          <button
            key={tab.id}
            role="tab"
            aria-selected={index === activeTab}
            aria-controls={`panel-${tab.id}`}
            onClick={() => handleTabChange(index)}
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
          tabIndex={0}
          hidden={index !== activeTab}
        >
          {tab.content}
        </div>
      ))}
    </div>
  );
}
```

---

## ⌨️ Screen Reader Keyboard Shortcuts

### NVDA (Windows)

| Key | Action |
|-----|--------|
| **Insert + ↑** | Read current line |
| **Insert + ↓** | Read from cursor |
| **Insert + Home** | Read all |
| **H** | Next heading |
| **Shift + H** | Previous heading |
| **Tab** | Next link/form field/button |
| **Shift + Tab** | Previous link/form field/button |
| **F** | Next form field |
| **B** | Next button |
| **L** | Next list |
| **T** | Next table |
| **D** | Next landmark region |
| **Insert + F7** | List all links |
| **Insert + F12** | Read title |

### JAWS (Windows)

| Key | Action |
|-----|--------|
| **Insert + ↑** | Read current line |
| **Insert + ↓** | Read from cursor |
| **Insert + Home** | Read all |
| **H** | Next heading |
| **Shift + H** | Previous heading |
| **Tab** | Next interactive element |
| **Shift + Tab** | Previous interactive element |
| **F** | Next form field |
| **B** | Next button |
| **L** | Next list item |
| **T** | Next table |
| **R** | Next landmark region |
| **Insert + F7** | Elements list |
| **Insert + T** | Window title |

### VoiceOver (macOS/iOS)

| Key | Action |
|-----|--------|
| **VO + A** | Read all |
| **VO + ↑** | Read from top |
| **VO + ←** | Read previous item |
| **VO + →** | Read next item |
| **VO + Space** | Activate item |
| **VO + H** | Next heading |
| **VO + Shift + H** | Previous heading |
| **VO + L** | Next landmark |
| **VO + D** | Next section |
| **VO + I** | Show item chooser |
| **VO + U** | Open rotor |

---

## 🧪 Testing with Screen Readers

### Manual Testing Checklist

#### Navigation
- [ ] Can navigate entire site using only keyboard
- [ ] Skip links work correctly
- [ ] All interactive elements are reachable
- [ ] Focus order is logical
- [ ] Focus is visible at all times

#### Content
- [ ] Page title is descriptive
- [ ] Headings convey document structure
- [ ] Images have appropriate alt text
- [ ] Links have meaningful text
- [ ] Tables have proper headers
- [ ] Lists are properly marked up

#### Forms
- [ ] All fields have labels
- [ ] Required fields are announced
- [ ] Error messages are clear and associated
- [ ] Form instructions are provided
- [ ] Success/error states announced

#### Dynamic Content
- [ ] Loading states announced
- [ ] Modal dialogs announced
- [ ] Tab panel changes announced
- [ ] Form validation feedback announced
- [ ] Content updates announced via live regions

### Testing Workflow

#### Step 1: Install NVDA (Free)
```bash
# Download from https://www.nvaccess.org/download/
# Install on Windows
# Restart computer after installation
```

#### Step 2: Configure NVDA
```
1. Press Insert + N to open NVDA menu
2. Select Preferences → Settings
3. Adjust speech rate to comfortable level (default: 5)
4. Enable "Speak command keys while typing"
5. Save settings
```

#### Step 3: Test Your Application
```
1. Open your application in Firefox
2. Press Insert + T to hear page title
3. Press Insert + F7 to see elements list
4. Navigate using H (headings), Tab (interactive), B (buttons)
5. Try to complete key tasks without mouse
```

#### Step 4: Document Issues
```markdown
## Screen Reader Testing Report

**Date:** 2024-01-15
**Screen Reader:** NVDA 2023.3
**Browser:** Firefox 121

### Issues Found:

1. **Missing Alt Text**
   - Course thumbnail images lack descriptive alt text
   - Impact: High
   - WCAG: 1.1.1
   
2. **Form Label Missing**
   - Search input has no associated label
   - Impact: Critical
   - WCAG: 1.3.1
   
3. **Focus Not Visible**
   - Custom dropdown doesn't show focus indicator
   - Impact: High
   - WCAG: 2.4.7

### Passed Tests:
✅ Heading hierarchy is logical
✅ Skip link works correctly
✅ Modal traps focus properly
✅ Error messages announced
```

---

## 💡 Best Practices for Specific Components

### Data Tables
```jsx
<table aria-label="Course enrollment statistics">
  <caption>
    Enrollment numbers by course for Fall 2024 semester
  </caption>
  <thead>
    <tr>
      <th scope="col">Course Name</th>
      <th scope="col">Instructor</th>
      <th scope="col">Enrolled</th>
      <th scope="col">Capacity</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>Blockchain Basics</td>
      <td>Dr. Smith</td>
      <td>45</td>
      <td>50</td>
    </tr>
  </tbody>
</table>
```

### Accordions
```jsx
function Accordion({ title, children, isExpanded, onToggle }) {
  const buttonRef = useRef(null);
  
  return (
    <div>
      <h3>
        <button
          ref={buttonRef}
          aria-expanded={isExpanded}
          aria-controls={`accordion-panel-${id}`}
          onClick={onToggle}
        >
          {title}
          <span aria-hidden="true">{isExpanded ? '−' : '+'}</span>
        </button>
      </h3>
      <div
        id={`accordion-panel-${id}`}
        role="region"
        aria-labelledby={`accordion-button-${id}`}
        hidden={!isExpanded}
      >
        {children}
      </div>
    </div>
  );
}
```

### Notifications/Toast Messages
```jsx
function ToastContainer() {
  return (
    <div 
      aria-live="polite" 
      aria-atomic="true"
      className="toast-container"
    >
      {toasts.map(toast => (
        <div 
          key={toast.id}
          role="status"
          className={`toast toast-${toast.type}`}
        >
          {toast.type === 'success' && (
            <IconCheck aria-hidden="true" />
          )}
          <span>{toast.message}</span>
          <button 
            onClick={() => removeToast(toast.id)}
            aria-label={`Dismiss: ${toast.message}`}
          >
            Close
          </button>
        </div>
      ))}
    </div>
  );
}
```

---

## 🚀 Quick Reference: ARIA Roles

### Landmark Roles
- `banner`: Site header
- `navigation`: Navigation links
- `main`: Primary content
- `complementary`: Sidebar/aside
- `contentinfo`: Footer
- `search`: Search form
- `form`: Form region
- `region`: Section of content

### Widget Roles
- `button`: Clickable button
- `link`: Hyperlink
- `checkbox`: On/off toggle
- `radio`: Single-choice option
- `slider`: Range selector
- `tab`: Tab in tab list
- `tabpanel`: Tab content area
- `menu`: Menu options
- `dialog`: Modal window
- `alert`: Important message

### Live Region Roles
- `status`: Polite updates
- `alert`: Assertive updates
- `log`: Chronological log
- `marquee`: Non-urgent updates
- `timer`: Periodic updates

---

## 📚 Additional Resources

### Documentation
- [WAI-ARIA Authoring Practices](https://www.w3.org/WAI/ARIA/apg/)
- [WebAIM Screen Reader Survey](https://webaim.org/projects/screenreadersurvey9/)
- [MDN Accessibility Guide](https://developer.mozilla.org/en-US/docs/Web/Accessibility)

### Tools
- [NVDA Screen Reader](https://www.nvaccess.org/)
- [VoiceOver Getting Started](https://www.apple.com/accessibility/mac/vision/)
- axe DevTools Extension
- WAVE Evaluation Tool

### Training
- WebAIM Introduction to Accessibility
- LinkedIn Learning: Accessibility for Developers
- Udemy: Web Accessibility for Beginners
