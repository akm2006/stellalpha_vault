# 🪩 Stellalpha Vault v1-Core

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Solana](https://img.shields.io/badge/Blockchain-Solana-green)
![Status](https://img.shields.io/badge/Audited-Mainnet%20Ready-success)

Stellalpha Vault is a **Zero-Gas, Non-Custodial, High-Frequency Copy Trading** smart contract built on Solana. It enables users to automatically mirror the trades of "Star Traders" with absolute mathematical security and zero network friction.

## 🌟 Core Architecture

The v1-Core was designed with three uncompromisable pillars:

1.  **Strictly Non-Custodial**
    *   Funds are locked via SPL Token Accounts (ATAs) explicitly owned by user-controlled PDAs.
    *   The `Backend Authority` (which executes the automated copy trades) is mathematically locked out of withdrawal functions. Only the connecting wallet can ever withdraw funds or close the vault.
2.  **Zero-Gas Experience (Jito Abstracted)**
    *   The program is built on a "token-only" architecture. It completely drops legacy Native SOL deposits.
    *   Users never need SOL to cover network fees or rent during copy trading. The backend agent signs as the `fee_payer` and bundles transactions via the Jito Network, ensuring atomic, MEV-protected, invisible execution.
3.  **Dynamic Slippage Protection**
    *   Slippage is not rigidly hardcoded on-chain. The contract delegates slippage calculations to the off-chain execution agent (who computes volatility for memecoins vs. stablecoins) and passes the exact `min_amount_out` into the contract.
    *   The Rust logic acts as an unbreakable mathematical floor, rejecting any Jupiter CPI routing that falls below the threshold, completely preventing sandwich attacks.

## 🏗️ Account Structure

*   **`UserVault` PDA:** The master account holding the user's allocated capital.
*   **`TraderState` PDA:** The child account dedicated to tracking a specific "Star Trader." This isolated environment prevents accounting contamination.
*   **`GlobalConfig` PDA:** Immutable fees. The 0.1% platform extraction is mathematically enforced via `checked_mul` SafeMath and cannot be maliciously inflated post-deployment.

## 🚀 Build and Test

The full suite is continuously tested via localnet Anchor integration:

```bash
# Build the program
anchor build

# Run the 46+ E2E integration tests
anchor test
```

## 🔐 Security Audit

This contract has undergone a rigorous line-by-line Pre-Mainnet Audit verifying:
*   Zero missing `mut` or signer constraints.
*   Zero account memory padding vulnerabilities.
*   Perfectly aligned SPL Token constraints preventing destination-spoofing during Jupiter Swaps.

## License
MIT
