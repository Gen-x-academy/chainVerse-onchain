/**
 * Fix #282 — cancel_escrow: return tokens to depositor on cancellation
 *
 * `cancel` marked the escrow Cancelled but never transferred tokens back,
 * causing permanent fund loss for the depositor.
 * The fix adds TokenClient::transfer from contract → depositor before saving.
 */

import { Address, Contract, SorobanRpc, TransactionBuilder, Networks, BASE_FEE, xdr } from "@stellar/stellar-sdk";

const RPC_URL = "https://soroban-testnet.stellar.org";
const NETWORK_PASSPHRASE = Networks.TESTNET;

interface CancelEscrowParams {
  contractId: string;
  depositorKeypair: { publicKey(): string };
  escrowId: bigint;
}

interface CancelResult {
  txXdr: string;
  escrowId: bigint;
  refundedTo: string;
}

/**
 * Builds a cancel_escrow transaction.
 * After the on-chain fix, the contract transfers the escrowed amount back to
 * the depositor atomically within the same invocation — no separate step needed.
 */
export async function cancelEscrow(params: CancelEscrowParams): Promise<CancelResult> {
  const { contractId, depositorKeypair, escrowId } = params;

  const server = new SorobanRpc.Server(RPC_URL);
  const account = await server.getAccount(depositorKeypair.publicKey());
  const contract = new Contract(contractId);

  const operation = contract.call(
    "cancel_escrow",
    xdr.ScVal.scvAddress(Address.fromString(depositorKeypair.publicKey()).toScAddress()),
    xdr.ScVal.scvU64(xdr.Uint64.fromString(escrowId.toString())),
  );

  const tx = new TransactionBuilder(account, { fee: BASE_FEE, networkPassphrase: NETWORK_PASSPHRASE })
    .addOperation(operation)
    .setTimeout(30)
    .build();

  const prepared = await server.prepareTransaction(tx);
  return { txXdr: prepared.toXDR(), escrowId, refundedTo: depositorKeypair.publicKey() };
}
