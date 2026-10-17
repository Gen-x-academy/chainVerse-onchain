# MENTOR-101: Mentorship Engagement-Scoped Messaging

**Track:** Communication | **Component:** on-chain | **Status:** design

Goal: Create engagement-scoped messaging.

## On-chain

A message index: engagement id, sender hash, message hash, sequence and ledger time.

## Off-chain boundary

Message text and attachments are stored off-chain under the attachment policy.

## Rules

- Only the two participants of an active engagement can post.
- Messages stop being accepted when the engagement ends; retention follows the safety policy.
- A moderation hook can hide a message without deleting its hash.

## Events and errors

Event `message_posted(engagement, seq)`. Errors: `NotParticipant`, `EngagementClosed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-101 work lives in the other ChainVerse repositories.
