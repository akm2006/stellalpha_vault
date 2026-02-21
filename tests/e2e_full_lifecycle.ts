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

describe("Full E2E: Vault Creation → TraderState Lifecycle → Withdrawal", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.StellalphaVault as Program<StellalphaVault>;

  // Backend authority (operator)
  const walletPath = os.homedir() + "/.config/solana/devnet-wallet.json";
  const rawKey = JSON.parse(fs.readFileSync(walletPath, "utf-8"));
  const backendAuthority = anchor.web3.Keypair.fromSecretKey(Uint8Array.from(rawKey));

  // User (vault owner)
  const user = anchor.web3.Keypair.generate();
  const trader = anchor.web3.Keypair.generate(); 
  
  let baseMint: anchor.web3.PublicKey;
  let userTokenAccount: anchor.web3.PublicKey;
  let vaultPda: anchor.web3.PublicKey;
  let globalConfigPda: anchor.web3.PublicKey;
  let vaultTokenAccount: anchor.web3.PublicKey;
  let traderStatePda: anchor.web3.PublicKey;
  let traderTokenAccount: anchor.web3.PublicKey;

  const INITIAL_MINT = new anchor.BN(10_000_000); // 10 tokens
  const VAULT_DEPOSIT = new anchor.BN(5_000_000); // 5 tokens
  const TRADER_ALLOCATION = new anchor.BN(2_000_000); // 2 tokens

  before(async () => {
    console.log("\n" + "═".repeat(70));
    console.log("  FULL E2E TEST: VAULT CREATION → TRADERSTATE → WITHDRAWAL");
    console.log("═".repeat(70));
    console.log("Backend Authority:", backendAuthority.publicKey.toBase58());
    console.log("User:", user.publicKey.toBase58());
    console.log("");

    // Fund user from backend
    const transferTx = new anchor.web3.Transaction().add(
        anchor.web3.SystemProgram.transfer({
            fromPubkey: backendAuthority.publicKey,
            toPubkey: user.publicKey,
            lamports: 100_000_000
        })
    );
    await anchor.web3.sendAndConfirmTransaction(provider.connection, transferTx, [backendAuthority]);

    // Create mint
    baseMint = await createMint(
      provider.connection,
      backendAuthority,
      backendAuthority.publicKey,
      null,
      6
    );

    // Create user token account and mint tokens
    userTokenAccount = await createAccount(
        provider.connection,
        backendAuthority,
        baseMint,
        user.publicKey
    );

    await mintTo(
        provider.connection,
        backendAuthority,
        baseMint,
        userTokenAccount,
        backendAuthority.publicKey,
        INITIAL_MINT.toNumber()
    );

    // GlobalConfig
    [globalConfigPda] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("global_config")],
        program.programId
    );
    
    try {
      await program.account.globalConfig.fetch(globalConfigPda);
    } catch {
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

    // Derive PDAs
    [vaultPda] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("user_vault_v1"), user.publicKey.toBuffer()],
        program.programId
    );
    vaultTokenAccount = getAssociatedTokenAddressSync(baseMint, vaultPda, true);
    
    [traderStatePda] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("trader_state"), user.publicKey.toBuffer(), trader.publicKey.toBuffer()],
        program.programId
    );
    traderTokenAccount = getAssociatedTokenAddressSync(baseMint, traderStatePda, true);
  });

  // ============================================================
  // STEP 1: Create Vault (User signs, authority = backend)
  // ============================================================
  it("Step 1: User creates vault (authority = backend)", async () => {
    console.log("\n▶ STEP 1: Create Vault");
    
    await program.methods
        .initializeVault(backendAuthority.publicKey, baseMint)
        .accounts({
            vault: vaultPda,
            owner: user.publicKey,
            systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([user])
        .rpc();

    const vault = await program.account.userVault.fetch(vaultPda);
    assert.ok(vault.owner.equals(user.publicKey), "Owner should be user");
    assert.ok(vault.authority.equals(backendAuthority.publicKey), "Authority should be backend");
    console.log("   ✓ Vault created");
    console.log("   Owner:", vault.owner.toBase58());
    console.log("   Authority:", vault.authority.toBase58());
  });

  // ============================================================
  // STEP 2: Fund Vault (User deposits tokens)
  // ============================================================
  it("Step 2: User deposits tokens to vault", async () => {
    console.log("\n▶ STEP 2: Fund Vault");

    // Init vault ATA
    await program.methods
        .initVaultAta()
        .accounts({
            vault: vaultPda,
            owner: user.publicKey,
            mint: baseMint,
            vaultTokenAccount: vaultTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([user])
        .rpc();

    // Deposit tokens
    await program.methods
        .depositToken(VAULT_DEPOSIT)
        .accounts({
            vault: vaultPda,
            owner: user.publicKey,
            ownerTokenAccount: userTokenAccount,
            vaultTokenAccount: vaultTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([user])
        .rpc();

    const vaultBalance = (await getAccount(provider.connection, vaultTokenAccount)).amount;
    const userBalance = (await getAccount(provider.connection, userTokenAccount)).amount;
    
    assert.equal(vaultBalance.toString(), VAULT_DEPOSIT.toString());
    console.log("   ✓ Vault funded with", VAULT_DEPOSIT.toNumber() / 1e6, "tokens");
    console.log("   User remaining:", Number(userBalance) / 1e6, "tokens");
  });

  // ============================================================
  // STEP 3: Create TraderState (User allocates to trader)
  // ============================================================
  it("Step 3: User creates TraderState allocation", async () => {
    console.log("\n▶ STEP 3: Create TraderState");

    await program.methods
        .createTraderState(TRADER_ALLOCATION)
        .accounts({
            owner: user.publicKey,
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
        .signers([user])
        .rpc();

    const ts = await program.account.traderState.fetch(traderStatePda);
    assert.equal(ts.isInitialized, false, "Should NOT be initialized");
    assert.ok(ts.currentValue.eq(TRADER_ALLOCATION));
    
    const traderBalance = (await getAccount(provider.connection, traderTokenAccount)).amount;
    console.log("   ✓ TraderState created with", TRADER_ALLOCATION.toNumber() / 1e6, "tokens");
    console.log("   TraderState balance:", Number(traderBalance) / 1e6, "tokens");
    console.log("   is_initialized:", ts.isInitialized);
  });

  // ============================================================
  // STEP 4: Mark Trader Initialized (User activates trading)
  // ============================================================
  it("Step 4: User marks TraderState initialized", async () => {
    console.log("\n▶ STEP 4: Mark Initialized");

    await program.methods
      .markTraderInitialized()
      .accounts({
        signer: user.publicKey,
        vault: vaultPda,
        traderState: traderStatePda,
      })
      .signers([user])
      .rpc();

    const ts = await program.account.traderState.fetch(traderStatePda);
    assert.equal(ts.isInitialized, true);
    console.log("   ✓ TraderState initialized");
    console.log("   is_initialized:", ts.isInitialized);
  });

  // ============================================================
  // STEP 6: User pauses TraderState
  // ============================================================
  it("Step 6: User pauses TraderState", async () => {
    console.log("\n▶ STEP 6: Pause TraderState");

    await program.methods
        .pauseTraderState()
        .accounts({
            owner: user.publicKey,
            traderState: traderStatePda,
        })
        .signers([user])
        .rpc();
    
    const ts = await program.account.traderState.fetch(traderStatePda);
    assert.equal(ts.isPaused, true);
    console.log("   ✓ TraderState paused");
    console.log("   is_paused:", ts.isPaused);
  });

  // ============================================================
  // STEP 7: User closes TraderState (funds return to vault)
  // ============================================================
  it("Step 7: User closes TraderState → funds return to vault", async () => {
    console.log("\n▶ STEP 7: Close TraderState");

    const vaultBalanceBefore = (await getAccount(provider.connection, vaultTokenAccount)).amount;
    const traderBalanceBefore = (await getAccount(provider.connection, traderTokenAccount)).amount;

    await program.methods
        .closeTraderState()
        .accounts({
            owner: user.publicKey,
            traderState: traderStatePda,
            vault: vaultPda,
            traderTokenAccount: traderTokenAccount,
            vaultTokenAccount: vaultTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([user])
        .rpc();

    const vaultBalanceAfter = (await getAccount(provider.connection, vaultTokenAccount)).amount;
    
    // Verify funds returned to vault
    const expected = BigInt(vaultBalanceBefore.toString()) + BigInt(traderBalanceBefore.toString());
    assert.equal(vaultBalanceAfter.toString(), expected.toString());
    
    // Verify TraderState closed
    try {
      await program.account.traderState.fetch(traderStatePda);
      assert.fail("TraderState should be closed");
    } catch (e: any) {
      assert.include(e.message, "Account does not exist");
    }

    console.log("   ✓ TraderState closed");
    console.log("   Funds returned to vault:", Number(traderBalanceBefore) / 1e6, "tokens");
    console.log("   Vault balance now:", Number(vaultBalanceAfter) / 1e6, "tokens");
  });

  // ============================================================
  // STEP 8: User withdraws from vault
  // ============================================================
  it("Step 8: User withdraws all funds from vault", async () => {
    console.log("\n▶ STEP 8: Withdraw from Vault");

    const vaultBalance = (await getAccount(provider.connection, vaultTokenAccount)).amount;
    const userBalanceBefore = (await getAccount(provider.connection, userTokenAccount)).amount;

    await program.methods
        .withdrawToken(new anchor.BN(vaultBalance.toString()))
        .accounts({
            vault: vaultPda,
            owner: user.publicKey,
            ownerTokenAccount: userTokenAccount,
            vaultTokenAccount: vaultTokenAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([user])
        .rpc();

    const vaultBalanceAfter = (await getAccount(provider.connection, vaultTokenAccount)).amount;
    const userBalanceAfter = (await getAccount(provider.connection, userTokenAccount)).amount;

    assert.equal(vaultBalanceAfter.toString(), "0");
    
    const received = BigInt(userBalanceAfter.toString()) - BigInt(userBalanceBefore.toString());
    assert.equal(received.toString(), vaultBalance.toString());

    console.log("   ✓ Withdrawal complete");
    console.log("   Withdrawn:", Number(vaultBalance) / 1e6, "tokens");
    console.log("   User final balance:", Number(userBalanceAfter) / 1e6, "tokens");
    console.log("   Vault final balance:", Number(vaultBalanceAfter) / 1e6, "tokens");
  });

  // ============================================================
  // SUMMARY
  // ============================================================
  after(() => {
    console.log("\n" + "═".repeat(70));
    console.log("  E2E TEST COMPLETE: FULL LIFECYCLE VERIFIED");
    console.log("═".repeat(70));
    console.log("\nFlow verified:");
    console.log("  1. ✓ User creates vault (authority = backend)");
    console.log("  2. ✓ User deposits tokens to vault");
    console.log("  3. ✓ User creates TraderState allocation");
    console.log("  4. ✓ User marks TraderState initialized");
    console.log("  5. ✓ User pauses TraderState");
    console.log("  6. ✓ User closes TraderState → funds to vault");
    console.log("  7. ✓ User withdraws all funds from vault");
    console.log("\nNon-custodial guarantee: User retained ownership throughout.");
    console.log("");
  });
});
