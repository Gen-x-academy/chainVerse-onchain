#!/usr/bin/env bash
set -euo pipefail

PUBKEY=$(stellar keys address deployer --network testnet)
curl -s "https://friendbot.stellar.org?addr=$PUBKEY" > /dev/null
echo "Account balance topped up"
