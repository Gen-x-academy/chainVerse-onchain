# MENTOR-076: Generate Completion Summary

**Track:** Goals | **Component:** on-chain | **Status:** design

Goal: Generate completion summary.

## On-chain

A closure snapshot per engagement holding the agreed goals, attendance totals, accepted deliverable ids, feedback state, achievement ids and any outstanding obligations, each with the value it held at the moment of closure.

## Off-chain boundary

The summary document, its formatting and the narrative written around the figures are served by the app.

## Rules

- A snapshot is taken once, at closure, and is immutable afterwards. Later amendments to a goal do not rewrite a summary already issued.
- Every figure is read from the record it summarises and copied in, so the summary stays accurate even if the underlying record is later corrected under dispute.
- Outstanding obligations are listed explicitly. A summary is not issued while any obligation is unresolved unless a coordinator records why.
- Feedback state is summarised as submitted, revealed or not requested - never as the feedback content itself.
- A snapshot is readable by both participants and by a coordinator; it contains nothing private to either one.
- Issuing a snapshot is idempotent. A retry returns the same snapshot rather than creating a second one.

## Events and errors

Events `completion_summary_generated(agreement, summary)` and `closure_recorded(agreement, obligations_open)`. Errors: `NotAuthorized`, `AgreementClosed`, `ObligationsOpen`, `AlreadySummarised`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-076 work lives in the other ChainVerse repositories.