# Stellalpha Vault

Stellalpha Vault is a non-custodial copy trading smart contract built on Solana. It enables users to mirror the trades of "Star Traders" automatically, securely, and without holding any native SOL for network fees.

## Features

- **Non-Custodial Architecture:** All deposited funds are held in Program Derived Addresses (PDAs) mathematically owned by the user. The protocol operator cannot withdraw or seize user funds.
- **Gasless User Experience:** The vault operates entirely on SPL tokens. All network fees, rent, and Jupiter routing costs are covered by the Backend Authority via Jito bundles, ensuring the user experiences zero friction.
- **Dynamic Trade Execution:** Integrates directly with Jupiter Aggregator v6 to facilitate optimal token swaps. Trade slippage is dynamically calculated off-chain and enforced on-chain as a hard mathematical floor (`min_amount_out`) to prevent MEV sandwich attacks.
- **Provable Accounting:** Separates capital across dedicated `UserVault` (deposits) and `TraderState` (active execution) accounts to prevent cross-contamination and ensure accurate per-allocation accounting.

## Technology Stack

- **Framework:** Anchor (v0.29.0)
- **Language:** Rust
- **Runtime Environment:** Solana Mainnet / Devnet
- **Execution Engine:** Jupiter v6 CPI & Jito Block Engine (Off-chain)

## Local Development

Ensure you have the Solana CLI and Anchor framework installed.

### Build the Program

```bash
anchor build
```

This will compile the Rust smart contract into a deployable BPF binary.

### Run the Test Suite

The repository contains a comprehensive suite of E2E Typescript tests validating the entire vault lifecycle, math precision, and security invariants.

```bash
# Run the localnet validator and execute all tests
anchor test
```

## Security

The smart contract utilizes strict validations to guarantee fund security:
- **ATA Ownership:** Input and Output token accounts during a copy-trade are strictly validated to belong to the user's PDA. The backend cannot redirect funds to an unauthorized wallet.
- **SafeMath Integration:** All platform fee deductions (0.1%) use Rust's `checked_math` macros to mathematically prevent integer overflow/underflow exploits.
- **Slippage Enforcement:** A mandatory `min_amount_out` parameter must be provided by the execution agent during any swap, with an on-chain verification forcing a transaction revert if the amount received from Jupiter falls beneath the threshold. 

## License

MIT
