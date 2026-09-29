## Summary

This PR implements the scholarship review workflow contract addressing four review workflow issues:

### Implemented Features

1. **Request Additional Applicant Information** (#1085)
   - `request_info()` / `respond_to_info_request()` for bounded questions with deadlines
   - Questions limited to 20 per request
   - Visible fields specify which application data applicant can reference
   - Versioned responses, deadlines enforced on-chain
   - All parties notified via events

2. **Submit and Lock Completed Reviews** (#1084)
   - `submit_review()` / `amend_review()` / `finalize_review()`
   - Submitted reviews immutable; correction uses auditable amendment
   - Aggregate results update atomically
   - Finalized reviews cannot be amended

3. **Save Private Reviewer Score Drafts** (#1083)
   - `save_draft()` / `get_draft()` for private reviewer drafts
   - Draft ownership enforced, autosave conflict-safe
   - Only submitted reviews affect decisions
   - Scores validated against rubric criteria

4. **Create Versioned Scoring Rubrics** (#1082)
   - `create_rubric()` / `update_rubric()` / `activate_rubric()`
   - Weighted criteria, scales, guidance, required comments, disqualifying conditions
   - Weights reconcile (sum = 10000 basis points)
   - Published rubrics immutable; updates create new version
   - Scores identify exact rubric version

### Contract Added

- `contracts/scholarship-review-workflow/` - New Soroban contract with:
  - Versioned scoring rubrics with weighted criteria
  - Private reviewer draft management
  - Review submission, amendment, and finalization
  - Info request/response workflow

### Documentation Added

- `contracts/docs/scholarship-review-workflow.md` - Complete contract documentation

### Tests Included

- Unit tests for rubric creation, validation, versioning
- Draft save/get, review submission, amendment, finalization
- Info request/response workflow
- Authorization boundary tests
- Rubric weight validation tests

### Closes

Closes #1085, #1084, #1083, #1082