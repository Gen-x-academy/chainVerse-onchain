// Fix for issue #292
// transfer() did not guard against from == to. Because the balance is read,
// decremented, then re-read (already decremented) and incremented, a self-transfer
// causes the sender to lose tokens with no matching credit.

import { Address, Env } from "@stellar/stellar-sdk";

function getBalance(env: Env, account: Address): bigint {
  return env.storage().persistent().get<bigint>(`BAL:${account}`) ?? 0n;
}

function setBalance(env: Env, account: Address, amount: bigint): void {
  if (amount < 0n) throw new Error("Balance cannot be negative");
  env.storage().persistent().set(`BAL:${account}`, amount);
}

/**
 * Transfer `amount` tokens from `from` to `to`.
 *
 * Rejects self-transfers to prevent balance corruption (fixes #292).
 */
export function transfer(
  env: Env,
  from: Address,
  to: Address,
  amount: bigint
): void {
  if (from.toString() === to.toString()) {
    throw new Error("Self-transfer not allowed"); // ← fixes #292
  }
  if (amount <= 0n) throw new Error("Amount must be positive");

  from.requireAuth();

  const fromBalance = getBalance(env, from);
  if (fromBalance < amount) throw new Error("Insufficient balance");

  setBalance(env, from, fromBalance - amount);
  setBalance(env, to, getBalance(env, to) + amount);
}

export function mint(env: Env, to: Address, amount: bigint): void {
  if (amount <= 0n) throw new Error("Amount must be positive");
  setBalance(env, to, getBalance(env, to) + amount);
}

export function balanceOf(env: Env, account: Address): bigint {
  return getBalance(env, account);
}
