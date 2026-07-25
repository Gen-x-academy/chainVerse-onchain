// Fix #290: set_royalty has no auth check — anyone can overwrite royalty recipients
// No authorization existed, so any account could redirect royalty payments to themselves.
// Fix: read the stored admin and call require_auth before allowing any royalty update.

import { Address } from "@stellar/stellar-sdk";

interface RoyaltyConfig {
  recipient: Address;
  basisPoints: number; // e.g. 500 = 5%
}

// Simulated instance storage
const store = new Map<string, unknown>();

export function initRoyalty(admin: Address): void {
  if (store.has("admin")) throw new Error("already initialized");
  store.set("admin", admin.toString());
}

/** Reads stored admin; throws if not set or caller doesn't match. */
function requireAdmin(caller: Address): void {
  const admin = store.get("admin") as string | undefined;
  if (!admin) throw new Error("admin not set");
  if (caller.toString() !== admin) throw new Error("unauthorized");
}

/**
 * Patched set_royalty: admin guard added at the top.
 * Previously had zero authorization — any account could call this.
 */
export function setRoyalty(
  caller: Address,
  tokenId: string,
  config: RoyaltyConfig
): void {
  requireAdmin(caller); // ← the missing auth guard

  if (config.basisPoints < 0 || config.basisPoints > 10_000) {
    throw new Error("basis points must be 0–10000");
  }

  store.set(`royalty:${tokenId}`, {
    recipient: config.recipient.toString(),
    basisPoints: config.basisPoints,
  });
}

export function getRoyalty(tokenId: string): RoyaltyConfig | undefined {
  return store.get(`royalty:${tokenId}`) as RoyaltyConfig | undefined;
}
