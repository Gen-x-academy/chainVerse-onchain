## Summary

This PR implements the scholarship discovery and notifications contract addressing four discovery and communications issues:

### Implemented Features

1. **Create Scholarship Catalog Search and Filters** (#1114)
   - `search_programs()` with filters, sorting, cursor-based pagination
   - Filters are URL-backed, counts reflect query, closed/private excluded
   - Pagination stable via opaque cursor
   - Filter options endpoint for UI

2. **Add Personalized Scholarship Matching** (#1115)
   - `update_student_profile()` / `get_recommendations()` / `dismiss_recommendation()`
   - Recommendations explain reasons, support cold starts and dismissal
   - Protected traits excluded from ranking unless legally justified
   - Cold start support for new students

3. **Generate Shareable Public Program Pages** (#1116)
   - `generate_program_page()` / `get_program_page()` with canonical URLs
   - Only published fields appear; revisions update safely
   - Private/invitation-only programs cannot leak (is_private flag)
   - JSON-LD structured data for SEO

4. **Create Scholarship Notification Events** (#1117)
   - `emit_notification()` / `get_notification_event()` for off-chain delivery
   - Events are idempotent (deduplication_key), carry stable references
   - Minimal data for channels (title, body, reference_id, reference_type)
   - Priority levels (Low, Normal, High, Critical)
   - 18 event types covering all scholarship lifecycle events

### Contract Added

- `contracts/scholarship-discovery-notifications/` - New Soroban contract with:
  - Catalog search with filters, sorting, pagination
  - Personalized matching with explainable recommendations
  - Public program pages with JSON-LD structured data
  - Notification event emission for off-chain delivery

### Documentation Added

- `contracts/docs/scholarship-discovery-notifications.md` - Complete contract documentation

### Tests Included

- Unit tests for search, filtering, pagination
- Student profile and recommendation flows
- Notification event emission with deduplication
- Authorization boundary tests

### Closes

Closes #1117, #1116, #1115, #1114