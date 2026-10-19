# MENTOR-077: Configure Mentorship Pricing Models

**Track:** Payments | **Component:** on-chain | **Status:** design

Goal: Configure mentorship pricing models.

## On-chain

A pricing model per engagement holding its kind, currency or asset, amount, payer, recipient, platform fee and the version and ledger time it became effective.

## Off-chain boundary

The pricing configuration screen, the currency picker and the fee breakdown shown before acceptance are served by the app.

## Rules

- Pricing kinds are a closed set: free, per-session, milestone, subscription and sponsored. Each states which schedule it settles against.
- Only the mentor may set the price and only the mentee may accept it. Neither party may both set and accept.
- Currency or asset is explicit and is never inferred. A mismatch between the agreed currency and the settled asset is rejected at settlement.
- A pricing model is versioned. A change creates a new version effective from a ledger timestamp rather than mutating the one already accepted.
- An accepted model is frozen. Changing it later requires a fresh agreement.
- Platform fee is a rate recorded with the model, so a later fee change cannot retroactively change what was agreed.
- Amounts are non-negative and are stored in the asset's minor units.

## Events and errors

Events `pricing_model_proposed(agreement, model, version)` and `pricing_model_accepted(agreement, model, version)`. Errors: `NotAuthorized`, `PricingKindInvalid`, `AmountInvalid`, `CurrencyMismatch`, `ModelFrozen`.

## Dependencies and rollout

Ships behind a feature flag. The matching MENTOR-077 work lives in the other ChainVerse repositories.