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

describe("Phase 5: Backend Swap Verification (Integration)", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.StellalphaVault as Program<StellalphaVault>;

  // Main wallet (Backend Agent)
  const walletPath = os.homedir() + "/.config/solana/devnet-wallet.json";
  // Fallback if file doesn't exist (e.g. CI), but we expect it to exist in this env
  const rawKey = fs.existsSync(walletPath) 
    ? JSON.parse(fs.readFileSync(walletPath, "utf-8"))
    : anchor.web3.Keypair.generate().secretKey;
  const backendKeypair = anchor.web3.Keypair.fromSecretKey(Uint8Array.from(rawKey));

  // Ephemeral keys for this test run
  // We use backendKeypair as the Vault Owner/Authority to simulate production setup
  const trader = anchor.web3.Keypair.generate(); 
  
  let baseMint: anchor.web3.PublicKey;
  let platformFeeAccount: anchor.web3.PublicKey;
  let vaultPda: anchor.web3.PublicKey;
  let vaultTokenAccount: anchor.web3.PublicKey;
  let traderStatePda: anchor.web3.PublicKey;
  let inputTokenAccount: anchor.web3.PublicKey;
  let outputTokenAccount: anchor.web3.PublicKey;

  const FUNDING_AMOUNT = new anchor.BN(1_000_000); 
  const SWAP_AMOUNT_IN = new anchor.BN(100_000);   
  const MIN_AMOUNT_OUT = new anchor.BN(90_000);   
  
  // Mock Swap Program ID (must match deployed mock_swap)
  const MOCK_SWAP_PROGRAM_ID = new anchor.web3.PublicKey("DcVa1Kxo9DCUuvj6E8eJpUv9pARdGwWTM72MCT2vC3rS");

  before(async () => {
    console.log("Setting up Phase 5 Integration Test...");
    console.log("Backend Agent:", backendKeypair.publicKey.toString());

    // 1. Setup Base Mint
    // We create a new mint for isolation
    baseMint = await createMint(
      provider.connection,
      backendKeypair, 
      backendKeypair.publicKey,
      null,
      6
    );
    console.log("Base Mint:", baseMint.toString());

    // 2. Setup Platform Fee Account (Admin's ATA)
    // GlobalConfig admin is likely backendKeypair from previous tests.
    // We create the ATA for backendKeypair.
    platformFeeAccount = await createAccount(
        provider.connection,
        backendKeypair, 
        baseMint,
        backendKeypair.publicKey
    );

    // 3. Initialize Global Config (if needed)
    const [globalConfigPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("global_config")],
      program.programId
    );
    
    // Check if initialized
    const gcInfo = await provider.connection.getAccountInfo(globalConfigPda);
    if (!gcInfo) {
        await program.methods.initializeGlobalConfig()
          .accounts({
            globalConfig: globalConfigPda,
            admin: backendKeypair.publicKey,
            systemProgram: anchor.web3.SystemProgram.programId,
          })
          .signers([backendKeypair])
          .rpc();
        console.log("Global Config Initialized");
    }

    // 4. Initialize UserVault
    [vaultPda] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("user_vault_v1"), backendKeypair.publicKey.toBuffer()],
        program.programId
    );
    
    // Initialize or skip if exists
    const vaultInfo = await provider.connection.getAccountInfo(vaultPda);
    if (!vaultInfo) {
        await program.methods
            .initializeVault(backendKeypair.publicKey, baseMint)
            .accounts({
                vault: vaultPda,
                owner: backendKeypair.publicKey,
                systemProgram: anchor.web3.SystemProgram.programId,
            })
            .signers([backendKeypair])
            .rpc();
        console.log("Vault Initialized");
    }

    // 5. Fund Vault
    vaultTokenAccount = getAssociatedTokenAddressSync(baseMint, vaultPda, true);
    
    // Check if Vault ATA exists, if not create
    const vaultAtaInfo = await provider.connection.getAccountInfo(vaultTokenAccount);
    if (!vaultAtaInfo) {
        await program.methods.initVaultAta().accounts({
            vault: vaultPda, owner: backendKeypair.publicKey, mint: baseMint,
            vaultTokenAccount: vaultTokenAccount, tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID, systemProgram: anchor.web3.SystemProgram.programId
        }).signers([backendKeypair]).rpc();
    }

    // Mint tokens directly to Vault ATA to simulate funding
    await mintTo(provider.connection, backendKeypair, baseMint, vaultTokenAccount, backendKeypair.publicKey, 5_000_000);

    // 6. Create TraderState
    [traderStatePda] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("trader_state"), backendKeypair.publicKey.toBuffer(), trader.publicKey.toBuffer()],
        program.programId
    );
    inputTokenAccount = getAssociatedTokenAddressSync(baseMint, traderStatePda, true);
    
    // Create separate output token account (same mint) for mock_swap test
    outputTokenAccount = await createAccount(
        provider.connection,
        backendKeypair,
        baseMint,
        traderStatePda,
        anchor.web3.Keypair.generate()
    );

    await program.methods.createTraderState(FUNDING_AMOUNT).accounts({
        owner: backendKeypair.publicKey, trader: trader.publicKey, vault: vaultPda,
        traderState: traderStatePda, vaultTokenAccount: vaultTokenAccount,
        traderTokenAccount: inputTokenAccount, mint: baseMint,
        systemProgram: anchor.web3.SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID
    }).signers([backendKeypair]).rpc();

    // 7. Initialize TraderState
    await program.methods.markTraderInitialized().accounts({
        signer: backendKeypair.publicKey, vault: vaultPda, traderState: traderStatePda,
    }).signers([backendKeypair]).rpc();

    console.log("Setup Complete");
  });

  it("Executes Backend Swap via Mock Program", async () => {
    const [globalConfigPda] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("global_config")],
      program.programId
    );

    const balanceInBefore = (await getAccount(provider.connection, inputTokenAccount)).amount;
    const balanceOutBefore = (await getAccount(provider.connection, outputTokenAccount)).amount;
    const feeBalanceBefore = (await getAccount(provider.connection, platformFeeAccount)).amount;

    console.log("Balance In Before:", balanceInBefore.toString());
    console.log("Balance Out Before:", balanceOutBefore.toString());

    // Mock Swap Data (passed to execute_trader_swap)
    // stellalpha_vault passes this data through to the CPI call
    // The mock_swap program expects (amount_in, min_amount_out)
    // But wait, execute_trader_swap takes (amount_in, min_amount_out, data)
    // And it passes 'data' to the CPI.
    // The mock_swap instruction `swap` arguments are (amount_in, min_amount_out).
    // The `data` buffer passed to CPI must match the selector + args of mock_swap::swap.
    // However, stellalpha_vault passes `data` as the instruction data.
    
    // Wait, let's verify how stellalpha_vault constructs the CPI instruction.
    // Line 637: `data: data,`
    
    // The `data` argument to `execute_trader_swap` MUST contain the discriminator and arguments for `mock_swap:swap`.
    // In `scripts/backend_swap.ts`, we constructed this manually.
    
    // IDL for mock_swap:
    // swap(amount_in: u64, min_amount_out: u64)
    // Discriminator is sha256("global:swap")[..8]
    
    // Let's rely on Anchor to construct the data, or construct manually.
    // Creating a dummy program instance to encode the instruction data is easiest if possible,
    // or just manual encoding.
    
    // Manual encoding:
    // Discriminator: "global:swap" -> sighash
    // u64 amount_in
    // u64 min_amount_out
    
    // But wait, `execute_trader_swap` arguments are (amount_in, min_amount_out, data).
    // The `amount_in` and `min_amount_out` passed to `execute_trader_swap` are used by stellalpha_vault for checks.
    // The `data` is passed BLINDLY to the CPI.
    // So `data` must encode the call to `mock_swap::swap(amount_in, min_amount_out)`.
    
    const hasher = anchor.utils.sha256.hash("global:swap");
    const sighash = Buffer.from(hasher.slice(0, 16), "hex"); // anchor 0.29+ utils might vary, check sighash logic
    // Actually the sighash is first 8 bytes of sha256("global:swap")
    
    // Let's use the actual mock_swap program interface to encode if we can.
    // Or just reproduce the logic from `backend_swap.ts`:
    // const mockSwapData = Buffer.concat([
    //   Buffer.from("248c69e917587c86", "hex"), // global:swap discriminator
    //   AMOUNT_IN.toBuffer("le", 8),
    //   MIN_AMOUNT_OUT.toBuffer("le", 8),
    // ]);
    
    // I need the discriminator. "global:swap"
    const discriminator = anchor.utils.sha256.hash("global:swap").slice(0, 16); 
    // Wait, hash returns hex string. slice(0, 16) gives 16 hex chars = 8 bytes.
    // "global:swap" -> 248c69e917587c86...
    
    const mockSwapData = Buffer.concat([
        Buffer.from("248c69e917587c86", "hex"),
        SWAP_AMOUNT_IN.toBuffer("le", 8),
        MIN_AMOUNT_OUT.toBuffer("le", 8),
    ]);
    
    const remainingAccounts = [
        { pubkey: traderStatePda, isWritable: false, isSigner: false }, // authority (signer via invoke_signed)
        { pubkey: inputTokenAccount, isWritable: true, isSigner: false },
        { pubkey: outputTokenAccount, isWritable: true, isSigner: false },
        { pubkey: TOKEN_PROGRAM_ID, isWritable: false, isSigner: false },
    ];

    await program.methods.executeTraderSwap(SWAP_AMOUNT_IN, MIN_AMOUNT_OUT, mockSwapData)
        .accounts({
            authority: backendKeypair.publicKey,
            vault: vaultPda,
            traderState: traderStatePda,
            inputTokenAccount: inputTokenAccount,
            outputTokenAccount: outputTokenAccount,
            platformFeeAccount: platformFeeAccount,
            globalConfig: globalConfigPda,
            jupiterProgram: MOCK_SWAP_PROGRAM_ID,
            tokenProgram: TOKEN_PROGRAM_ID,
            instructions: anchor.web3.SYSVAR_INSTRUCTIONS_PUBKEY,
        })
        .remainingAccounts(remainingAccounts)
        .signers([backendKeypair])
        .rpc();

    console.log("Swap Executed");

    const balanceInAfter = (await getAccount(provider.connection, inputTokenAccount)).amount;
    const balanceOutAfter = (await getAccount(provider.connection, outputTokenAccount)).amount;
    const feeBalanceAfter = (await getAccount(provider.connection, platformFeeAccount)).amount;

    // Verify Fee (10 bps of 100,000 = 100)
    const expectedFee = BigInt(100);
    assert.equal(feeBalanceAfter - feeBalanceBefore, expectedFee, "Fee should be paid");

    // Verify Output (95% of 99,900 = 94,905)
    // Input used = 100,000
    // Fee = 100
    // Swap Input = 99,900
    // Return = 99,900 * 0.95 = 94,905
    
    const expectedOutputDelta = BigInt(94905);
    assert.equal(balanceOutAfter - balanceOutBefore, expectedOutputDelta, "Output should match mock 95% logic");
    
    // Verify Input Decrease
    // Should be exactly SWAP_AMOUNT_IN (100,000)
    // Because mock_swap doesn't burn/transfer from input?
    // Wait, mock_swap DOES transfer from input to output.
    // So input should decrease by SWAP_AMOUNT_IN?
    // In `backend_swap.ts` we saw:
    // Input ATA: 500,000 -> 404,995 (-95,005)
    // Wait. 
    // Fee = 100.
    // Transfer (Swap) = 94,905.
    // Total out of Input = 100 + 94,905 = 95,005.
    // But SWAP_AMOUNT_IN passed to `execute_trader_swap` is 100,000.
    // The `stellalpha_vault` verifies `amount_spent`.
    
    // amount_spent = balance_in_before - balance_in_after
    // amount_spent <= swap_amount_in is the restriction.
    // Here amount_spent = 95,005 which is <= 100,000. So it passes.
    
    const expectedInputDecrease = BigInt(95005);
    assert.equal(balanceInBefore - balanceInAfter, expectedInputDecrease, "Input balance reflection");
  });
});
