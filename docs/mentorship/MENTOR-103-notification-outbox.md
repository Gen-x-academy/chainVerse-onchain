# MENTOR-103: Mentorship Lifecycle Notifications

**Track:** Communication | **Component:** on-chain | **Status:** design

Goal: Send lifecycle notifications reliably.

## On-chain

An outbox entry: template id, locale, recipient hash, dedupe key and attempt count.

## Off-chain boundary

Rendering, delivery and dead-letter review are done by the off-chain notifier.

## Rules

- The same dedupe key never creates a second entry.
- Failed sends are retried a limited number of times, then moved to dead letter.
- Message text contains no sensitive data.

## Events and errors

Event `outbox_queued(entry, template)`. Errors: `DuplicateKey`, `NotAuthorized`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-103 work lives in the other ChainVerse repositories.
