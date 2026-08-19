# ChainVerse Academy — On-chain MVP

ChainVerse Academy is a Web3 education platform. This repository contains the
Solidity contracts for its first on-chain milestone, targeting World Chain.

## MVP scope

- Instructor-created courses with admin approval.
- Native-token course purchases.
- Pull-based instructor earnings and platform fees.
- Transferable enrollment before course completion.
- Admin/verifier-authorized course completion.
- Soulbound ERC-721 completion certificates.
- Capped ERC-20 completion rewards.

The frontend, wallet authentication, course media/IPFS services, exams, live
sessions, subscriptions, reputation, DAO governance, resale markets, and
production deployment automation are intentionally deferred until this core is
stable.

## Contracts

- `ChainVerseCourseRegistry`: course ownership, metadata, pricing, and approval.
- `ChainVerseMarketplace`: enrollment ownership, purchases, fees, and completion.
- `ChainVerseCertificate`: non-transferable course-completion NFT.
- `ChainVerseRewardToken`: capped token minted by authorized reward contracts.

## Local development

Use Node.js 20 or 22.

```sh
npm install
npm run build
npm test
```

Copy `.env.example` to `.env` before configuring a network deployment. Local
tests do not require environment variables.

## Deployment roles

After deploying the contracts, the deployment script grants the marketplace:

- `MINTER_ROLE` on the certificate contract.
- `REWARDER_ROLE` on the reward token.

The deployment administrator initially holds course approval, completion
verification, treasury management, and role-administration permissions. These
roles should move to multisig or governance accounts before a production launch.

## Security status

This is an unaudited MVP. Do not deploy it with production funds until its
requirements, invariant tests, network configuration, and independent audit are
complete.
