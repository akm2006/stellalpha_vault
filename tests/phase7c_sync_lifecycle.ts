import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { StellalphaVault } from "../target/types/stellalpha_vault";
import { assert } from "chai";
import { 
  createMint, 
  createAccount, 
  mintTo, 
  getAccount, 
  TOKEN_PROGRAM_ID, 
  ASSOCIATED_TOKEN_PROGRAM_ID, 
  getAssociatedTokenAddressSync
} from "@solana/spl-token";

import * as fs from "fs";
import * as os from "os";

describe("Phase 7C: Sync Lifecycle", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.StellalphaVault as Program<StellalphaVault>;

  // Load wallet explicitly (Payer/Backend authority)
  const walletPath = os.homedir() + "/.config/solana/devnet-wallet.json";
  const rawKey = JSON.parse(fs.readFileSync(walletPath, "utf-8"));
  const backendAuthority = anchor.web3.Keypair.fromSecretKey(Uint8Array.from(rawKey));

  // Ephemeral Vault Owner (User)
  const vaultOwner = anchor.web3.Keypair.generate();
  const trader = anchor.web3.Keypair.generate(); 
  
  let baseMint: anchor.web3.PublicKey;
  let ownerTokenAccount: anchor.web3.PublicKey;
  let vaultPda: anchor.web3.PublicKey;
  let globalConfigPda: anchor.web3.PublicKey;
  let vaultTokenAccount: anchor.web3.PublicKey;
  let traderStatePda: anchor.web3.PublicKey;
  let traderTokenAccount: anchor.web3.PublicKey;

  const FUNDING_AMOUNT = new anchor.BN(1_000_000); // 1.0 USDC

  before(async () => {
    console.log("=".repeat(60));
    console.log("  PHASE 7C: SYNC LIFECYCLE TESTS - SETUP");
    console.log("=".repeat(60));
    console.log("Backend Authority:", backendAuthority.publicKey.toBase58());
    console.log("Vault Owner (User):", vaultOwner.publicKey.toBase58());

    // 0. Fund Vault Owner from Backend
    const transferTx = new anchor.web3.Transaction().add(
        anchor.web3.SystemProgram.transfer({
            fromPubkey: backendAuthority.publicKey,
            toPubkey: vaultOwner.publicKey,
            lamports: 100_000_000 // 0.1 SOL
        })
    );
    await anchor.web3.sendAndConfirmTransaction(provider.connection, transferTx, [backendAuthority]);

    // 1. Setup Base Mint and Owner Tokens
    console.log("Creating Mint...");
    baseMint = await createMint(
      provider.connection,
      backendAuthority,
      backendAuthority.publicKey,
      null,
      6
    );

    ownerTokenAccount = await createAccount(
        provider.connection,
        backendAuthority,
        baseMint,
        vaultOwner.publicKey
    );

    await mintTo(
        provider.connection,
        backendAuthority,
        baseMint,
        ownerTokenAccount,
        backendAuthority.publicKey,
        10_000_000 // 10 USDC
    );

    // 2. GlobalConfig
    [globalConfigPda] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("global_config")],
        program.programId
    );
    
    // Initialize if needed
    try {
      await program.account.globalConfig.fetch(globalConfigPda);
      console.log("GlobalConfig already exists");
    } catch {
      console.log("Initializing GlobalConfig...");
      await program.methods
        .initializeGlobalConfig()
        .accounts({
          globalConfig: globalConfigPda,
          admin: backendAuthority.publicKey,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([backendAuthority])
        .rpc();
    }

    // 3. Initialize UserVault - WITH BACKEND AS AUTHORITY
    [vaultPda] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("user_vault_v1"), vaultOwner.publicKey.toBuffer()],
        program.programId
    );
    console.log("Initializing Vault (authority = backend)...");
    await program.methods
        .initializeVault(backendAuthority.publicKey, baseMint) // Backend is authority!
        .accounts({
            vault: vaultPda,
            owner: vaultOwner.publicKey,
            systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([vaultOwner])
        .rpc();

    // 4. Create Vault ATA and Fund it
    vaultTokenAccount = getAssociatedTokenAddressSync(
        baseMint,
        vaultPda,
        true
    );
    await program.methods
        .initVaultAta()
        .accounts({
            vault: vaultPda,
            owner: vaultOwner.publicKey,
            mint: baseMint,
            vaultTokenAccount: vaultTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([vaultOwner])
        .rpc();
        
    await program.methods
        .depositToken(new anchor.BN(5_000_000))
        .accounts({
            vault: vaultPda,
            owner: vaultOwner.publicKey,
            ownerTokenAccount: ownerTokenAccount,
            vaultTokenAccount: vaultTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([vaultOwner])
        .rpc();
    console.log("Vault created and funded.");

    // 5. Create TraderState (NOT initialized, NOT syncing)
    [traderStatePda] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("trader_state"), vaultOwner.publicKey.toBuffer(), trader.publicKey.toBuffer()],
        program.programId
    );
    
    traderTokenAccount = getAssociatedTokenAddressSync(
        baseMint,
        traderStatePda,
        true
    );

    console.log("Creating TraderState...");
    await program.methods
        .createTraderState(FUNDING_AMOUNT)
        .accounts({
            owner: vaultOwner.publicKey,
            trader: trader.publicKey,
            vault: vaultPda,
            traderState: traderStatePda,
            vaultTokenAccount: vaultTokenAccount,
            traderTokenAccount: traderTokenAccount,
            mint: baseMint,
            systemProgram: anchor.web3.SystemProgram.programId,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        })
        .signers([vaultOwner])
        .rpc();

    // Verify initial state
    const ts = await program.account.traderState.fetch(traderStatePda);
    assert.equal(ts.isInitialized, false, "Should NOT be initialized");
    assert.equal(ts.isSyncing, false, "Should NOT be syncing");
    
    console.log("Setup complete. TraderState ready for sync tests.");
    console.log("=".repeat(60));
  });


  // ============================================================
  // TEST 1: User cannot start sync
  // ============================================================
  it("User cannot start sync (backend-only)", async () => {
    try {
      await program.methods
        .startTraderSync()
        .accounts({
          signer: vaultOwner.publicKey,
          vault: vaultPda,
          traderState: traderStatePda,
        })
        .signers([vaultOwner])
        .rpc();
      assert.fail("Should have failed - user cannot start sync");
    } catch (e: any) {
      assert.include(e.message, "Unauthorized", "Expected Unauthorized error");
      console.log("   ✓ User correctly denied from starting sync");
    }
  });


  // ============================================================
  // TEST 2: Backend can start sync
  // ============================================================
  it("Backend can start sync", async () => {
    await program.methods
      .startTraderSync()
      .accounts({
        signer: backendAuthority.publicKey,
        vault: vaultPda,
        traderState: traderStatePda,
      })
      .signers([backendAuthority])
      .rpc();

    const ts = await program.account.traderState.fetch(traderStatePda);
    assert.equal(ts.isSyncing, true, "is_syncing should be true");
    assert.equal(ts.isInitialized, false, "is_initialized should still be false");
    console.log("   ✓ Backend started sync phase");
  });


  // ============================================================
  // TEST 3: Cannot start sync twice
  // ============================================================
  it("Cannot start sync twice (AlreadySyncing)", async () => {
    try {
      await program.methods
        .startTraderSync()
        .accounts({
          signer: backendAuthority.publicKey,
          vault: vaultPda,
          traderState: traderStatePda,
        })
        .signers([backendAuthority])
        .rpc();
      assert.fail("Should have failed - already syncing");
    } catch (e: any) {
      assert.include(e.message, "AlreadySyncing", "Expected AlreadySyncing error");
      console.log("   ✓ Correctly prevented double sync start");
    }
  });


  // ============================================================
  // TEST 4: Backend can finish sync
  // ============================================================
  it("Backend can finish sync", async () => {
    await program.methods
      .finishTraderSync()
      .accounts({
        signer: backendAuthority.publicKey,
        vault: vaultPda,
        traderState: traderStatePda,
      })
      .signers([backendAuthority])
      .rpc();

    const ts = await program.account.traderState.fetch(traderStatePda);
    assert.equal(ts.isSyncing, false, "is_syncing should be false");
    assert.equal(ts.isInitialized, true, "is_initialized should be true");
    console.log("   ✓ Backend finished sync, now initialized");
  });


  // ============================================================
  // TEST 5: Cannot re-enter sync after initialization
  // ============================================================
  it("Cannot re-enter sync after initialization (AlreadyInitialized)", async () => {
    try {
      await program.methods
        .startTraderSync()
        .accounts({
          signer: backendAuthority.publicKey,
          vault: vaultPda,
          traderState: traderStatePda,
        })
        .signers([backendAuthority])
        .rpc();
      assert.fail("Should have failed - already initialized");
    } catch (e: any) {
      assert.include(e.message, "AlreadyInitialized", "Expected AlreadyInitialized error");
      console.log("   ✓ Correctly blocked re-entry into sync phase");
    }
  });


  // ============================================================
  // TEST 6: User cannot finish sync (backend-only)
  // ============================================================
  it("User cannot finish sync (already tested via start, but confirming)", async () => {
    // Create a new TraderState to test finish without starting
    const trader2 = anchor.web3.Keypair.generate();
    const [traderStatePda2] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("trader_state"), vaultOwner.publicKey.toBuffer(), trader2.publicKey.toBuffer()],
        program.programId
    );
    const traderTokenAccount2 = getAssociatedTokenAddressSync(
        baseMint,
        traderStatePda2,
        true
    );

    // Create new TraderState
    await program.methods
        .createTraderState(new anchor.BN(100_000))
        .accounts({
            owner: vaultOwner.publicKey,
            trader: trader2.publicKey,
            vault: vaultPda,
            traderState: traderStatePda2,
            vaultTokenAccount: vaultTokenAccount,
            traderTokenAccount: traderTokenAccount2,
            mint: baseMint,
            systemProgram: anchor.web3.SystemProgram.programId,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        })
        .signers([vaultOwner])
        .rpc();

    // Backend starts sync
    await program.methods
      .startTraderSync()
      .accounts({
        signer: backendAuthority.publicKey,
        vault: vaultPda,
        traderState: traderStatePda2,
      })
      .signers([backendAuthority])
      .rpc();

    // User tries to finish sync
    try {
      await program.methods
        .finishTraderSync()
        .accounts({
          signer: vaultOwner.publicKey,
          vault: vaultPda,
          traderState: traderStatePda2,
        })
        .signers([vaultOwner])
        .rpc();
      assert.fail("Should have failed - user cannot finish sync");
    } catch (e: any) {
      assert.include(e.message, "Unauthorized", "Expected Unauthorized error");
      console.log("   ✓ User correctly denied from finishing sync");
    }
  });
});
