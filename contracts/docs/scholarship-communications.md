# Scholarship Communications Contract

## Overview

The `scholarship-communications` contract manages communication preferences, templates, scheduling, and accessibility for the scholarship system (issues #1118, #1119, #1120, #1121):

- **#1118**: Per-event communication preferences (in-app, email, SMS, push)
- **#1119**: Deadline and action reminders (timezone-aware, deduplicated, cancellable)
- **#1120**: Versioned communication templates with safe variables
- **#1121**: WCAG accessibility requirements across scholarship journeys

This contract emits events for off-chain delivery services. No PII is stored on-chain; preferences are scoped to addresses and program IDs.

## Data Model

### CommunicationPreferences (#1118)
```rust
pub struct CommunicationPreferences {
    pub user: Address,
    pub program_id: BytesN<32>,
    pub channels: Map<EventType, Vec<Channel>>,
    pub quiet_hours_start: Option<u8>,    // hour in user's timezone (0-23)
    pub quiet_hours_end: Option<u8>,
    pub timezone_offset_minutes: i32,      // -720 to +840
    pub mandatory_only: bool,              // if true, only mandatory operational notices
    pub updated_at: u64,
}
```

Key features:
- Preferences scoped to user + program
- Consent-aware: users opt-in per event type + channel
- Changes take effect predictably without suppressing required messages
- Mandatory operational notices always delivered regardless of preferences

### CommunicationTemplate (#1120)
```rust
pub struct CommunicationTemplate {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub locale: String,
    pub event_type: EventType,
    pub channel: Channel,
    pub subject: String,
    pub body: String,
    pub variables: Vec<TemplateVariable>,
    pub version: u32,
    pub is_active: bool,
    pub created_at: u64,
    pub created_by: Address,
    pub approved_at: Option<u64>,
    pub approved_by: Option<Address>,
}
```

```rust
pub struct TemplateVariable {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub variable_type: VariableType,  // String, Number, Date, Address, Currency, Url
    pub validation_regex: Option<String>,
    pub max_length: Option<u32>,
}
```

Key features:
- Versioned: updates create new versions, require re-approval
- Safe variables: validation regex, max length, required flags
- Invalid variables fail before publication via `validate_template_variables()`
- Delivery records identify template version
- Sensitive data not logged (only template ID, version, event type)

### Reminder (#1119)
```rust
pub struct Reminder {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub user: Address,
    pub event_type: EventType,
    pub channel: Channel,
    pub template_id: BytesN<32>,
    pub scheduled_at: u64,
    pub timezone_offset_minutes: i32,
    pub status: ReminderStatus,  // Scheduled, Sent, Cancelled, Failed
    pub deduplication_key: String,
    pub sent_at: Option<u64>,
    pub cancelled_at: Option<u64>,
    pub created_at: u64,
}
```

Key features:
- Timezone-aware scheduling (stores UTC, applies user offset)
- Deduplicated via `deduplication_key` (user + program + event + channel + time)
- Cancellable after completion via `cancel_reminder()`
- Respects quiet hours (off-chain worker checks preferences)
- Respects `mandatory_only` preference

### AccessibilityAudit (#1121)
```rust
pub struct AccessibilityAudit {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub template_id: Option<BytesN<32>>,
    pub journey_stage: JourneyStage,  // Discovery, Application, Review, Decision, Acceptance, Milestone, Payment, PostAward
    pub wcag_level: WcagLevel,        // A, AA, AAA
    pub passed_checks: Vec<String>,
    pub failed_checks: Vec<String>,
    pub warnings: Vec<String>,
    pub audited_at: u64,
    pub audited_by: Address,
}
```

Key features:
- Per journey stage + template
- WCAG 2.1 level A/AA/AAA
- Checks: keyboard, focus, labels, errors, announcements, contrast, zoom, reduced-motion
- Automated + manual checks recorded
- Failed checks block template activation at required level

## Event Types

```rust
enum EventType {
    ApplicationOpened,
    ApplicationClosing,
    ApplicationSubmitted,
    ReviewAssigned,
    ReviewCompleted,
    AdditionalInfoRequested,
    DecisionReleased,
    AwardAccepted,
    MilestoneDue,
    MilestoneSubmitted,
    MilestoneApproved,
    PaymentScheduled,
    PaymentCompleted,
    PaymentFailed,
    RefundProcessed,
    RecoveryInitiated,
}
```

## Channels

```rust
enum Channel {
    InApp,
    Email,
    Sms,
    Push,
}
```

## Events

| Event | Topic | Data |
|-------|-------|------|
| Preferences set | `PREFSET` | `(user, program_id)` |
| Reminder scheduled | `REMINDNW` | `(reminder_id, user, program_id, event_type, channel, scheduled_at)` |
| Reminder cancelled | `REMINDCL` | `(reminder_id, user)` |
| Template created | `TMPLNEW` | `(template_id, program_id, event_type, channel)` |
| Template updated | `TMPLUPD` | `(template_id, version)` |
| Template approved | `TMPLAPPR` | `(template_id, admin)` |
| Audit recorded | `AUDITNEW` | `(audit_id, program_id, journey_stage, wcag_level)` |

## Authorization

| Operation | Authorization |
|-----------|---------------|
| `set_preferences` | User themselves |
| `get_preferences` | Public (no auth) |
| `schedule_reminder` | Admin |
| `cancel_reminder` | User themselves |
| `mark_reminder_sent` | Admin (off-chain worker) |
| `create_template` | Admin |
| `update_template` | Admin |
| `approve_template` | Admin |
| `validate_template_variables` | Public |
| `record_accessibility_audit` | Admin |
| `get_accessibility_audit` | Public |

## TTL Policy

All persistent records use:
- `RECORD_MIN_TTL = 3,110,400` (~1 year)
- `RECORD_MAX_TTL = 6,220,800` (~2 years)

## Accessibility Requirements (#1121)

### Checked Properties
| Check | WCAG Criterion | Description |
|-------|----------------|-------------|
| Keyboard navigation | 2.1.1 | All functionality operable via keyboard |
| Focus indicators | 2.4.7 | Visible focus styles |
| Labels | 3.3.2 | Form inputs have associated labels |
| Error identification | 3.3.1 | Errors identified and described |
| Error announcements | 4.1.3 | Status changes announced to AT |
| Color contrast | 1.4.3 | 4.5:1 (AA) / 7:1 (AAA) |
| Text resize | 1.4.4 | 200% zoom without loss |
| Reduced motion | 2.3.3 | Respects `prefers-reduced-motion` |

### Compliance Flow
1. Template created → `is_active = false`
2. Accessibility audit recorded via `record_accessibility_audit()`
3. If `failed_checks` empty for required `WcagLevel` → `approve_template()`
4. If failed checks exist → template cannot be activated at that level

## Migration & Upgrades

- Template versions: immutable once created; updates create new version
- Preferences: additive; new event types auto-disabled until user opts in
- Accessibility: audit records immutable; re-audit creates new record
- Reminders: deduplication key includes all scheduling params

## Operational Notes

1. **Delivery**: Off-chain worker polls `Reminder` records with `status = Scheduled` and `scheduled_at <= now`, checks preferences/quiet hours, delivers via channel, calls `mark_reminder_sent()`
2. **Quiet hours**: Worker converts `scheduled_at` to user timezone, skips if in quiet window, reschedules to `quiet_hours_end`
3. **Mandatory notices**: `mandatory_only = true` means only operational events (PaymentFailed, RecoveryInitiated) delivered
4. **Deduplication**: Key = hash(user, program, event, channel, scheduled_at); prevents duplicate reminders
5. **Template variables**: Always validated before send via `validate_template_variables()`
6. **Sensitive data**: Never in template body; variables injected at send time from secure store

## Security Considerations

- All authorization checks use `require_auth()`
- User-scoped preferences prevent cross-user access
- Template variables validated at send time (not just creation)
- Reminders cancellable only by recipient
- Accessibility audits immutable; re-audit creates new record
- No PII in events (only IDs, timestamps, enums)