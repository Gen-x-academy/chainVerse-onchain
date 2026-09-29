## Summary

This PR implements the scholarship communications contract addressing four accessibility and communications issues:

### Implemented Features

1. **Meet WCAG Requirements Across Scholarship Journeys** (#1121)
   - `record_accessibility_audit()` records audit results per journey stage + template
   - Checks: keyboard, focus, labels, errors, announcements, contrast, zoom, reduced-motion
   - WCAG levels A/AA/AAA supported
   - Failed checks block template activation at required level
   - Automated and manual checks recorded

2. **Version Communication Templates** (#1120)
   - `create_template()` / `update_template()` / `approve_template()` manage localized templates
   - Template variables with validation (regex, max length, required flags)
   - `validate_template_variables()` fails invalid variables before publication
   - Delivery records identify template version
   - Sensitive data not logged (only template ID, version, event type)

3. **Schedule Deadline and Action Reminders** (#1119)
   - `schedule_reminder()` / `cancel_reminder()` / `mark_reminder_sent()` for configurable reminders
   - Timezone-aware scheduling (stores UTC, applies user offset)
   - Deduplicated via `deduplication_key` (user + program + event + channel + time)
   - Cancellable after completion via `cancel_reminder()`
   - Respects quiet hours and `mandatory_only` preference

4. **Add Per-Event Communication Preferences** (#1118)
   - `set_preferences()` / `get_preferences()` / `is_channel_enabled()` for in-app, email, SMS, push
   - Preferences scoped to user + program
   - Consent-aware: users opt-in per event type + channel
   - Changes take effect predictably without suppressing required messages
   - Mandatory operational notices always delivered

### Contract Added

- `contracts/scholarship-communications/` - New Soroban contract with:
  - Per-event communication preferences (in-app, email, SMS, push)
  - Versioned templates with safe variable validation
  - Timezone-aware reminder scheduling with deduplication
  - WCAG accessibility audit recording

### Documentation Added

- `contracts/docs/scholarship-communications.md` - Complete contract documentation

### Tests Included

- Unit tests for preferences, templates, reminders, accessibility audits
- Authorization boundary tests
- Variable validation tests

### Closes

Closes #1121, #1120, #1119, #1118