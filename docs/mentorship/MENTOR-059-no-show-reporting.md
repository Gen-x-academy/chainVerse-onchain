# MENTOR-059: Handle No-show Reporting

**Track:** Scheduling | **Component:** on-chain | **Status:** design

Goal: Handle no-show reporting.

## On-chain

A no-show report keyed by session and reporting participant, holding the accused role, the ledger time of the report and a hash reference to evidence held off-chain.

## Off-chain boundary

Evidence upload, the dossier reviewers read, and the messaging that notifies the accused are served by the app.

## Rules

- Only a booked participant or a coordinator may report, and only within the configured window after the session ends.
- Reports are keyed by session, so a second report from the same participant merges into the first instead of creating a second record.
- Evidence is committed as a hash; content, location and biometric data never enter the ledger.
- No consequence is applied automatically. The accused is notified and the outcome is set later under policy.
- Either the mentor, the mentee, both, or a technical no-show may be reported, and each is recorded distinctly.

## Events and errors

Event `no_show_reported(session, reporter, accused, evidence_hash)`. Errors: `NotAuthorized`, `NotBooked`, `WindowClosed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-059 work lives in the other ChainVerse repositories.