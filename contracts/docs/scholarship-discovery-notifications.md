# Scholarship Discovery & Notifications Contract

## Overview

The `scholarship-discovery-notifications` contract provides discovery and notification features (issues #1114, #1115, #1116, #1117):

- **#1114**: Scholarship catalog search and filters (URL-backed, stable pagination)
- **#1115**: Personalized scholarship matching (explainable, cold-start, privacy-preserving)
- **#1116**: Shareable public program pages (accessible, indexable, canonical URLs)
- **#1117**: Scholarship notification events (idempotent, stable references, minimal data)

## Data Model

### SearchParams & SearchResult (#1114)
```rust
pub struct SearchParams {
    pub filters: SearchFilters,
    pub sort_by: SortBy,           // DeadlineAsc, DeadlineDesc, AmountAsc, AmountDesc, Relevance, Newest
    pub page_size: u32,            // max 100, default 20
    pub cursor: Option<String>,    // opaque cursor for pagination
}

pub struct SearchFilters {
    pub sponsor_id: Option<BytesN<32>>,
    pub eligibility_criteria: Option<EligibilityCriteria>,
    pub min_award: Option<i128>,
    pub max_award: Option<i128>,
    pub funding_type: Option<FundingType>,
    pub deadline_from: Option<u64>,
    pub deadline_to: Option<u64>,
    pub term: Option<String>,
    pub status: Option<ProgramStatus>,
    pub currency: Option<Symbol>,
}

pub struct SearchResult {
    pub programs: Vec<ProgramSummary>,
    pub total_count: u32,
    pub next_cursor: Option<String>,
    pub filters_applied: SearchFilters,
}
```

Key features:
- URL-backed filters (shareable search URLs)
- Counts reflect the query
- Closed/private programs excluded
- Stable cursor-based pagination

### StudentProfile & MatchingResult (#1115)
```rust
pub struct StudentProfile {
    pub student: Address,
    pub interests: Vec<String>,
    pub academic_level: String,
    pub field_of_study: String,
    pub gpa: Option<String>,
    pub location: String,
    pub demographic_data: Map<String, String>, // optional, protected
    pub completed_applications: Vec<BytesN<32>>,
    pub updated_at: u64,
}

pub struct Recommendation {
    pub program_id: BytesN<32>,
    pub score: u32,              // 0-10000 basis points
    pub reasons: Vec<String>,    // explainable
    pub matched_criteria: Vec<String>,
}

pub struct MatchingResult {
    pub student: Address,
    pub recommendations: Vec<Recommendation>,
    pub cold_start: bool,
    pub generated_at: u64,
}
```

Key features:
- Recommendations explain reasons
- Cold start support (new students get general recommendations)
- Dismissal support for feedback
- Protected traits excluded from ranking unless legally justified
- Privacy: demographic data optional, not used in ranking

### PublicProgramPage (#1116)
```rust
pub struct PublicProgramPage {
    pub program_id: BytesN<32>,
    pub canonical_url: String,
    pub title: String,
    pub description: String,
    pub sponsor_name: String,
    pub sponsor_id: BytesN<32>,
    pub currency: Symbol,
    pub funding_type: FundingType,
    pub max_award: i128,
    pub min_award: i128,
    pub deadline: u64,
    pub timezone_offset_minutes: i32,
    pub eligibility_summary: String,
    pub application_window: ProgramWindow,
    pub award_budget: AwardBudget,
    pub structured_data: Map<String, String>, // JSON-LD compatible
    pub last_updated: u64,
    pub is_published: bool,
    pub is_private: bool,
}
```

Key features:
- Canonical URL for sharing
- JSON-LD structured data for SEO
- Only published fields appear
- Revisions update safely
- Private/invitation-only programs cannot leak (is_private flag)
- Metadata tags set per page

### NotificationEvent (#1117)
```rust
pub struct NotificationEvent {
    pub id: BytesN<32>,
    pub program_id: Option<BytesN<32>>,
    pub recipient: Option<Address>,  // None = broadcast
    pub event_type: NotificationEventType,
    pub title: String,
    pub body: String,
    pub reference_id: Option<BytesN<32>>,  // intent_id, application_id, etc.
    pub reference_type: Option<String>,
    pub priority: NotificationPriority,    // Low, Normal, High, Critical
    pub emitted_at: u64,
    pub deduplication_key: String,
}
```

Key features:
- Idempotent: deduplication_key prevents duplicates
- Stable references: reference_id links to intent/application/milestone
- Minimal data: only what channels need
- Priority for delivery ordering
- Broadcast support (recipient = None)

## Event Types

| Event Type | Description |
|------------|-------------|
| `ApplicationOpened` | New program accepting applications |
| `ApplicationClosing` | Deadline approaching |
| `ApplicationSubmitted` | Student submitted application |
| `ReviewAssigned` | Reviewer assigned |
| `ReviewCompleted` | Review submitted |
| `AdditionalInfoRequested` | Info needed from applicant |
| `DecisionReleased` | Decision available |
| `AwardAccepted` | Student accepted award |
| `MilestoneDue` | Milestone deadline approaching |
| `MilestoneSubmitted` | Evidence submitted |
| `MilestoneApproved` | Milestone approved |
| `PaymentScheduled` | Payment queued |
| `PaymentCompleted` | Payment confirmed |
| `PaymentFailed` | Payment failed |
| `RefundProcessed` | Refund completed |
| `RecoveryInitiated` | Clawback started |
| `InfoRequested` | Additional info needed |

## Events

| Event | Topic | Data |
|-------|-------|------|
| Search performed | `SEARCH` | `(student, filters_hash, result_count)` |
| Recommendations generated | `MATCHGEN` | `(student, cold_start, rec_count)` |
| Page generated | `PAGEGEN` | `(program_id, is_published, is_private)` |
| Notification emitted | `NOTIFEVT` | `(event_id, event_type, priority, dedup_key)` |

## Authorization

| Operation | Authorization |
|-----------|---------------|
| `search_programs` | Public |
| `get_filter_options` | Public |
| `update_student_profile` | Student themselves |
| `get_recommendations` | Student themselves |
| `dismiss_recommendation` | Student themselves |
| `generate_program_page` | Admin (program owner/sponsor) |
| `get_program_page` | Public (if published, not private) |
| `emit_notification` | Admin (program owner/sponsor) |
| `get_notification_event` | Public |

## TTL Policy

All persistent records use:
- `RECORD_MIN_TTL = 3,110,400` (~1 year)
- `RECORD_MAX_TTL = 6,220,800` (~2 years)

## Search & Discovery (#1114)

- **Filters**: URL-backed (e.g., `?funding_type=scholarship&min_award=1000`)
- **Sorting**: DeadlineAsc, DeadlineDesc, AmountAsc, AmountDesc, Relevance, Newest
- **Pagination**: Opaque cursor, max 100/page, default 20
- **Exclusions**: Closed/private programs excluded automatically
- **Counts**: Reflect actual query results

## Personalized Matching (#1115)

Algorithm considerations:
1. Interest matching (student interests ↔ program tags)
2. Eligibility fit (student profile ↔ program criteria)
3. Deadline urgency (closing soon = higher relevance)
4. Award amount fit (student preferences)

Explainability:
- Each recommendation includes `reasons` array
- `matched_criteria` shows which criteria aligned
- Cold start: new students get general recommendations (popular, closing soon)

Privacy:
- Demographic data optional, never used in ranking
- Protected traits excluded unless legally justified
- Dismissal feedback improves future recommendations

## Public Program Pages (#1116)

Structured data (JSON-LD):
```json
{
  "@context": "https://schema.org",
  "@type": "Scholarship",
  "identifier": "program_id",
  "name": "title",
  "description": "description",
  "provider": "sponsor_name",
  "currency": "XLM",
  "fundingType": "Scholarship",
  "maxValue": "10000",
  "minValue": "1000",
  "validUntil": "1735689600",
  "applicationStartDate": "1704067200",
  "applicationEndDate": "1735689600"
}
```

Access control:
- `is_published`: visible in search/catalog
- `is_private`: invitation-only, never in public listings
- Only published fields appear in page
- Revisions update safely (new version replaces old)

## Notification Events (#1117)

Idempotency:
- `deduplication_key` = hash(program_id, recipient, event_type, priority, timestamp)
- Off-chain delivery checks dedup key before sending
- Same event emitted twice → second ignored

Minimal data:
- Only title, body, reference_id, reference_type
- No PII in event
- Channels enrich from reference

Priorities:
- Critical: PaymentFailed, RecoveryInitiated
- High: DecisionReleased, PaymentCompleted, DeadlineApproaching
- Normal: ApplicationSubmitted, ReviewCompleted, MilestoneDue
- Low: ApplicationOpened, InfoRequested

## Migration & Upgrades

- Search index: external (Meilisearch/Elasticsearch), contract stores params only
- Matching algorithm: off-chain, contract stores profile only
- Notification events: append-only log
- Structured data: extensible Map

## Operational Notes

1. **Search**: Uses external indexer; contract validates params only
2. **Matching**: Off-chain ML; contract stores profile + generates recommendations
3. **Pages**: Generated on publish; stored in CDN; contract stores metadata
4. **Notifications**: Emitted by admin; off-chain worker delivers via channels
5. **Deduplication**: 24-hour window for same event/recipient

## Security Considerations

- Search params validated (page size ≤ 100)
- Student profile self-managed (no admin access to demographic data)
- Program pages: private programs never indexed
- Notification dedup prevents spam
- No PII in events (only references)
- Demographic data never used in ranking