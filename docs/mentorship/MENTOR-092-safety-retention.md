# MENTOR-092: Safety Data Retention and Legal Hold

**Track:** Safety | **Component:** on-chain | **Status:** design

Goal: Define safety data retention and legal hold.

## On-chain

A retention record per data class: delete-after ledger, legal-hold flag and the hash of the hold authority.

## Off-chain boundary

Reports, evidence, messages and recordings are deleted or held off-chain according to the record.

## Rules

- Each category has its own retention period.
- An authorized officer can place and release a legal hold; a held item is never deleted.
- Every access to held data is logged.

## Events and errors

Events `hold_placed` and `hold_released`. Errors: `NotAuthorized`, `OnHold`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-092 work lives in the other ChainVerse repositories.
