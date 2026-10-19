# MENTOR-083: Calculate Cancellations and Refunds

**Track:** Payments | **Component:** on-chain | **Status:** design

Goal: Calculate cancellations and refunds.

## On-chain

A calculation per cancelled session holding the snapshotted cancellation policy version, notice given, the refund to the payer, mentor compensation, platform fee treatment and the explanation lines that produced them.

## Off-chain boundary

The refund breakdown shown to both parties, the dispute prompt and any off-rail adjustment workflow are served by the app.

## Rules

- The cancellation policy version is snapshotted at cancellation. A later policy edit cannot change a refund that has already been calculated.
- The calculation is derived, not supplied. Every figure is computed from the snapshotted policy and the notice period, and a caller-supplied refund amount is ignored.
- Each output figure carries the reason line that produced it, so either party can see why the split came out as it did.
- Refund, mentor compensation and fee are calculated with checked arithmetic and the calculation fails outright on overflow rather than returning a partial split.
- A cancellation is calculated exactly once. Recalculating returns the original result unchanged.
- Notice inside the free window refunds in full with no mentor compensation. Notice outside it applies the snapshotted fee.
- A session cancelled after its milestone already settled is excluded from recalculation; settled amounts are never clawed back by this path.

## Events and errors

Event `cancellation_calculated(session, refund, mentor_amount, fee, policy_version)`. Errors: `NotAuthorized`, `SessionNotCancelled`, `AlreadyCalculated`, `PolicyNotFound`, `ArithmeticOverflow`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-083 work lives in the other ChainVerse repositories.