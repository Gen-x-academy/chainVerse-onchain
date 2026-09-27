## Summary

This PR implements the scholarship dashboards contract addressing four dashboard issues:

### Implemented Features

1. **Build the Student Scholarship Dashboard** (#1110)
   - `get_student_dashboard()` returns all programs, drafts, submissions, requests, decisions, awards, milestones, payments in one role-safe view
   - Counts and statuses are authoritative
   - Partial failures are isolated
   - Every actionable state has a clear next step
   - Sensitive identity follows blind-review settings

2. **Build the Sponsor Program Dashboard** (#1111)
   - `get_sponsor_dashboard()` / `get_sponsor_program_detail()` show program budget, application funnel, review progress, awards, disbursements, impact indicators
   - Data is sponsor-scoped
   - Freshness is visible via `last_updated`
   - Financial summaries reconcile with ledger via `scholarship-finance` contract

3. **Build the Reviewer Workbench** (#1112)
   - `get_reviewer_workbench()` / `get_reviewer_assignments()` / `get_reviewer_conflicts()` present assigned applications, deadlines, conflicts, rubric progress, submitted reviews
   - Sensitive identity follows blind-review settings (uses `student_anon_id`)
   - Navigation is efficient with deadline sorting
   - Workload counts match assignments

4. **Build the Finance Operations Dashboard** (#1113)
   - `get_finance_ops_dashboard()` / `get_due_payments()` / `get_failed_payments()` / `get_reconciliation_items()` surface funding, liabilities, due payments, failures, reconciliation, refunds, recoveries
   - High-risk actions require confirmation and permissions
   - Filters are shareable
   - Stale financial data is labeled via `last_updated`

### Contract Added

- `contracts/scholarship-dashboards/` - New Soroban contract with:
  - Student dashboard aggregation
  - Sponsor dashboard with program detail
  - Reviewer workbench with assignments, workload, deadlines, conflicts
  - Finance operations dashboard with due payments, failed payments, reconciliation
  - Caching with TTL and global invalidation

### Documentation Added

- `contracts/docs/scholarship-dashboards.md` - Complete contract documentation

### Tests Included

- Unit tests for all dashboard types and authorization
- Authorization boundary tests

### Closes

Closes #1113, #1112, #1111, #1110