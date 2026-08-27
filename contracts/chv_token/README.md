# CHV Token

## Pending admin transfer

`propose_admin(current_admin, new_admin, expires_at)` creates a two-step admin transfer. `expires_at` is an absolute Unix timestamp from the Soroban ledger and must be later than the current ledger timestamp. The proposed admin must call `accept_admin(new_admin)` before expiry. At or after expiry, acceptance returns `AdminTransferExpired`.

The current admin can call `cancel_admin(current_admin)` to remove the pending proposal. A proposal emits `ADM_PROP` with `(current_admin, new_admin, expires_at)`, successful acceptance emits `ADM_NEW` with `(new_admin)`, and cancellation emits `ADM_CANC` with `(current_admin, pending_admin)`.

## Allowance events

Allowance events use stable topics `(event, owner, spender)`. `approval` publishes `(amount, expiry, remaining)` after `approve`; `allow_dec` publishes the same fields after `transfer_from`; and `allow_rev` publishes them after `revoke_allowance`. CHV allowances currently have no expiry, so `expiry` is always `None`. Approval sets `remaining` to `amount`, decrements report the consumed `amount` and new `remaining`, and revocations report the removed amount with `remaining` set to zero.

### Upgrade migration

The expiry is stored in a new `PendingAdminExpiry` instance entry; the existing `PendingAdmin` address entry remains unchanged. An already deployed contract upgraded from the previous implementation cannot safely accept an old proposal because it has no expiry. Re-propose the transfer with a future `expires_at` after upgrading. A current admin can cancel an old pending proposal before re-proposing it.
