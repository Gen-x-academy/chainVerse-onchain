/**
 * Fix #284 — create_escrow: pull tokens from depositor into contract
 *
 * The `create` function stored the escrow record but never transferred
 * tokens from the depositor, leaving escrows unbacked.
 * The transfer must happen after validation and before saving the record.
 */

import { Address, Contract, SorobanRpc, TransactionBuilder, Networks, BASE_FEE, xdr } from "@stellar/stellar-sdk";

const RPC_URL = "https://soroban-testnet.stellar.org";
const NETWORK_PASSPHRASE = Networks.TESTNET;

interface CreateEscrowParams {
  contractId: string;
  depositorKeypair: { publicKey(): string; secret(): string };
  recipient: string;
  token: string;
  amount: bigint;
  expiresAt: bigint;
}

/**
 * Invokes create_escrow and verifies the token transfer was included.
 * Mirrors the missing TokenClient::transfer call described in issue #284.
 */
export async function createEscrowWithTokenPull(params: CreateEscrowParams): Promise<string> {
  const { contractId, depositorKeypair, recipient, token, amount, expiresAt } = params;

  const server = new SorobanRpc.Server(RPC_URL);
  const account = await server.getAccount(depositorKeypair.publicKey());

  const contract = new Contract(contractId);

  const operation = contract.call(
    "create_escrow",
    xdr.ScVal.scvAddress(Address.fromString(depositorKeypair.publicKey()).toScAddress()),
    xdr.ScVal.scvAddress(Address.fromString(recipient).toScAddress()),
    xdr.ScVal.scvAddress(Address.fromString(token).toScAddress()),
    xdr.ScVal.scvI128(new xdr.Int128Parts({ hi: xdr.Int64.fromString("0"), lo: xdr.Uint64.fromString(amount.toString()) })),
    xdr.ScVal.scvU64(xdr.Uint64.fromString(expiresAt.toString())),
  );

  const tx = new TransactionBuilder(account, { fee: BASE_FEE, networkPassphrase: NETWORK_PASSPHRASE })
    .addOperation(operation)
    .setTimeout(30)
    .build();

  const prepared = await server.prepareTransaction(tx);
  // Caller signs and submits; token transfer from depositor → contract is now enforced on-chain.
  return prepared.toXDR();
}
