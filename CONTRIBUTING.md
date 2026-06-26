# Contributing to chainVerse

Thank you for your interest in contributing to chainVerse! This guide will help you get started and while you are here do well to join our Telegram at [**chainverse**](https://t.me/+nfr3_9fyvDozYzI0).

### Important Note Before Applying 📝

⚠️ **Avoid Generic Comments:** Comments such as 🚫  
"Can I help with this?" 🚫  
"I’d love to contribute!" 🚫  
"Check out my profile!" or 🚫  
"Can I work on this?"... these will not be considered.

Instead, provide a **clear explanation of your approach**, which includes:

- A brief introduction about yourself.
- A concise plan outlining how you will address the issue (3–6 lines max).
- Your estimated completion time (ETA).

## What is chainVerse?

ChainVerse Academy is a decentralized Web3 education platform built on the Stellar blockchain. It offers crypto-based payments, NFT certifications, and DAO governance, allowing students to learn about multiple blockchain ecosystems, earn rewards, and own their learning assets through secure, low-cost transactions.

## chainVerse Academy - Key Points

- Enable crypto-based course purchases and seamless Web3 wallet integration (e.g., Metamask, WalletConnect)
- Provide an instructor dashboard for uploading courses, setting crypto prices, and tracking student engagements
- Facilitate live learning sessions and 1-on-1 mentorship with smart contract-backed payments
- Conduct exams and assignments on-chain, offering crypto rewards for top-performing students
- Issue verifiable NFT certificates upon course completion, stored securely on the blockchain
- Allow users to transfer or resell courses through a smart contract-driven ownership model
- Implement a decentralized reputation system and DAO governance for community-led platform improvements

How to Contribute🤝

## Pull Request Template

To ensure consistency and improve the review process, we've implemented a PR template. When creating a pull request, please:

1. Follow the PR template that automatically loads when you create a new PR.
2. Fill out all relevant sections of the template.
3. Ensure your PR description clearly communicates the changes you've made.
4. Include screenshots or recordings when applicable.
5. Link to any related issues using keywords like "Closes #123" or "Fixes #123".

The template location is at `.github/PULL_REQUEST_TEMPLATE.md` and provides a structured format to help maintainers understand and review your contribution more efficiently.

## Steps to apply

1. Apply for an issue.
   - Look for an open issue and comment expressing your interest in working on it.
2. Wait for the maintainer to assign the issue to you.
3. Remember to apply only if you can solve the issue.
4. In the comment, add a quick introduction about yourself, the ETA, and how you plan to tackle the issue.

## Setup Instructions

1. Fork the repository.

2. Install the required Rust target for building WASM contracts:

   ```bash
   rustup target add wasm32-unknown-unknown
   ```