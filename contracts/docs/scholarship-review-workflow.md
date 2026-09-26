# Scholarship Review Workflow Contract

## Overview

The `scholarship-review-workflow` contract manages the complete review workflow (issues #1082, #1083, #1084, #1085):

- **#1082**: Create versioned scoring rubrics (weighted criteria, scales, guidance, required comments, disqualifying conditions)
- **#1083**: Save private reviewer score drafts (draft ownership, autosave conflict-safe, only submitted affects decisions)
- **#1084**: Submit and lock completed reviews (immutable, auditable amendment, aggregate updates atomic)
- **#1085**: Request additional applicant information (bounded questions, deadlines, versioned responses, notifications)

## Data Model

### ScoringRubric (#1082)
```rust
pub struct ScoringRubric {
    pub id: BytesN<32>,
    pub program_id: BytesN<32>,
    pub name: String,
    pub version: u32,
    pub criteria: Vec<RubricCriterion>,
    pub is_active: bool,
    pub created_at: u64,
    pub created_by: Address,
    pub approved_at: Option<u64>,
    pub approved_by: Option<Address>,
}

pub struct RubricCriterion {
    pub id: String,
    pub name: String,
    pub description: String,
    pub weight: u32,                    // basis points, sum = 10000
    pub scale: CriterionScale,          // Numeric, Boolean, Categorical
    pub guidance: String,
    pub required_comment: bool,
    pub disqualifying: bool,
    pub disqualify_threshold: Option<i32>,
}

pub enum CriterionScale {
    Numeric { min: i32, max: i32 },
    Boolean,
    Categorical { options: u32 },
}
```

Key features:
- Weights must sum to 10000 basis points (100%)
- Published rubrics are immutable (updates create new version)
- Scores identify exact rubric version
- Disqualifying criteria can auto-reject applications

### ReviewDraft (#1083)
```rust
pub struct ReviewDraft {
    pub reviewer: Address,
    pub application_id: BytesN<32>,
    pub rubric_id: BytesN<32>,
    pub rubric_version: u32,
    pub scores: Vec<ReviewScore>,
    pub last_saved_at: u64,
    pub is_complete: bool,
}

pub struct ReviewScore {
    pub criterion_id: String,
    pub score: i32,
    pub comment: String,
}
```

Key features:
- Draft ownership enforced (only reviewer can access)
- Autosave conflict-safe (last write wins, but reviewer controls)
- Only submitted reviews affect decisions
- Scores validated against rubric criteria

### Review (#1084)
```rust
pub struct Review {
    pub id: BytesN<32>,
    pub application_id: BytesN<32>,
    pub reviewer: Address,
    pub rubric_id: BytesN<32>,
    pub rubric_version: u32,
    pub scores: Vec<ReviewScore>,
    pub total_score: u32,        // weighted sum (0-10000)
    pub status: ReviewStatus,    // Draft, Submitted, Amended, Finalized
    pub submitted_at: u64,
    pub finalized_at: Option<u64>,
    pub amendment_count: u32,
}

pub enum ReviewStatus {
    Draft,
    Submitted,
    Amended,
    Finalized,
}
```

Key features:
- Submitted reviews immutable (correction via auditable amendment)
- Aggregate results update atomically
- Amendment creates `ReviewAmendment` record with diff
- Finalized reviews cannot be amended

### ReviewAmendment
```rust
pub struct ReviewAmendment {
    pub id: BytesN<32>,
    pub review_id: BytesN<32>,
    pub amended_by: Address,
    pub previous_scores: Vec<ReviewScore>,
    pub new_scores: Vec<ReviewScore>,
    pub reason: String,
    pub amended_at: u64,
}
```

### InfoRequest & InfoResponse (#1085)
```rust
pub struct InfoRequest {
    pub id: BytesN<32>,
    pub application_id: BytesN<32>,
    pub reviewer: Address,
    pub questions: Vec<String>,
    pub visible_fields: Vec<String>,  // which application fields response can see
    pub deadline: u64,
    pub status: InfoRequestStatus,    // Pending, Responded, Expired, Cancelled
    pub created_at: u64,
    pub responded_at: Option<u64>,
}

pub struct InfoResponse {
    pub id: BytesN<32>,
    pub request_id: BytesN<32>,
    pub applicant: Address,
    pub responses: Vec<String>,
    pub version: u32,
    pub responded_at: u64,
}

pub enum InfoRequestStatus {
    Pending,
    Responded,
    Expired,
    Cancelled,
}
```

Key features:
- Bounded questions (max 20 per request)
- Versioned responses
- Deadlines enforced on-chain
- All parties notified via events

## Events

| Event | Topic | Data |
|-------|-------|------|
| Rubric created | `RUBRNEW` | `(rubric_id, program_id, version)` |
| Rubric updated | `RUBRUPD` | `(rubric_id, new_version)` |
| Rubric activated | `RUBRACT` | `(rubric_id, version, admin)` |
| Draft saved | `DRAFTSAV` | `(reviewer, application_id, is_complete)` |
| Review submitted | `REVSUB` | `(review_id, application_id, reviewer, total_score)` |
| Review amended | `REVAMND` | `(review_id, amendment_id, reviewer)` |
| Review finalized | `REVFIN` | `(review_id, admin)` |
| Info requested | `INFOREQ` | `(request_id, application_id, reviewer, deadline)` |
| Info responded | `INFORESP` | `(response_id, request_id, applicant)` |

## Authorization

| Operation | Authorization |
|-----------|---------------|
| `create_rubric` | Admin |
| `update_rubric` | Admin (only if not active) |
| `activate_rubric` | Admin |
| `save_draft` | Reviewer |
| `get_draft` | Reviewer (own) |
| `submit_review` | Reviewer |
| `amend_review` | Reviewer (own) |
| `finalize_review` | Admin |
| `request_info` | Reviewer |
| `respond_to_info_request` | Applicant |
| `get_info_request` | Public |
| `get_info_response` | Public |

## TTL Policy

All persistent records use:
- `RECORD_MIN_TTL = 3,110,400` (~1 year)
- `RECORD_MAX_TTL = 6,220,800` (~2 years)

## Rubric Versioning (#1082)

- Weights must sum to 10000 basis points
- Published rubrics immutable: updates create new version
- Scores reference exact rubric version
- Criteria scales: Numeric (min/max), Boolean, Categorical
- Disqualifying criteria: if score < threshold, auto-reject
- Required comments enforced at submission

## Draft Management (#1083)

- Private to reviewer: only reviewer can read/write their drafts
- Autosave: `save_draft()` overwrites previous draft (last write wins)
- Conflict-safe: reviewer controls their own draft
- `is_complete` flag: must be true to submit
- Scores validated against rubric criteria at save time

## Review Submission & Locking (#1084)

Submission flow:
1. `submit_review()` from complete draft
2. Calculates weighted total score
3. Creates `Review` with `status = Submitted`
4. Draft preserved for audit (optional)

Amendment flow:
1. `amend_review()` with new scores + reason
2. Creates `ReviewAmendment` with diff
3. Updates review scores and total
4. `status = Amended`, `amendment_count++`

Finalization:
1. `finalize_review()` by admin
2. `status = Finalized`, prevents further amendments

## Info Requests (#1085)

- Max 20 questions per request
- Visible fields specify which application data applicant can reference
- Deadlines enforced on-chain
- Responses versioned (increment on re-submit)
- All parties notified via events
- Expired requests auto-transition to `Expired`

## Migration & Upgrades

- Rubric versions: append-only, never deleted
- Review amendments: full history preserved
- Info requests: append-only log
- Schema changes: new fields optional

## Operational Notes

1. **Rubric design**: Create with `create_rubric()`, validate weights, `activate_rubric()` when ready
2. **Reviewer workflow**: `save_draft()` repeatedly during review, `submit_review()` when complete
3. **Amendments**: Use for corrections; `reason` field for audit trail
4. **Finalization**: Admin finalizes after all reviews in; prevents further changes
5. **Info requests**: Use for missing docs/clarifications; deadline auto-enforced
6. **Aggregation**: Off-chain computes aggregates from submitted reviews

## Security Considerations

- Draft ownership: only reviewer can read/write their drafts
- Review submission: only reviewer can submit their review
- Amendments: reviewer-only, cannot amend finalized
- Info requests: reviewer creates, applicant responds
- Deadlines enforced on-chain
- All arithmetic uses checked operations
- Rubric weights validated at creation (sum = 10000)