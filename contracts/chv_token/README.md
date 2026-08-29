# CHV Token

## Standard token interface

CHV exposes the Soroban token metadata methods `name()`, `symbol()`, and `decimals()`, returning `ChainVerse`, `CHV`, and `7`. Its existing `balance`, `transfer`, `allowance`, `approve`, and `transfer_from` entry points use the standard Soroban token argument order, so wallets and contracts can use the standard `soroban_sdk::token::Client` for those operations. Metadata methods are read-only and emit no events.

## Freeze policy

A frozen account cannot send or receive tokens. `transfer` rejects a frozen sender or recipient, and `transfer_from` rejects a frozen owner, spender, or recipient. A frozen owner or spender cannot create an approval. `allowance` remains a read-only query, while `mint` and administrative freeze operations retain their existing authorization rules.

## Pending admin transfer

`propose_admin(current_admin, new_admin, expires_at)` creates a two-step admin transfer. `expires_at` is an absolute Unix timestamp from the Soroban ledger and must be later than the current ledger timestamp. The proposed admin must call `accept_admin(new_admin)` before expiry. At or after expiry, acceptance returns `AdminTransferExpired`.

The current admin can call `cancel_admin(current_admin)` to remove the pending proposal. A proposal emits `ADM_PROP` with `(current_admin, new_admin, expires_at)`, successful acceptance emits `ADM_NEW` with `(new_admin)`, and cancellation emits `ADM_CANC` with `(current_admin, pending_admin)`.

## Allowance expiration

`approve(owner, spender, amount, expiration_ledger)` stores the amount and its expiration ledger in persistent allowance storage. An `expiration_ledger` of `0` means the allowance does not expire. Once the current ledger sequence is greater than the expiration ledger, `allowance` returns zero and `transfer_from` rejects the approval.

The added `expiration_ledger` argument is a public ABI change. Existing callers must provide `0` for a non-expiring allowance or a future ledger sequence.

This also changes the value stored under each existing allowance key from an integer to an expiration-aware record. An upgraded deployment must migrate or re-create existing approvals before relying on them; callers should re-approve any allowance that was created by the previous implementation.

## Allowance events

Allowance events use stable topics `(event, owner, spender)`. `approval` publishes `(amount, expiration_ledger, remaining)` after `approve`; `allow_dec` publishes the same fields after `transfer_from`; and `allow_rev` publishes them after `revoke_allowance`. Approval sets `remaining` to `amount`, decrements report the consumed `amount` and new `remaining`, and revocations report the removed amount with `remaining` set to zero. The decrement and revocation events use `0` for the expiration field because they do not recreate the approval.

### Upgrade migration

The expiry is stored in a new `PendingAdminExpiry` instance entry; the existing `PendingAdmin` address entry remains unchanged. An already deployed contract upgraded from the previous implementation cannot safely accept an old proposal because it has no expiry. Re-propose the transfer with a future `expires_at` after upgrading. A current admin can cancel an old pending proposal before re-proposing it.
