# MENTOR-090: Mentorship Dispute Remedies

**Track:** Safety | **Component:** on-chain | **Status:** design

Goal: Resolve disputes and execute remedies.

## On-chain

A resolution record: dispute id, remedy (dismiss, refund, partial refund, release, credit, rematch, warning, escalate), amount and approvals count.

## Off-chain boundary

Reasoning and evidence review stay off-chain.

## Rules

- A remedy runs only after the required number of approvals.
- Money-moving remedies cannot exceed the escrowed amount.
- A resolved dispute cannot be resolved again.

## Events and errors

Event `dispute_resolved(dispute, remedy, amount)`. Errors: `NotEnoughApprovals`, `AlreadyProcessed`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-090 work lives in the other ChainVerse repositories.
