// Fix #286: backend_public_key supplied by caller at mint time — not stored on-chain
// Any user could pass their own key, self-sign, and mint without real backend approval.
// Fix: store the trusted key during init and read it from storage at mint time.

import * as nacl from "tweetnacl";

const BACKEND_PUBKEY_KEY = "backend_pubkey";

// Simulated on-chain storage (in Soroban this is env.storage().instance())
const instanceStorage = new Map<string, Uint8Array>();

/** Called once during contract init — stores the trusted backend public key. */
export function init(backendPublicKey: Uint8Array): void {
  if (instanceStorage.has(BACKEND_PUBKEY_KEY)) {
    throw new Error("already initialized");
  }
  if (backendPublicKey.length !== 32) {
    throw new Error("invalid pubkey length");
  }
  instanceStorage.set(BACKEND_PUBKEY_KEY, backendPublicKey);
}

/** Reads the stored backend public key — never trusts caller-supplied value. */
function getBackendPubkey(): Uint8Array {
  const key = instanceStorage.get(BACKEND_PUBKEY_KEY);
  if (!key) throw new Error("backend pubkey not set");
  return key;
}

interface MintParams {
  courseId: string;
  recipient: string;
  proof: Uint8Array; // Ed25519 signature from the real backend
}

/** Patched mint: verifies proof against the on-chain stored pubkey. */
export function mint(params: MintParams): { certificateId: string } {
  const pubkey = getBackendPubkey(); // ← read from storage, not from caller
  const payload = new TextEncoder().encode(
    `${params.courseId}:${params.recipient}`
  );

  const valid = nacl.sign.detached.verify(payload, params.proof, pubkey);
  if (!valid) throw new Error("invalid backend proof");

  return { certificateId: `cert-${params.courseId}-${Date.now()}` };
}
