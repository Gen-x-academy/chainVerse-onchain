# MENTOR-105: Mentorship Activity Inbox

**Track:** Communication | **Component:** on-chain | **Status:** design

Goal: Build mentorship activity inbox.

## On-chain

A read marker per user and category (last read sequence) and an expiry ledger for each inbox item reference.

## Off-chain boundary

Item content, deep links, filters and pagination are served off-chain from events.

## Rules

- Items are listed only for users who still have permission at read time.
- Expired items are dropped from results.
- Marking an item read twice has no extra effect.

## Events and errors

Event `inbox_read(user, category, seq)`. Errors: `NotAuthorized`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-105 work lives in the other ChainVerse repositories.
