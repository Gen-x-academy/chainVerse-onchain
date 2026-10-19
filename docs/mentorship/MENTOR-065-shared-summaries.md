# MENTOR-065: Create Shared Session Summaries

**Track:** Sessions | **Component:** on-chain | **Status:** design

Goal: Create shared session summaries.

## On-chain

A shared summary per session holding its revision, the hash of the summary content, the list of decision and next-step references, and the confirmation state of each participant.

## Off-chain boundary

The summary editor, formatting and the read view for both participants are served by the app.

## Rules

- Either participant may draft; a summary becomes shared only once both participants have confirmed the same revision.
- Confirmation is per revision. Editing after a confirmation clears both confirmations.
- Decisions and next steps are stored as references so they can be linked to milestones and action items.
- Content is committed as a hash; the summary body lives off-chain.
- Either participant may withdraw confirmation, which returns the summary to draft without deleting it.
- A summary may only be read by the two booked participants.

## Events and errors

Events `session_summary_drafted(session, revision)` and `session_summary_confirmed(session, participant, revision)`. Errors: `NotAuthorized`, `NotBooked`, `RevisionConflict`, `AlreadyConfirmed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-065 work lives in the other ChainVerse repositories.