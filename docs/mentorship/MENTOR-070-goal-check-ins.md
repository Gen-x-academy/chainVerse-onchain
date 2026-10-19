# MENTOR-070: Track Goal Progress Check-ins

**Track:** Goals | **Component:** on-chain | **Status:** design

Goal: Track goal progress check-ins.

## On-chain

A check-in per goal holding the reported status, the confidence level, the ledger time, the evidence hash references and the next action, appended in order and never overwritten.

## Off-chain boundary

The check-in form, blocker text and the progress chart across check-ins are served by the app.

## Rules

- Only the goal owner may file a check-in. The other participant may read them but never file one.
- A check-in is appended, never amended. A correction is a new check-in that supersedes an earlier one by reference.
- Status and confidence are enum values, so progress cannot be recorded as free text that later proves unparseable.
- Evidence is committed as hashes. Content, attachments and screenshots stay off-chain.
- A check-in against a closed goal is rejected.
- Check-in cadence is enforced as a minimum interval, so progress cannot be backdated in bulk to manufacture an apparent trajectory.
- The latest check-in determines the goal's current status; earlier ones remain readable as history.

## Events and errors

Event `goal_checked_in(goal, checkin, status, confidence)`. Errors: `NotAuthorized`, `GoalClosed`, `CadenceTooShort`, `CheckInInvalid`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-070 work lives in the other ChainVerse repositories.