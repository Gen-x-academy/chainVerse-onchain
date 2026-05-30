#!/bin/bash
# Generate a new keypair
stellar keys generate deployer --network testnet
PUBKEY=$(stellar keys address deployer)
echo "Deployer public key: $PUBKEY"
# Fund via Friendbot
curl -s "https://friendbot.stellar.org?addr=$PUBKEY" | jq .
echo "Account funded on testnet"
