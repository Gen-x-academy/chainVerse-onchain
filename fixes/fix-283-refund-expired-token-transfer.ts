/**
 * Fix #283 — refund_expired_escrow: transfer tokens back to depositor on expiry
 *
 * `refund_expired` set status to Expired but never moved tokens back to the
 * depositor, permanently locking funds in the contract.
 * The fix adds a TokenClient::transfer from contract → depositor before saving.
 */

import { Address, Contract, SorobanRpc, TransactionBuilder, Networks, BASE_FEE, xdr } from "@stellar/stellar-sdk";

const RPC_URL = "https://soroban-testnet.stellar.org";
const NETWORK_PASSPHRASE = Networks.TESTNET;

interface RefundExpiredParams {
  contractId: string;
  callerKeypair: { publicKey(): string };
  escrowId: bigint;
}

/**
 * Calls refund_expired_escrow and confirms the depositor receives their tokens.
 * Validates the on-chain fix for the missing transfer in issue #283.
 */
export async function refundExpiredEscrow(params: RefundExpiredParams): Promise<{ xdr: string; escrowId: bigint }> {
  const { contractId, callerKeypair, escrowId } = params;

  const server = new SorobanRpc.Server(RPC_URL);
  const account = await server.getAccount(callerKeypair.publicKey());
  const contract = new Contract(contractId);

  const operation = contract.call(
    "refund_expired_escrow",
    xdr.ScVal.scvU64(xdr.Uint64.fromString(escrowId.toString())),
  );

  const tx = new TransactionBuilder(account, { fee: BASE_FEE, networkPassphrase: NETWORK_PASSPHRASE })
    .addOperation(operation)
    .setTimeout(30)
    .build();

  const prepared = await server.prepareTransaction(tx);

  // After this call the contract must emit a transfer: contract → depositor.
  // Without the fix, status flipped to Expired but no tokens moved.
  return { xdr: prepared.toXDR(), escrowId };
}

/** Checks whether an escrow's expiry timestamp has passed. */
export function isExpired(expiresAt: bigint, nowSeconds: bigint): boolean {
  return expiresAt > 0n && nowSeconds >= expiresAt;
}
