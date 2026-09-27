# Scholarship Dashboards Contract

## Overview

The `scholarship-dashboards` contract provides read-only aggregated views for four dashboard types (issues #1110, #1111, #1112, #1113):

- **#1110**: Student Scholarship Dashboard
- **#1111**: Sponsor Program Dashboard
- **#1112**: Reviewer Workbench
- **#1113**: Finance Operations Dashboard

This contract is a read-only aggregation layer. It does not modify state (except for cached view materialization TTLs). All data is derived from the canonical source contracts:
- `scholarship-core`: Program lifecycle and metadata
- `scholarship-programs`: Award budgets and application windows
- `scholarship-disbursements`: Payment intents and wallet verification
- `scholarship-applications`: Application records
- `scholarship-milestones`: Milestone tracking
- `scholarship-finance`: Financial aggregates
- `scholarship-registry`: Application and award registry

## Data Model

### StudentDashboard (#1110)
```rust
pub struct StudentDashboard {
    pub student: Address,
    pub programs: Vec<StudentProgramView>,
    pub drafts: Vec<BytesN<32>>,
    pub submissions: Vec<StudentApplicationView>,
    pub requests: Vec<InfoRequestView>,
    pub decisions: Vec<DecisionView>,
    pub awards: Vec<AwardView>,
    pub milestones: Vec<MilestoneView>,
    pub payments: Vec<PaymentView>,
    pub last_updated: u64,
}
```

Key features:
- Counts and statuses are authoritative
- Partial failures are isolated
- Every actionable state has a clear next step
- Sensitive identity follows blind-review settings

### SponsorDashboard (#1111)
```rust
pub struct SponsorDashboard {
    pub sponsor_id: BytesN<32>,
    pub programs: Vec<SponsorProgramView>,
    pub total_budget: i128,
    pub total_disbursed: i128,
    pub total_fees: i128,
    pub total_refunded: i128,
    pub total_recovered: i128,
    pub pending_liabilities: i128,
    pub last_updated: u64,
}
```

Key features:
- Data is sponsor-scoped
- Freshness is visible via `last_updated`
- Financial summaries reconcile with ledger via `scholarship-finance`

### ReviewerDashboard (#1112)
```rust
pub struct ReviewerDashboard {
    pub reviewer: Address,
    pub assignments: Vec<ReviewerAssignment>,
    pub workload: WorkloadSummary,
    pub deadlines: Vec<UpcomingDeadline>,
    pub conflicts: Vec<ConflictView>,
    pub last_updated: u64,
}
```

Key features:
- Sensitive identity follows blind-review settings (uses `student_anon_id`)
- Navigation is efficient with deadline sorting
- Workload counts match assignments

### FinanceOpsDashboard (#1113)
```rust
pub struct FinanceOpsDashboard {
    pub programs: Vec<FinanceProgramView>,
    pub total_funding: i128,
    pub total_liabilities: i128,
    pub due_payments: Vec<DuePaymentView>,
    pub failed_payments: Vec<FailedPaymentView>,
    pub reconciliation_items: Vec<ReconciliationItem>,
    pub pending_refunds: i128,
    pub pending_recoveries: i128,
    pub last_updated: u64,
}
```

Key features:
- High-risk actions require confirmation and permissions
- Filters are shareable
- Stale financial data is labeled via `last_updated`
- Reconciliation items flag discrepancies

## Events

| Event | Topic | Data |
|-------|-------|------|
| Cache invalidated | `CACHEINV` | `(version)` |

## Caching Strategy

- Dashboard views cached with `CACHE_MIN_TTL = 155,520` (~10.8 hours) and `CACHE_MAX_TTL = 311,040` (~21.6 hours)
- Cache keyed by `(dashboard_type, scope, params_hash)`
- Global cache invalidation via `CacheVersion` counter
- Stale data labeled with `last_updated` timestamp

## Authorization

| Dashboard | Authorization |
|-----------|---------------|
| Student | Must be the student (`require_student`) |
| Sponsor | Must be sponsor admin/member (`require_sponsor`) |
| Reviewer | Must be the reviewer (`require_reviewer`) |
| FinanceOps | Must be admin (`require_admin`) |

## Privacy Considerations

- **Student dashboard**: Only student's own data
- **Sponsor dashboard**: Only sponsor's programs
- **Reviewer workbench**: Blind review uses `student_anon_id` (blinded address)
- **FinanceOps**: Admin only, full financial visibility

No PII is stored in dashboard views. Aggregates only.

## TTL Policy

- Dashboard cache: `CACHE_MIN_TTL = 155,520` (~10.8 hours), `CACHE_MAX_TTL = 311,040` (~21.6 hours)
- Cache version counter in instance storage

## Migration & Upgrades

- Adding new dashboard fields: extend view structs, increment `CONTRACT_VERSION`
- New dashboard types: add to `DashboardType` enum, implement query function
- Cache invalidation: call `invalidate_cache()` after source contract upgrades

## Operational Notes

1. **Freshness**: All views include `last_updated` timestamp; UI should show staleness
2. **Reconciliation**: FinanceOps dashboard includes `ReconciliationItem` for discrepancies
3. **Failed payments**: `FailedPaymentView` includes `retry_count` for prioritization
4. **Conflicts**: Reviewer workbench surfaces declared conflicts for recusal
5. **Blind review**: `ReviewerAssignment.student_anon_id` replaces real address when enabled

## Security Considerations

- All authorization checks use `require_auth()` on caller
- Role-based access prevents cross-role data leakage
- Cache invalidation requires admin auth
- No arithmetic operations that could overflow (read-only aggregates)