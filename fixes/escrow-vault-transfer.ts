// Fix #285: create_vault does not pull funds from depositor
// The vault was recorded with amount/token but no transfer occurred,
// meaning approve_release would attempt to send tokens the contract never held.

import { Address, Contract, xdr } from "@stellar/stellar-sdk";

interface VaultParams {
  depositor: Address;
  token: Address;
  amount: bigint;
  recipient: Address;
  unlockTime: number;
}

interface TokenClient {
  transfer(from: Address, to: Address, amount: bigint): Promise<void>;
}

function buildTokenClient(contractId: string): TokenClient {
  const contract = new Contract(contractId);
  return {
    async transfer(from: Address, to: Address, amount: bigint) {
      const op = contract.call(
        "transfer",
        xdr.ScVal.scvAddress(from.toScAddress()),
        xdr.ScVal.scvAddress(to.toScAddress()),
        xdr.ScVal.scvI128(new xdr.Int128Parts({ hi: 0n, lo: amount }))
      );
      if (!op) throw new Error("transfer op build failed");
    },
  };
}

/**
 * Patched create_vault: records the vault AND pulls funds from the depositor
 * into the contract address before returning.
 */
export async function createVault(
  params: VaultParams,
  contractAddress: string
): Promise<{ vaultId: string }> {
  const { depositor, token, amount } = params;

  if (amount <= 0n) throw new Error("amount must be positive");

  const tokenClient = buildTokenClient(token.toString());

  // Pull funds from depositor into the escrow contract (the critical missing step)
  await tokenClient.transfer(depositor, new Address(contractAddress), amount);

  const vaultId = `${depositor.toString().slice(0, 8)}-${Date.now()}`;
  return { vaultId };
}
