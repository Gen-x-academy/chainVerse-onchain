// Fix for issue #289
// create_tier, update_tier, deactivate_tier called admin.require_auth() without
// verifying the caller matches the stored admin, so any self-authenticating address
// could manage subscription tiers.

import { Address, Env } from "@stellar/stellar-sdk";

interface Tier {
  id: string;
  name: string;
  price: bigint;
  active: boolean;
}

function requireStoredAdmin(env: Env, caller: Address): void {
  const stored = env.storage().persistent().get<string>("ADMIN");
  if (!stored || caller.toString() !== stored) {
    throw new Error("Unauthorised: caller is not the stored admin");
  }
  caller.requireAuth(); // ← replaces bare admin.require_auth() (fixes #289)
}

export function createTier(env: Env, admin: Address, tier: Tier): void {
  requireStoredAdmin(env, admin);
  env.storage().persistent().set(`TIER:${tier.id}`, tier);
}

export function updateTier(
  env: Env,
  admin: Address,
  tierId: string,
  patch: Partial<Tier>
): void {
  requireStoredAdmin(env, admin);
  const existing = env.storage().persistent().get<Tier>(`TIER:${tierId}`);
  if (!existing) throw new Error("Tier not found");
  env.storage().persistent().set(`TIER:${tierId}`, { ...existing, ...patch });
}

export function deactivateTier(
  env: Env,
  admin: Address,
  tierId: string
): void {
  requireStoredAdmin(env, admin);
  const existing = env.storage().persistent().get<Tier>(`TIER:${tierId}`);
  if (!existing) throw new Error("Tier not found");
  env.storage().persistent().set(`TIER:${tierId}`, { ...existing, active: false });
}
