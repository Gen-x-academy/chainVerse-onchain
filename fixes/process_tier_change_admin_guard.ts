// Fix for issue #287
// process_tier_change allowed any caller to finalise another user's tier change.
// The TODO placeholder was never enforced; this module adds the missing admin guard.

import { Address, Env } from "@stellar/stellar-sdk";

interface TierChangeRequest {
  user: Address;
  newTier: string;
  requestedAt: number;
}

function getAdmin(env: Env): Address {
  const raw = env.storage().persistent().get<string>("ADMIN");
  if (!raw) throw new Error("Admin not initialised");
  return new Address(raw);
}

function requireAdmin(env: Env, caller: Address): void {
  const admin = getAdmin(env);
  if (caller.toString() !== admin.toString()) {
    throw new Error("Unauthorised: caller is not the stored admin");
  }
  caller.requireAuth();
}

/**
 * Process a pending tier-change request.
 *
 * Rules:
 *  - The user may always process their own request.
 *  - Any other caller must be the stored admin (fixes #287).
 */
export function processTierChange(
  env: Env,
  caller: Address,
  changeRequest: TierChangeRequest
): void {
  caller.requireAuth();

  if (caller.toString() !== changeRequest.user.toString()) {
    // Previously: // TODO: Add admin check here  ← bug
    requireAdmin(env, caller); // ← fix
  }

  applyTierChange(env, changeRequest);
}

function applyTierChange(env: Env, req: TierChangeRequest): void {
  env.storage().persistent().set(`TIER:${req.user}`, req.newTier);
}
