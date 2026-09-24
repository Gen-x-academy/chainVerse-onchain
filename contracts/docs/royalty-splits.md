# Royalty Splits Contract

- Status: Implemented (bookkeeping only — see Scope)
- Owner: E-Library On-chain / Royalties working group
- Related issue: #993 (distribute licensed-content revenue)

## Scope

This contract (`contracts/royalty-splits`) implements #993's acceptance
criteria directly:

- **Splits total exactly 100 percent.** `create_manifest()` rejects any
  `bps` list that doesn't sum to exactly `TOTAL_BPS = 10_000` (or contains
  a zero share) before the manifest is ever persisted. A manifest is
  immutable once created — there is no "edit split" operation, only
  create-a-new-manifest, so a manifest's percentages can never drift after
  the fact.
- **Rounding is deterministic.** `distribute()` gives every recipient but
  the last `amount * bps[i] / TOTAL_BPS` (integer division), and the last
  recipient gets exactly what's left (`amount` minus every prior share).
  The sum of credited shares always equals `amount` exactly — there is no
  floating point and no dust left unaccounted for, and the same inputs
  always produce the same per-recipient shares.
- **Recipients authenticate withdrawals.** `withdraw()` requires
  `recipient.require_auth()` from the recipient's own address — not the
  admin, not the caller of `distribute`. No one can withdraw on another
  recipient's behalf.
- **Liabilities reconcile per asset.** Every asset (a `Symbol`, e.g.
  `"USDC"`) has its own `AssetLedger { total_distributed, total_withdrawn }`.
  `get_outstanding_liability(asset)` returns
  `total_distributed - total_withdrawn`, which always equals the sum of
  every recipient's outstanding balance in that asset — proven by
  construction, since every credit to a balance is matched by an equal
  credit to `total_distributed`, and every debit (a withdrawal) is matched
  by an equal credit to `total_withdrawn`.

**Deliberately out of scope for this pass:** actual token custody.
`distribute()` credits an internal ledger; it does not pull real tokens
from a caller into escrow, and `withdraw()` returns the withdrawn amount
but does not itself call a token contract to move funds to the recipient.
The accounting invariants above hold regardless — they're pure bookkeeping
correctness — but wiring this contract to real token transfers (most
likely via `soroban_sdk::token::Client`, matching the pattern
`course_registry`/`payment` already use elsewhere in this workspace) is
necessary before this contract can be used against real revenue, and is
recommended as the very next follow-up.

## Privacy

This contract stores only addresses, basis-point shares, and integer
amounts — no content, licensing, or purchaser data. It doesn't know or
care what generated the revenue being split; `distribute()` takes a bare
`amount` and `asset`, with no reference back to which license purchase or
content item it came from (that linkage, if needed for reporting, belongs
to whatever off-chain/indexer layer calls `distribute()`).

## Ownership

A single admin (`initialize`) exclusively creates manifests and triggers
distributions. Recipients have no on-chain say over a manifest's
percentages — only over withdrawing their own accrued balance.

## Migration

New contract, no prior on-chain state.

## Operational impact

- Because manifests are immutable, a publisher/author relationship that
  needs new percentages requires a new `manifest_id` and directing future
  `distribute()` calls there — old balances already credited under the
  prior manifest are unaffected and still withdrawable normally.
- `MAX_RECIPIENTS = 20` bounds both storage and the per-`distribute()` gas
  cost (linear in recipient count); a split needing more parties should be
  modeled as a manifest whose "last recipient" is itself another
  royalty-splits manifest's distribution trigger (a follow-up pattern, not
  implemented here).
- Since this contract holds no real token custody (see Scope), its
  `total_distributed`/`total_withdrawn` figures are *promises*, not
  *proof of funds* — an operator must independently ensure whatever
  process actually moves tokens keeps pace with what this ledger says is
  owed, until the token-transfer follow-up lands.
