// Fix #288: set_usdc_contract — any signed address can replace the payment token
// require_auth() only proves the caller signed; it never checks they ARE the admin.
// Fix: compare caller against the stored admin address before accepting the change.

import { Address } from "@stellar/stellar-sdk";

// Simulated instance storage
const store = new Map<string, string>();

export function initSubscription(admin: Address, usdcContract: Address): void {
  if (store.has("admin")) throw new Error("already initialized");
  store.set("admin", admin.toString());
  store.set("usdc_contract", usdcContract.toString());
}

/** Reads stored admin and throws if caller does not match. */
function requireAdmin(caller: Address): void {
  const storedAdmin = store.get("admin");
  if (!storedAdmin) throw new Error("admin not set");

  // This is the critical check that was missing — identity, not just auth
  if (caller.toString() !== storedAdmin) {
    throw new Error("unauthorized: caller is not admin");
  }
}

/**
 * Patched set_usdc_contract: verifies the caller IS the stored admin,
 * not merely that they signed the transaction.
 */
export function setUsdcContract(caller: Address, newContract: Address): void {
  // require_auth equivalent — proves caller signed (omitted here, done on-chain)
  requireAdmin(caller); // ← identity check that was missing

  if (!newContract.toString().startsWith("C")) {
    throw new Error("invalid contract address");
  }

  store.set("usdc_contract", newContract.toString());
}

export function getUsdcContract(): string {
  const addr = store.get("usdc_contract");
  if (!addr) throw new Error("usdc contract not set");
  return addr;
}
