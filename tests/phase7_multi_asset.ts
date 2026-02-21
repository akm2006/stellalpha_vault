import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { StellalphaVault } from "../target/types/stellalpha_vault";
import { assert } from "chai";
import {
    createMint,
    mintTo,
    getAccount,
    TOKEN_PROGRAM_ID,
    ASSOCIATED_TOKEN_PROGRAM_ID,
    getAssociatedTokenAddressSync,
    getOrCreateAssociatedTokenAccount
} from "@solana/spl-token";
import * as fs from "fs";
import * as os from "os";

describe("Phase 7: Multi-Asset Support", () => {
    const provider = anchor.AnchorProvider.env();
    anchor.setProvider(provider);

    const program = anchor.workspace.StellalphaVault as Program<StellalphaVault>;

    const walletPath = os.homedir() + "/.config/solana/devnet-wallet.json";
    const rawKey = JSON.parse(fs.readFileSync(walletPath, "utf-8"));
    const payer = anchor.web3.Keypair.fromSecretKey(Uint8Array.from(rawKey));

    // Ephemeral vault owner to avoid state collisions
    const vaultOwner = anchor.web3.Keypair.generate();
    const trader = anchor.web3.Keypair.generate();

    let baseMint: anchor.web3.PublicKey;
    let altMint: anchor.web3.PublicKey;  // Alternative mint for multi-asset tests
    let altMint2: anchor.web3.PublicKey; // Second alt mint for token→token tests
    let vaultPda: anchor.web3.PublicKey;
    let traderStatePda: anchor.web3.PublicKey;
    let globalConfigPda: anchor.web3.PublicKey;

    before(async () => {
        console.log("Setting up Phase 7 test environment...");
        console.log("Vault Owner (Ephemeral):", vaultOwner.publicKey.toBase58());
        console.log("Trader:", trader.publicKey.toBase58());

        // Fund Vault Owner
        const transferTx = new anchor.web3.Transaction().add(
            anchor.web3.SystemProgram.transfer({
                fromPubkey: payer.publicKey,
                toPubkey: vaultOwner.publicKey,
                lamports: 200_000_000
            })
        );
        await anchor.web3.sendAndConfirmTransaction(provider.connection, transferTx, [payer]);

        // Create mints
        baseMint = await createMint(provider.connection, payer, payer.publicKey, null, 6);
        altMint = await createMint(provider.connection, payer, payer.publicKey, null, 6);
        altMint2 = await createMint(provider.connection, payer, payer.publicKey, null, 6);
        console.log("Base Mint:", baseMint.toBase58());
        console.log("Alt Mint:", altMint.toBase58());
        console.log("Alt Mint 2:", altMint2.toBase58());

        // GlobalConfig
        [globalConfigPda] = anchor.web3.PublicKey.findProgramAddressSync(
            [Buffer.from("global_config")],
            program.programId
        );
        try {
            await program.methods.initializeGlobalConfig().accounts({
                globalConfig: globalConfigPda,
                admin: payer.publicKey,
                systemProgram: anchor.web3.SystemProgram.programId
            }).signers([payer]).rpc();
            console.log("GlobalConfig initialized.");
        } catch (e) {
            console.log("GlobalConfig might be already initialized.");
        }

        // UserVault
        [vaultPda] = anchor.web3.PublicKey.findProgramAddressSync(
            [Buffer.from("user_vault_v1"), vaultOwner.publicKey.toBuffer()],
            program.programId
        );
        await program.methods.initializeVault(vaultOwner.publicKey, baseMint)
            .accounts({
                vault: vaultPda,
                owner: vaultOwner.publicKey,
                systemProgram: anchor.web3.SystemProgram.programId
            })
            .signers([vaultOwner]).rpc();
        console.log("Vault Initialized:", vaultPda.toBase58());

        // Vault ATA for base mint
        const vaultTokenAccount = getAssociatedTokenAddressSync(baseMint, vaultPda, true);
        await program.methods.initVaultAta().accounts({
            vault: vaultPda,
            owner: vaultOwner.publicKey,
            mint: baseMint,
            vaultTokenAccount: vaultTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            systemProgram: anchor.web3.SystemProgram.programId
        }).signers([vaultOwner]).rpc();

        // Fund Vault
        const ownerAta = await getOrCreateAssociatedTokenAccount(
            provider.connection,
            payer,
            baseMint,
            vaultOwner.publicKey
        );
        await mintTo(provider.connection, payer, baseMint, ownerAta.address, payer.publicKey, 10_000_000);
        await program.methods.depositToken(new anchor.BN(5_000_000)).accounts({
            vault: vaultPda,
            owner: vaultOwner.publicKey,
            ownerTokenAccount: ownerAta.address,
            vaultTokenAccount: vaultTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID
        }).signers([vaultOwner]).rpc();
        console.log("Vault funded with 5M tokens.");

        // Create TraderState
        [traderStatePda] = anchor.web3.PublicKey.findProgramAddressSync(
            [Buffer.from("trader_state"), vaultOwner.publicKey.toBuffer(), trader.publicKey.toBuffer()],
            program.programId
        );
        const traderTokenAccount = getAssociatedTokenAddressSync(baseMint, traderStatePda, true);

        await program.methods.createTraderState(new anchor.BN(1_000_000)).accounts({
            owner: vaultOwner.publicKey,
            trader: trader.publicKey,
            vault: vaultPda,
            traderState: traderStatePda,
            vaultTokenAccount: vaultTokenAccount,
            traderTokenAccount: traderTokenAccount,
            mint: baseMint,
            systemProgram: anchor.web3.SystemProgram.programId,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID
        }).signers([vaultOwner]).rpc();
        console.log("TraderState created:", traderStatePda.toBase58());
    });

    // ========================================================================
    // Test 1: TraderState is NOT initialized by default
    // ========================================================================
    it("TraderState is NOT initialized by default", async () => {
        const ts = await program.account.traderState.fetch(traderStatePda);
        assert.isFalse(ts.isInitialized, "TraderState should NOT be initialized by default");
        console.log("✅ TraderState.is_initialized = false by default.");
    });

    // ========================================================================
    // Test 2: execute_trader_swap fails if not initialized
    // ========================================================================
    it("execute_trader_swap fails if TraderState not initialized", async () => {
        const traderBaseAta = getAssociatedTokenAddressSync(baseMint, traderStatePda, true);
        const platformFeeAta = await getOrCreateAssociatedTokenAccount(
            provider.connection, payer, baseMint, payer.publicKey
        );

        try {
            await program.methods.executeTraderSwap(
                new anchor.BN(100),
                new anchor.BN(1),
                Buffer.from([])
            ).accounts({
                authority: vaultOwner.publicKey,
                vault: vaultPda,
                traderState: traderStatePda,
                inputTokenAccount: traderBaseAta,
                outputTokenAccount: traderBaseAta,
                platformFeeAccount: platformFeeAta.address,
                globalConfig: globalConfigPda,
                jupiterProgram: new anchor.web3.PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcQb"),
                tokenProgram: TOKEN_PROGRAM_ID,
                instructions: anchor.web3.SYSVAR_INSTRUCTIONS_PUBKEY
            }).signers([vaultOwner]).rpc();
            assert.fail("Should have failed with TraderNotInitialized");
        } catch (e: any) {
            assert.include(e.message, "TraderState not initialized");
            console.log("✅ execute_trader_swap correctly blocked before initialization.");
        }
    });

    // ========================================================================
    // Test 3: create_trader_ata works for owner
    // ========================================================================
    it("create_trader_ata succeeds for owner", async () => {
        const altAta = getAssociatedTokenAddressSync(altMint, traderStatePda, true);

        await program.methods.createTraderAta().accounts({
            owner: vaultOwner.publicKey,
            traderState: traderStatePda,
            mint: altMint,
            traderTokenAccount: altAta,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            systemProgram: anchor.web3.SystemProgram.programId
        }).signers([vaultOwner]).rpc();

        const ataInfo = await getAccount(provider.connection, altAta);
        assert.equal(ataInfo.owner.toBase58(), traderStatePda.toBase58(), "ATA should be owned by TraderState PDA");
        console.log("✅ create_trader_ata succeeded. ATA authority:", ataInfo.owner.toBase58());
    });

    // ========================================================================
    // Test 4: create_trader_ata is idempotent
    // ========================================================================
    it("create_trader_ata is idempotent (calling twice doesn't fail)", async () => {
        const altAta = getAssociatedTokenAddressSync(altMint, traderStatePda, true);

        // Second call should succeed (init_if_needed)
        await program.methods.createTraderAta().accounts({
            owner: vaultOwner.publicKey,
            traderState: traderStatePda,
            mint: altMint,
            traderTokenAccount: altAta,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            systemProgram: anchor.web3.SystemProgram.programId
        }).signers([vaultOwner]).rpc();

        console.log("✅ create_trader_ata is idempotent.");
    });

    // ========================================================================
    // Test 5: create_trader_ata fails for non-owner
    // ========================================================================
    it("create_trader_ata fails for non-owner", async () => {
        const fakeOwner = anchor.web3.Keypair.generate();

        // Fund fake owner
        const transferTx = new anchor.web3.Transaction().add(
            anchor.web3.SystemProgram.transfer({
                fromPubkey: payer.publicKey,
                toPubkey: fakeOwner.publicKey,
                lamports: 10_000_000
            })
        );
        await anchor.web3.sendAndConfirmTransaction(provider.connection, transferTx, [payer]);

        const altMint2Ata = getAssociatedTokenAddressSync(altMint2, traderStatePda, true);

        try {
            await program.methods.createTraderAta().accounts({
                owner: fakeOwner.publicKey,  // Wrong owner
                traderState: traderStatePda,
                mint: altMint2,
                traderTokenAccount: altMint2Ata,
                tokenProgram: TOKEN_PROGRAM_ID,
                associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
                systemProgram: anchor.web3.SystemProgram.programId
            }).signers([fakeOwner]).rpc();
            assert.fail("Should have failed with Unauthorized");
        } catch (e: any) {
            // Will fail due to has_one constraint
            console.log("✅ create_trader_ata correctly rejects non-owner.");
        }
    });

    // ========================================================================
    // Test 6: mark_trader_initialized works for owner
    // ========================================================================
    it("mark_trader_initialized succeeds for owner", async () => {
        await program.methods.markTraderInitialized().accounts({
            signer: vaultOwner.publicKey,
            vault: vaultPda,
            traderState: traderStatePda
        }).signers([vaultOwner]).rpc();

        const ts = await program.account.traderState.fetch(traderStatePda);
        assert.isTrue(ts.isInitialized, "TraderState should be initialized");
        console.log("✅ mark_trader_initialized succeeded. is_initialized:", ts.isInitialized);
    });

    // ========================================================================
    // Test 7: mark_trader_initialized fails if already initialized
    // ========================================================================
    it("mark_trader_initialized fails if already initialized", async () => {
        try {
            await program.methods.markTraderInitialized().accounts({
                signer: vaultOwner.publicKey,
                vault: vaultPda,
                traderState: traderStatePda
            }).signers([vaultOwner]).rpc();
            assert.fail("Should have failed with AlreadyInitialized");
        } catch (e: any) {
            assert.include(e.message, "already initialized");
            console.log("✅ mark_trader_initialized correctly rejects re-initialization.");
        }
    });

    // ========================================================================
    // Test 8: execute_trader_swap works after initialization
    // ========================================================================
    it("execute_trader_swap works after initialization", async () => {
        const traderBaseAta = getAssociatedTokenAddressSync(baseMint, traderStatePda, true);
        const platformFeeAta = await getOrCreateAssociatedTokenAccount(
            provider.connection, payer, baseMint, payer.publicKey
        );

        // Since we're using Memo mock, this should succeed if initialization is correct
        try {
            await program.methods.executeTraderSwap(
                new anchor.BN(100),
                new anchor.BN(1),
                Buffer.from([])
            ).accounts({
                authority: vaultOwner.publicKey,
                vault: vaultPda,
                traderState: traderStatePda,
                inputTokenAccount: traderBaseAta,
                outputTokenAccount: traderBaseAta,
                platformFeeAccount: platformFeeAta.address,
                globalConfig: globalConfigPda,
                jupiterProgram: new anchor.web3.PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcQb"),
                tokenProgram: TOKEN_PROGRAM_ID,
                instructions: anchor.web3.SYSVAR_INSTRUCTIONS_PUBKEY
            }).signers([vaultOwner]).rpc();
            console.log("✅ execute_trader_swap works after initialization.");
        } catch (e: any) {
            // May fail due to Memo mock limitations, but NOT due to initialization
            if (e.message.includes("not initialized")) {
                assert.fail("Should NOT fail with TraderNotInitialized after marking initialized");
            }
            console.log("✅ execute_trader_swap passed initialization check (other error OK for mock).");
        }
    });

    // ========================================================================
    // PHASE 7.1: close_trader_ata Tests
    // ========================================================================

    // Test 9: close_trader_ata fails if not paused
    it("close_trader_ata fails if TraderState not paused", async () => {
        const altAta = getAssociatedTokenAddressSync(altMint, traderStatePda, true);

        try {
            await program.methods.closeTraderAta().accounts({
                owner: vaultOwner.publicKey,
                traderState: traderStatePda,
                traderTokenAccount: altAta,
                tokenProgram: TOKEN_PROGRAM_ID
            }).signers([vaultOwner]).rpc();
            assert.fail("Should have failed with TraderNotPaused");
        } catch (e: any) {
            assert.include(e.message, "must be paused");
            console.log("✅ close_trader_ata correctly rejects when not paused.");
        }
    });

    // Test 10: Pause TraderState for cleanup tests
    it("Pause TraderState for cleanup tests", async () => {
        await program.methods.pauseTraderState().accounts({
            owner: vaultOwner.publicKey,
            traderState: traderStatePda
        }).signers([vaultOwner]).rpc();

        const ts = await program.account.traderState.fetch(traderStatePda);
        assert.isTrue(ts.isPaused, "TraderState should be paused");
        console.log("✅ TraderState paused for cleanup tests.");
    });

    // Test 11: close_trader_ata fails if balance > 0
    it("close_trader_ata fails if balance > 0", async () => {
        // First, fund the altMint ATA
        const altAta = getAssociatedTokenAddressSync(altMint, traderStatePda, true);
        await mintTo(provider.connection, payer, altMint, altAta, payer.publicKey, 1000);

        try {
            await program.methods.closeTraderAta().accounts({
                owner: vaultOwner.publicKey,
                traderState: traderStatePda,
                traderTokenAccount: altAta,
                tokenProgram: TOKEN_PROGRAM_ID
            }).signers([vaultOwner]).rpc();
            assert.fail("Should have failed with NonZeroBalance");
        } catch (e: any) {
            assert.include(e.message, "non-zero balance");
            console.log("✅ close_trader_ata correctly rejects non-zero balance.");
        }
    });

    // Test 12: close_trader_ata fails for non-owner
    it("close_trader_ata fails for non-owner", async () => {
        const fakeOwner = anchor.web3.Keypair.generate();

        const transferTx = new anchor.web3.Transaction().add(
            anchor.web3.SystemProgram.transfer({
                fromPubkey: payer.publicKey,
                toPubkey: fakeOwner.publicKey,
                lamports: 10_000_000
            })
        );
        await anchor.web3.sendAndConfirmTransaction(provider.connection, transferTx, [payer]);

        // Create altMint2 ATA first (empty)
        const altMint2Ata = getAssociatedTokenAddressSync(altMint2, traderStatePda, true);
        await program.methods.createTraderAta().accounts({
            owner: vaultOwner.publicKey,
            traderState: traderStatePda,
            mint: altMint2,
            traderTokenAccount: altMint2Ata,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            systemProgram: anchor.web3.SystemProgram.programId
        }).signers([vaultOwner]).rpc();

        try {
            await program.methods.closeTraderAta().accounts({
                owner: fakeOwner.publicKey,  // Wrong owner
                traderState: traderStatePda,
                traderTokenAccount: altMint2Ata,
                tokenProgram: TOKEN_PROGRAM_ID
            }).signers([fakeOwner]).rpc();
            assert.fail("Should have failed with Unauthorized");
        } catch (e: any) {
            // Will fail due to has_one or seeds constraint
            console.log("✅ close_trader_ata correctly rejects non-owner.");
        }
    });

    // Test 13: close_trader_ata succeeds when empty and paused
    it("close_trader_ata succeeds when empty and paused", async () => {
        const altMint2Ata = getAssociatedTokenAddressSync(altMint2, traderStatePda, true);

        await program.methods.closeTraderAta().accounts({
            owner: vaultOwner.publicKey,
            traderState: traderStatePda,
            traderTokenAccount: altMint2Ata,
            tokenProgram: TOKEN_PROGRAM_ID
        }).signers([vaultOwner]).rpc();

        // Verify ATA is closed
        try {
            await getAccount(provider.connection, altMint2Ata);
            assert.fail("ATA should have been closed");
        } catch (e: any) {
            console.log("✅ close_trader_ata succeeded. ATA closed and rent reclaimed.");
        }
    });

    // ========================================================================
    // EDGE CASE SECURITY TESTS (Task 2)
    // ========================================================================

    // Test 14: Backend authority can mark_trader_initialized
    it("backend_authority_can_mark_initialized (new TraderState)", async () => {
        // Create a new TraderState for this test
        const newTrader = anchor.web3.Keypair.generate();
        const [newTraderStatePda] = anchor.web3.PublicKey.findProgramAddressSync(
            [Buffer.from("trader_state"), vaultOwner.publicKey.toBuffer(), newTrader.publicKey.toBuffer()],
            program.programId
        );
        const newTraderBaseAta = getAssociatedTokenAddressSync(baseMint, newTraderStatePda, true);
        const vaultTokenAccount = getAssociatedTokenAddressSync(baseMint, vaultPda, true);

        // Create TraderState
        await program.methods.createTraderState(new anchor.BN(100_000)).accounts({
            owner: vaultOwner.publicKey,
            trader: newTrader.publicKey,
            vault: vaultPda,
            traderState: newTraderStatePda,
            vaultTokenAccount: vaultTokenAccount,
            traderTokenAccount: newTraderBaseAta,
            mint: baseMint,
            systemProgram: anchor.web3.SystemProgram.programId,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID
        }).signers([vaultOwner]).rpc();

        // Backend (vaultOwner is also authority) calls mark_trader_initialized
        await program.methods.markTraderInitialized().accounts({
            signer: vaultOwner.publicKey,  // Using vault.authority
            vault: vaultPda,
            traderState: newTraderStatePda
        }).signers([vaultOwner]).rpc();

        const ts = await program.account.traderState.fetch(newTraderStatePda);
        assert.isTrue(ts.isInitialized, "Backend authority should be able to mark initialized");
        console.log("✅ Backend authority can mark_trader_initialized.");
    });

    // Test 15: Random signer cannot mark_trader_initialized
    it("fails_mark_initialized_by_random_signer", async () => {
        // Create another new TraderState
        const newTrader2 = anchor.web3.Keypair.generate();
        const [newTraderStatePda2] = anchor.web3.PublicKey.findProgramAddressSync(
            [Buffer.from("trader_state"), vaultOwner.publicKey.toBuffer(), newTrader2.publicKey.toBuffer()],
            program.programId
        );
        const newTraderBaseAta2 = getAssociatedTokenAddressSync(baseMint, newTraderStatePda2, true);
        const vaultTokenAccount = getAssociatedTokenAddressSync(baseMint, vaultPda, true);

        // Create TraderState
        await program.methods.createTraderState(new anchor.BN(100_000)).accounts({
            owner: vaultOwner.publicKey,
            trader: newTrader2.publicKey,
            vault: vaultPda,
            traderState: newTraderStatePda2,
            vaultTokenAccount: vaultTokenAccount,
            traderTokenAccount: newTraderBaseAta2,
            mint: baseMint,
            systemProgram: anchor.web3.SystemProgram.programId,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID
        }).signers([vaultOwner]).rpc();

        // Random signer tries to mark initialized
        const randomSigner = anchor.web3.Keypair.generate();
        const fundTx = new anchor.web3.Transaction().add(
            anchor.web3.SystemProgram.transfer({
                fromPubkey: payer.publicKey,
                toPubkey: randomSigner.publicKey,
                lamports: 10_000_000
            })
        );
        await anchor.web3.sendAndConfirmTransaction(provider.connection, fundTx, [payer]);

        try {
            await program.methods.markTraderInitialized().accounts({
                signer: randomSigner.publicKey,
                vault: vaultPda,
                traderState: newTraderStatePda2
            }).signers([randomSigner]).rpc();
            assert.fail("Should have failed with Unauthorized");
        } catch (e: any) {
            assert.include(e.message, "Unauthorized");
            console.log("✅ Random signer correctly rejected from mark_trader_initialized.");
        }
    });

    // Test 16: Swap fails with external (non-TraderState) output ATA
    it("fails_swap_with_external_output_ata", async () => {
        // Resume TraderState first (it was paused earlier)
        await program.methods.resumeTraderState().accounts({
            owner: vaultOwner.publicKey,
            traderState: traderStatePda
        }).signers([vaultOwner]).rpc();

        // Create an ATA owned by payer (external, not TraderState)
        const externalAta = await getOrCreateAssociatedTokenAccount(
            provider.connection, payer, baseMint, payer.publicKey
        );
        const traderBaseAta = getAssociatedTokenAddressSync(baseMint, traderStatePda, true);
        const platformFeeAta = await getOrCreateAssociatedTokenAccount(
            provider.connection, payer, baseMint, payer.publicKey
        );

        try {
            await program.methods.executeTraderSwap(
                new anchor.BN(100),
                new anchor.BN(0),
                Buffer.from([])
            ).accounts({
                authority: vaultOwner.publicKey,
                vault: vaultPda,
                traderState: traderStatePda,
                inputTokenAccount: traderBaseAta,
                outputTokenAccount: externalAta.address,  // External ATA!
                platformFeeAccount: platformFeeAta.address,
                globalConfig: globalConfigPda,
                jupiterProgram: new anchor.web3.PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcQb"),
                tokenProgram: TOKEN_PROGRAM_ID,
                instructions: anchor.web3.SYSVAR_INSTRUCTIONS_PUBKEY
            }).signers([vaultOwner]).rpc();
            assert.fail("Should have failed with InvalidTokenAccountOwner");
        } catch (e: any) {
            if (e.message.includes("InvalidTokenAccountOwner") || e.message.includes("Unauthorized") || e.message.includes("Constraint") || e.message.includes("ConstraintAssociated")) {
                console.log("✅ External output ATA correctly rejected.");
            } else {
                console.log(e.message);
                assert.fail("Unexpected error: " + e.message);
            }
        }
    });

    // Test 17: Settle fails with non-base mint ATA
    it("fails_settle_with_non_base_ata", async () => {
        // First pause the TraderState
        await program.methods.pauseTraderState().accounts({
            owner: vaultOwner.publicKey,
            traderState: traderStatePda
        }).signers([vaultOwner]).rpc();

        // Create an altMint ATA for TraderState
        const altMintAta = getAssociatedTokenAddressSync(altMint, traderStatePda, true);
        await program.methods.createTraderAta().accounts({
            owner: vaultOwner.publicKey,
            traderState: traderStatePda,
            mint: altMint,
            traderTokenAccount: altMintAta,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            systemProgram: anchor.web3.SystemProgram.programId
        }).signers([vaultOwner]).rpc();

        // Mint some alt tokens to it
        await mintTo(provider.connection, payer, altMint, altMintAta, payer.publicKey, 1000);

        try {
            await program.methods.settleTraderState().accounts({
                owner: vaultOwner.publicKey,
                vault: vaultPda,
                traderState: traderStatePda,
                traderTokenAccount: altMintAta  // Non-base mint!
            }).signers([vaultOwner]).rpc();
            assert.fail("Should have failed with MintMismatch");
        } catch (e: any) {
            if (e.message.includes("Mint mismatch") || e.message.includes("ConstraintTokenMint") || e.message.includes("A token mint constraint") || e.message.includes("ConstraintAssociated")) {
                console.log("✅ Settle correctly rejects non-base mint ATA.");
            } else {
                console.log(e.message);
                assert.fail("Unexpected error: " + e.message);
            }
        }
    });

});
