# Scholarship Disbursements Contract

- Status: Implemented (partial — see Scope)
- Owner: Scholarships On-chain working group
- Related issues: #1096 (idempotent disbursement intents), #1097 (validate
  payout wallet ownership)

## Scope

This contract (`contracts/scholarship-disbursements`) implements two of the
"Scholarships On-chain/Disbursements" epic's behaviors:

- **#1096 — Create idempotent disbursement intents.** `create_intent()`
  turns an eligible award installment into exactly one stable payment
  intent *before* any external execution. The intent key is derived
  deterministically from `(program_id, recipient, installment)`, so:
  - a retry with identical parameters returns the existing intent and
    changes nothing ("retries reconcile existing state");
  - a retry with a different `amount` (or a recipient wallet that no longer
    matches the stored intent) is rejected with `IntentConflict`.
  - `recipient`, `wallet`, `amount`, and `installment` are set once and
    never mutated — there is no update entry point.
  - `record_execution()` moves a `Pending` intent to `Executed` exactly
    once (`AlreadyExecuted` on retry), so the same installment can never be
    paid twice.
- **#1097 — Validate payout wallet ownership.** A recipient configures a
  payout wallet with `set_payout_wallet()` and proves control of it by
  opening a challenge (`open_wallet_challenge`) and returning an ed25519
  signature over the contract-derived, **domain-separated** payload
  (`wallet_challenge_payload`) to `confirm_wallet_challenge`. Challenges
  expire (`ChallengeExpired`) and are single-use (`ChallengeAlreadyUsed`).
  Changing the configured wallet increments a per-recipient `epoch` and
  clears the verified flag, which places every intent created under the
  previous epoch on safe hold *without enumerating them*.

`is_intent_executable()` is the single source of truth for "may this intent
be executed": it is true only while the intent is `Pending` and its captured
wallet is the recipient's currently verified wallet at the same epoch.
`record_execution()` therefore fails with `IntentOnHold` after a wallet
change until a fresh intent is created for the newly-verified wallet.

**Not implemented here:** actually moving tokens. This contract records
intents and their execution status; it does not touch a token client. When
execution is wired to a payment rail, it should be reviewed as its own ADR
extension given the existing `payment` / `payout-automation` / `escrow`
contracts — consistent with ADR 0002's "disbursement / payment execution"
non-goal.

## Privacy

On-chain state only ever contains addresses, a per-wallet ed25519 public
key, integer `amount`/`installment` values, status enums, counters, and
timestamps. No financial account detail, identity document, or evidence is
stored — the contract's job is to bind an already-decided award to a wallet
the recipient provably controls. The signed challenge body commits only to
`(challenge id, recipient, wallet, expiry)`.

## Ownership

The admin (`initialize`) exclusively controls the disbursement-creator
allowlist. A recipient exclusively controls their own wallet binding: only
the recipient can `set_payout_wallet` or `open_wallet_challenge`, and only
a signature from the registered key can complete a challenge. Intents may
be created and executed by the admin or an allowlisted creator, but never
on terms that differ from an existing intent — the immutable-field conflict
rule means a creator cannot silently re-price or re-target an installment.

## Migration

New contract, no prior on-chain state. Setup order: `initialize` (with the
network id, `sha256(network passphrase)`) → `add_creator` (as needed) →
per recipient: `register_wallet_key` → `set_payout_wallet` →
`open_wallet_challenge`/`confirm_wallet_challenge`. An intent cannot be
created for a recipient with no configured wallet (`WalletNotFound`).
`network_id` is fixed at initialization; a contract deployed against the
wrong network id would reject all wallet proofs and should be redeployed
rather than migrated in place.

## Operational impact

- **Wallet change is a safety boundary.** `set_payout_wallet` always clears
  verification and bumps `epoch`. Intents created under the old epoch are
  permanently on hold (their immutable target is the old wallet) unless the
  recipient signs a proof for that same wallet again. Operators should
  expect a wallet change to require *new* intents for future installments,
  and to `cancel_intent` any abandoned old-epoch intents.
- **Domain separation.** Wallet proofs use the shared canonical signing
  envelope (`contracts/shared/src/signing.rs`) with the
  `PayoutWalletProof` message type. The signed digest commits to the
  domain tag, envelope version, network id, this contract's address, the
  message type, and the challenge body — so a proof for one contract,
  network, or purpose cannot be replayed as another.
- **Challenge lifecycle.** Challenges live in persistent storage with the
  sibling contracts' ~1-year TTL bump, but are logically short-lived
  (`expires_at`, capped at `MAX_CHALLENGE_TTL_SECONDS`); an expired or used
  challenge changes no state and can be ignored.
- **Events:** `WALLKEY`, `WALLSET`, `WCHALNGE`, `WALLVERF`, `INTENTNW`,
  `INTENTEX`, `INTENTCN` — enough for an indexer to reconstruct wallet
  history and intent state without on-chain lists.
- **Cross-contract wiring** (a verified `EVPASS` from
  `scholarship-milestones` automatically creating an intent here) is a
  follow-up, consistent with ADR 0002's cross-contract non-goal.
