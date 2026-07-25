// Fix for issue #291
// whitelist_token in the escrow contract had no authorisation check, allowing
// any account to whitelist a malicious token contract they control.

import { Address, Env } from "@stellar/stellar-sdk";

const WHITELIST_KEY = "TOKEN_WHITELIST";

function getAdmin(env: Env): Address {
  const raw = env.storage().persistent().get<string>("ADMIN");
  if (!raw) throw new Error("Escrow admin not initialised");
  return new Address(raw);
}

/**
 * Whitelist a token so it can be used in escrow deposits.
 *
 * Only the stored admin may call this function (fixes #291).
 * Previously there was no auth check at all.
 */
export function whitelistToken(env: Env, caller: Address, token: Address): void {
  const admin = getAdmin(env);

  if (caller.toString() !== admin.toString()) {
    throw new Error("Unauthorised: only admin can whitelist tokens");
  }
  admin.requireAuth(); // ← the missing guard (fixes #291)

  const list: string[] =
    env.storage().persistent().get<string[]>(WHITELIST_KEY) ?? [];

  if (!list.includes(token.toString())) {
    list.push(token.toString());
    env.storage().persistent().set(WHITELIST_KEY, list);
  }
}

export function isWhitelisted(env: Env, token: Address): boolean {
  const list: string[] =
    env.storage().persistent().get<string[]>(WHITELIST_KEY) ?? [];
  return list.includes(token.toString());
}

export function initEscrow(env: Env, admin: Address): void {
  if (env.storage().persistent().has("ADMIN")) {
    throw new Error("Already initialised");
  }
  env.storage().persistent().set("ADMIN", admin.toString());
}
