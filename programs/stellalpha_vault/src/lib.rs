use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer, Mint, CloseAccount};
use anchor_spl::associated_token::AssociatedToken;
use std::str::FromStr;

declare_id!("64XogE2RvY7g4fDp8XxWZxFTycANjDK37n88GZizm5nx");

// Jupiter V6 Program ID (Mocked to Memo v1 Program for Devnet)
pub const JUPITER_PROGRAM_ID: Pubkey = pubkey!("Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo");

// Platform Fee Wallet (Replace with actual address in production)
pub const PLATFORM_FEE_WALLET: Pubkey = pubkey!("11111111111111111111111111111111"); 

#[program]
pub mod stellalpha_vault {
    use super::*;

    pub fn initialize_vault(ctx: Context<InitializeVault>, authority: Pubkey, base_mint: Pubkey) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        vault.owner = ctx.accounts.owner.key();
        vault.authority = authority;
        vault.bump = ctx.bumps.vault;
        vault.is_paused = false;
        vault.trade_amount_lamports = 0;
        vault.base_mint = base_mint;
        vault.allowed_mints = Vec::new(); // Start empty
        msg!("Vault initialized for owner: {} with Base Asset: {}", vault.owner, base_mint);
        Ok(())
    }

    pub fn initialize_global_config(ctx: Context<InitializeGlobalConfig>) -> Result<()> {
        let config = &mut ctx.accounts.global_config;
        config.admin = ctx.accounts.admin.key();
        config.platform_fee_bps = 10; // 0.1% default
        config.performance_fee_bps = 2000; // 20% default
        config.legacy_trading_enabled = false; // Disabled by default for new deployments
        msg!("Global Config initialized. Admin: {}. Legacy trading disabled.", config.admin);
        Ok(())
    }

    pub fn add_allowed_mint(ctx: Context<ManageWhitelist>, mint: Pubkey) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        if !vault.allowed_mints.contains(&mint) {
            vault.allowed_mints.push(mint);
            msg!("Added allowed mint: {}", mint);
        }
        Ok(())
    }

    pub fn remove_allowed_mint(ctx: Context<ManageWhitelist>, mint: Pubkey) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        if let Some(pos) = vault.allowed_mints.iter().position(|x| *x == mint) {
            vault.allowed_mints.remove(pos);
            msg!("Removed allowed mint: {}", mint);
        }
        Ok(())
    }

    pub fn toggle_pause(ctx: Context<TogglePause>) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        vault.is_paused = !vault.is_paused;
        msg!("Vault pause state toggled to: {}", vault.is_paused);
        Ok(())
    }

    /// Toggle legacy trading enabled/disabled. Admin only.
    pub fn toggle_legacy_trading(ctx: Context<AdminGlobalConfig>) -> Result<()> {
        let config = &mut ctx.accounts.global_config;
        config.legacy_trading_enabled = !config.legacy_trading_enabled;
        msg!("Legacy trading toggled to: {}", config.legacy_trading_enabled);
        
        emit!(LegacyTradingToggled {
            enabled: config.legacy_trading_enabled,
            admin: ctx.accounts.admin.key(),
        });
        
        Ok(())
    }

    pub fn deposit_sol(ctx: Context<DepositSol>, amount: u64) -> Result<()> {
        let ix = anchor_lang::solana_program::system_instruction::transfer(
            &ctx.accounts.owner.key(),
            &ctx.accounts.vault.key(),
            amount,
        );
        anchor_lang::solana_program::program::invoke(
            &ix,
            &[
                ctx.accounts.owner.to_account_info(),
                ctx.accounts.vault.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
            ],
        )?;
        msg!("Deposited {} lamports to vault", amount);
        Ok(())
    }

    pub fn withdraw_sol(ctx: Context<WithdrawSol>, amount: u64) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        let owner = &mut ctx.accounts.owner;
        
        require!(vault.owner == owner.key(), ErrorCode::Unauthorized);

        **vault.to_account_info().try_borrow_mut_lamports()? -= amount;
        **owner.to_account_info().try_borrow_mut_lamports()? += amount;

        msg!("Withdrew {} lamports from vault", amount);
        Ok(())
    }

    pub fn deposit_token(ctx: Context<DepositToken>, amount: u64) -> Result<()> {
        let cpi_accounts = Transfer {
            from: ctx.accounts.owner_token_account.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.owner.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;
        msg!("Deposited {} tokens to vault", amount);
        Ok(())
    }

    pub fn withdraw_token(ctx: Context<WithdrawToken>, amount: u64) -> Result<()> {
        let vault = &ctx.accounts.vault;
        let seeds = &[
            b"user_vault_v1",
            vault.owner.as_ref(),
            &[vault.bump],
        ];
        let signer = &[&seeds[..]];

        let cpi_accounts = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.owner_token_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, amount)?;
        msg!("Withdrew {} tokens from vault", amount);
        Ok(())
    }

    /// Close a Vault Token Account (ATA) if its balance is zero.
    /// Only the owner can close, and rent is returned to owner.
    pub fn close_vault_ata(ctx: Context<CloseVaultAta>) -> Result<()> {
        require!(
            ctx.accounts.vault_token_account.amount == 0,
            ErrorCode::NonZeroBalance
        );
        
        let vault = &ctx.accounts.vault;
        let seeds = &[
            b"user_vault_v1",
            vault.owner.as_ref(),
            &[vault.bump],
        ];
        let signer = &[&seeds[..]];

        let close_accounts = CloseAccount {
            account: ctx.accounts.vault_token_account.to_account_info(),
            destination: ctx.accounts.owner.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            close_accounts,
            signer
        );
        token::close_account(cpi_ctx)?;
        
        msg!("Closed Vault ATA. Rent returned to owner.");
        Ok(())
    }

    pub fn init_vault_ata(ctx: Context<InitVaultAta>) -> Result<()> {
        msg!("Initialized Vault ATA for mint: {}", ctx.accounts.mint.key());
        Ok(())
    }

    // =========================================================================
    // LEGACY PATH — DEPRECATED
    // Disabled by GlobalConfig.legacy_trading_enabled
    // DO NOT EXTEND. All new execution must use TraderState (execute_trader_swap).
    // =========================================================================
    pub fn execute_swap(ctx: Context<ExecuteSwap>, amount_in: u64, min_amount_out: u64) -> Result<()> {
        // Gate: Legacy trading must be enabled
        require!(
            ctx.accounts.global_config.legacy_trading_enabled,
            ErrorCode::LegacyTradingDisabled
        );

        let vault = &ctx.accounts.vault;
        require!(!vault.is_paused, ErrorCode::Paused);
        require!(vault.authority == ctx.accounts.authority.key(), ErrorCode::Unauthorized);

        // --- Security Checks (Non-Custodial Invariants) ---

        let mint_in = ctx.accounts.token_account_in.mint;
        let mint_out = ctx.accounts.token_account_out.mint;
        let base_mint = vault.base_mint;

        // 1. Whitelist Check
        let is_valid_mint = |mint: Pubkey| -> bool {
            mint == base_mint || vault.allowed_mints.contains(&mint)
        };
        require!(is_valid_mint(mint_in), ErrorCode::TokenNotAllowed);
        require!(is_valid_mint(mint_out), ErrorCode::TokenNotAllowed);

        // 2. Topology Check (Round-Trip Guarantee)
        require!(
            mint_in == base_mint || mint_out == base_mint,
            ErrorCode::InvalidSwapTopology
        );

        // 3. Platform Fee Destination Check
        // Ensure the fee is going to the correct wallet.
        // We check the owner of the token account matches the hardcoded wallet.
        require!(ctx.accounts.platform_fee_account.owner == PLATFORM_FEE_WALLET, ErrorCode::InvalidFeeDestination);

        // 4. Snapshots for Balance Validation
        let balance_in_before = ctx.accounts.token_account_in.amount;
        let balance_out_before = ctx.accounts.token_account_out.amount;

        // --- Execution ---

        // 5. Deduct Platform Fee (0.1%)
        let fee_amount = amount_in.checked_mul(10).unwrap().checked_div(10000).unwrap(); // 10 bps
        
        if fee_amount > 0 {
            let seeds = &[
                b"user_vault_v1",
                vault.owner.as_ref(),
                &[vault.bump],
            ];
            let signer = &[&seeds[..]];

            let cpi_accounts = Transfer {
                from: ctx.accounts.token_account_in.to_account_info(),
                to: ctx.accounts.platform_fee_account.to_account_info(),
                authority: ctx.accounts.vault.to_account_info(),
            };
            let cpi_program = ctx.accounts.token_program.to_account_info();
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
            token::transfer(cpi_ctx, fee_amount)?;
            msg!("Deducted platform fee: {}", fee_amount);
        }

        // 6. Execute Jupiter Swap via CPI (Zero-Copy Introspection)
        let jupiter_program = &ctx.accounts.jupiter_program;
        let remaining_accounts = ctx.remaining_accounts;

        // Load current instruction to get raw data buffer
        use anchor_lang::solana_program::sysvar::instructions::{
             load_current_index_checked,
             load_instruction_at_checked,
        };

        let current_ix_index = load_current_index_checked(&ctx.accounts.sysvar_instructions)?;
        let current_ix = load_instruction_at_checked(current_ix_index as usize, &ctx.accounts.sysvar_instructions)?;

        // Validate data length. Anchor discriminator (8) + amount_in (8) + min_amount_out (8) = 24 bytes
        // The rest is the Jupiter Instruction Data that was appended.
        let header_len = 8 + 8 + 8;
        require!(
            current_ix.data.len() > header_len,
            ErrorCode::InvalidInstructionData
        );

        // Zero-copy slice of jupiter data
        let jupiter_data = &current_ix.data[header_len..];
        msg!("Introspected Jupiter Data Len: {}", jupiter_data.len());

        let mut accounts = vec![];
        for acc in remaining_accounts {
            accounts.push(if acc.is_writable {
                AccountMeta::new(acc.key(), acc.is_signer)
            } else {
                AccountMeta::new_readonly(acc.key(), acc.is_signer)
            });
        }

        let ix = anchor_lang::solana_program::instruction::Instruction {
            program_id: jupiter_program.key(),
            accounts,
            data: jupiter_data.to_vec(), // Necessary copy for invoke, but bounded by slice
        };

        let seeds = &[
            b"user_vault",
            vault.owner.as_ref(),
            &[vault.bump],
        ];
        let signer = &[&seeds[..]];

        // Invoke signed
        anchor_lang::solana_program::program::invoke_signed(
            &ix,
            remaining_accounts,
            signer,
        )?;

        // --- Post-Swap Security Validation ---

        ctx.accounts.token_account_in.reload()?;
        ctx.accounts.token_account_out.reload()?;
        
        let balance_in_after = ctx.accounts.token_account_in.amount;
        let balance_out_after = ctx.accounts.token_account_out.amount;

        // 7. Slippage Protection (MUST-HAVE)
        // Ensure we received at least the minimum amount expected.
        // Also serves as the "Balance Must Increase" check.
        let amount_received = balance_out_after.checked_sub(balance_out_before).unwrap_or(0);
        require!(amount_received >= min_amount_out, ErrorCode::SlippageExceeded);

        // 8. Fee Evasion Check (MUST-HAVE)
        // Ensure the Authority didn't under-declare 'amount_in' to pay less fee.
        // We check: The total decrease in the input account (fee + swap) must be <= amount_in passed.
        // If they swapped MORE than 'amount_in', then they underpaid fee.
        // Note: balance_in_before - balance_in_after includes the fee deduction we did earlier.
        // Example: 
        // Declared: 1000. Fee: 1. Before: 2000.
        // Deduct 1: Before->1999.
        // Swap 999: After->1000.
        // Total Decrease: 2000 - 1000 = 1000.
        // 1000 <= 1000. OK.
        //
        // Exploit Attempt:
        // Declared: 1. Fee: 0. Before: 2000.
        // Deduct 0: Before->2000.
        // Swap 1000: After->1000.
        // Total Decrease: 1000.
        // 1000 <= 1. FAIL.
        let amount_spent = balance_in_before.checked_sub(balance_in_after).unwrap_or(0);
        require!(amount_spent <= amount_in, ErrorCode::FeeEvasion);

        msg!("Swap Success. In: {} (fee+swap), Out: {}", amount_spent, amount_received);
        Ok(())
    }
    pub fn create_trader_state(ctx: Context<CreateTraderState>, amount: u64) -> Result<()> {
        let trader_state = &mut ctx.accounts.trader_state;
        trader_state.owner = ctx.accounts.owner.key();
        trader_state.trader = ctx.accounts.trader.key();
        trader_state.vault = ctx.accounts.vault.key();
        trader_state.bump = ctx.bumps.trader_state;
        
        trader_state.current_value = amount;
        trader_state.high_water_mark = amount;
        trader_state.cumulative_profit = 0;
        trader_state.is_paused = false;
        trader_state.is_initialized = false;  // Phase 7B: Must call mark_trader_initialized after sync

        // Funding: Transfer 'amount' from UserVault ATA to TraderState ATA
        let vault = &ctx.accounts.vault;
        let seeds = &[
            b"user_vault_v1",
            vault.owner.as_ref(),
            &[vault.bump],
        ];
        let signer = &[&seeds[..]];

        let cpi_accounts = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.trader_token_account.to_account_info(),
            authority: ctx.accounts.vault.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, amount)?;

        msg!("Created TraderState for trader: {}. Funded with: {}", trader_state.trader, amount);
        Ok(())
    }

    pub fn pause_trader_state(ctx: Context<UpdateTraderState>) -> Result<()> {
        let trader_state = &mut ctx.accounts.trader_state;
        trader_state.is_paused = true;
        msg!("TraderState paused.");
        Ok(())
    }

    pub fn resume_trader_state(ctx: Context<UpdateTraderState>) -> Result<()> {
        let trader_state = &mut ctx.accounts.trader_state;
        trader_state.is_paused = false;
        msg!("TraderState resumed.");
        Ok(())
    }

    pub fn close_trader_state(ctx: Context<CloseTraderState>) -> Result<()> {
        let trader_state = &ctx.accounts.trader_state;
        require!(trader_state.is_paused, ErrorCode::TraderNotPaused);

        // Refund: Transfer ALL balance from TraderState ATA to UserVault ATA
        let balance = ctx.accounts.trader_token_account.amount;
        if balance > 0 {
            let seeds = &[
                b"trader_state",
                trader_state.owner.as_ref(),
                trader_state.trader.as_ref(),
                &[trader_state.bump],
            ];
            let signer = &[&seeds[..]];

            let cpi_accounts = Transfer {
                from: ctx.accounts.trader_token_account.to_account_info(),
                to: ctx.accounts.vault_token_account.to_account_info(),
                authority: ctx.accounts.trader_state.to_account_info(),
            };
            let cpi_program = ctx.accounts.token_program.to_account_info();
            let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
            token::transfer(cpi_ctx, balance)?;
            msg!("Refunded {} tokens to UserVault", balance);
        }

        // Close TraderState ATA
        let seeds = &[
            b"trader_state",
            trader_state.owner.as_ref(),
            trader_state.trader.as_ref(),
            &[trader_state.bump],
        ];
        let signer = &[&seeds[..]];

        let cpi_accounts_close = token::CloseAccount {
            account: ctx.accounts.trader_token_account.to_account_info(),
            destination: ctx.accounts.owner.to_account_info(), // Rent to owner
            authority: ctx.accounts.trader_state.to_account_info(),
        };
        let cpi_ctx_close = CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), cpi_accounts_close, signer);
        token::close_account(cpi_ctx_close)?;

        msg!("Closed TraderState and refunded.");
        Ok(())
    }

    // =========================================================================
    // PHASE 7: Multi-Asset Support
    // =========================================================================

    /// Phase 7A: Create additional token account for TraderState to hold non-base assets.
    /// Owner-only. No funds transferred.
    pub fn create_trader_ata(ctx: Context<CreateTraderAta>) -> Result<()> {
        msg!(
            "Created TraderState ATA for mint: {}",
            ctx.accounts.mint.key()
        );
        Ok(())
    }

    /// Phase 7D: Mark TraderState as initialized after portfolio sync.
    /// Can be called by owner OR backend authority (one-time only).
    pub fn mark_trader_initialized(ctx: Context<MarkTraderInitialized>) -> Result<()> {
        let trader_state = &mut ctx.accounts.trader_state;
        let vault = &ctx.accounts.vault;
        let signer = ctx.accounts.signer.key();

        // Only owner or backend authority may initialize
        require!(
            signer == trader_state.owner || signer == vault.authority,
            ErrorCode::Unauthorized
        );

        require!(!trader_state.is_initialized, ErrorCode::AlreadyInitialized);

        trader_state.is_initialized = true;
        msg!("TraderState marked as initialized.");
        Ok(())
    }

    /// Phase 7.1: Close a non-base TraderState ATA to reclaim rent.
    /// Owner-only. Requires is_paused = true. ATA balance must be 0.
    /// Rent returned to owner.
    pub fn close_trader_ata(ctx: Context<CloseTraderAtaContext>) -> Result<()> {
        let trader_state = &ctx.accounts.trader_state;
        
        // Safety: Must be paused to prevent closing ATAs mid-trade
        require!(trader_state.is_paused, ErrorCode::TraderNotPaused);
        
        // Safety: Cannot close ATA with funds
        require!(
            ctx.accounts.trader_token_account.amount == 0,
            ErrorCode::NonZeroBalance
        );
        
        // Close the ATA
        let seeds = &[
            b"trader_state",
            trader_state.owner.as_ref(),
            trader_state.trader.as_ref(),
            &[trader_state.bump],
        ];
        let signer = &[&seeds[..]];
        
        let close_accounts = CloseAccount {
            account: ctx.accounts.trader_token_account.to_account_info(),
            destination: ctx.accounts.owner.to_account_info(),
            authority: trader_state.to_account_info(),
        };
        let close_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            close_accounts,
            signer
        );
        token::close_account(close_ctx)?;
        
        msg!("Closed TraderState ATA for mint: {}. Rent returned to owner.", 
            ctx.accounts.trader_token_account.mint);
        Ok(())
    }

    /// Execute a swap on behalf of a TraderState via Jupiter CPI.
    /// amount_in: Total amount to spend, including platform fee.
    /// min_amount_out: Minimum amount to receive (slippage protection).
    /// data: Opaque data blob for Jupiter swap instruction.
    pub fn execute_trader_swap(ctx: Context<ExecuteTraderSwap>, amount_in: u64, min_amount_out: u64, data: Vec<u8>) -> Result<()> {
        let trader_state = &mut ctx.accounts.trader_state;
        let vault = &ctx.accounts.vault;
        let global_config = &ctx.accounts.global_config;

        // 1. Auth & Status Checks
        require!(!trader_state.is_paused, ErrorCode::TraderPaused);
        require!(vault.authority == ctx.accounts.authority.key(), ErrorCode::Unauthorized);
        // Phase 7B: Require initialization before trading
        require!(trader_state.is_initialized, ErrorCode::TraderNotInitialized);

        // 2. Topology Checks
        let input_mint = ctx.accounts.input_token_account.mint;
        let output_mint = ctx.accounts.output_token_account.mint;
        let base_mint = vault.base_mint;
        
        // Phase 7C: Explicit ownership validation for all swaps
        // Both token accounts MUST be owned by the TraderState PDA
        let is_trader_owned_in = ctx.accounts.input_token_account.owner == trader_state.key();
        let is_trader_owned_out = ctx.accounts.output_token_account.owner == trader_state.key();
        require!(is_trader_owned_in, ErrorCode::InvalidTokenAccountOwner);
        require!(is_trader_owned_out, ErrorCode::InvalidTokenAccountOwner);

        // Phase 7C: Relaxed topology for token→token swaps
        // Original: at least one side must be Base Asset
        // New: Allow non-base swaps if both sides owned by this TraderState
        // This is already guaranteed by the ownership checks above
        require!(
            (input_mint == base_mint || output_mint == base_mint) ||
            (is_trader_owned_in && is_trader_owned_out),
            ErrorCode::InvalidSwapTopology
        );

        // 3. Platform Fee
        // Ensure fee destination is correct (admin's token account)
        require!(ctx.accounts.platform_fee_account.owner == global_config.admin, ErrorCode::InvalidFeeDestination);
        // Mint of fee account must match input mint? 
        // Logic: Fee is taken from input amount. So fee account must accept input token.
        require!(ctx.accounts.platform_fee_account.mint == input_mint, ErrorCode::InvalidFeeDestination); 

        let fee_bps = global_config.platform_fee_bps as u64;
        let fee = (amount_in as u128)
            .checked_mul(fee_bps as u128)
            .unwrap()
            .checked_div(10000)
            .unwrap() as u64;
        
        // Safety: swap_amount is what initiates the swap. Verification uses full amount_in budget.
        let swap_amount = amount_in.checked_sub(fee).ok_or(ErrorCode::FeeEvasion)?;

        // Transfer Fee
        if fee > 0 {
            let seeds = &[
                b"trader_state",
                trader_state.owner.as_ref(),
                trader_state.trader.as_ref(),
                &[trader_state.bump],
            ];
            let signer = &[&seeds[..]];

            let cpi_accounts = Transfer {
                from: ctx.accounts.input_token_account.to_account_info(),
                to: ctx.accounts.platform_fee_account.to_account_info(),
                authority: trader_state.to_account_info(), // Use ref
            };
            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                cpi_accounts, 
                signer
            );
            token::transfer(cpi_ctx, fee)?;
            msg!("Paid platform fee: {}", fee);
        }

        // 4. Jupiter CPI
        let seeds = &[
            b"trader_state",
            trader_state.owner.as_ref(),
            trader_state.trader.as_ref(),
            &[trader_state.bump],
        ];
        let signer = &[&seeds[..]];

        // Balance Snapshot
        // RELOAD required because fee transfer modified the account on-chain, 
        // but local 'ctx.accounts' struct is stale.
        ctx.accounts.input_token_account.reload()?;
        let balance_in_before = ctx.accounts.input_token_account.amount;
        let balance_out_before = ctx.accounts.output_token_account.amount;

        // Devnet Mock (Memo) vs Mainnet (Jupiter)
        let jupiter_program_id = ctx.accounts.jupiter_program.key();
        let memo_program_id = Pubkey::from_str("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcQb").unwrap();
        
        if jupiter_program_id == memo_program_id {
             // Mock Swap via Memo
             msg!("Devnet: Simulating swap via Memo");
             // Simulate token movement if mints match (test only)
             if input_mint == output_mint {
                 let cpi_accounts = Transfer {
                    from: ctx.accounts.input_token_account.to_account_info(),
                    to: ctx.accounts.output_token_account.to_account_info(),
                    authority: trader_state.to_account_info(), // Use ref
                };
                let cpi_ctx = CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), cpi_accounts, signer);
                token::transfer(cpi_ctx, swap_amount)?;
             }
        } else {
             // Real Jupiter CPI (or external swap program)
             // IMPORTANT: Mark TraderState PDA as signer in the CPI instruction.
             // This is required because invoke_signed signs for this PDA, and the
             // instruction's AccountMeta must have is_signer=true to match.
             // AUDIT: PDA signing via invoke_signed accepted; does not grant backend private key; invariants remain.
             let trader_state_key = trader_state.key();
             let remaining_accounts: Vec<anchor_lang::solana_program::instruction::AccountMeta> = ctx.remaining_accounts.iter().map(|acc| {
                // If this account is the TraderState PDA, mark as signer (will be signed via invoke_signed)
                let is_signer = if *acc.key == trader_state_key {
                    true
                } else {
                    acc.is_signer
                };
                if acc.is_writable {
                    anchor_lang::solana_program::instruction::AccountMeta::new(*acc.key, is_signer)
                } else {
                    anchor_lang::solana_program::instruction::AccountMeta::new_readonly(*acc.key, is_signer)
                }
             }).collect();

             let ix = anchor_lang::solana_program::instruction::Instruction {
                program_id: jupiter_program_id,
                accounts: remaining_accounts,
                data: data,
            };
            
            anchor_lang::solana_program::program::invoke_signed(
                &ix,
                ctx.remaining_accounts,
                signer
            )?;
        }

        // 5. Post-Swap Balance Check
        ctx.accounts.input_token_account.reload()?;
        ctx.accounts.output_token_account.reload()?;
        let balance_in_after = ctx.accounts.input_token_account.amount;
        let balance_out_after = ctx.accounts.output_token_account.amount;

        // amount_spent: balance decreased in Input Account.
        // This snapshot is AFTER fee transfer.
        // So balance_in_before = Initial - Fee.
        // balance_in_after = Final.
        // spent = (Initial - Fee) - Final.
        // We ensure spent <= swap_amount.
        let amount_spent = balance_in_before.checked_sub(balance_in_after).unwrap();
        let amount_received = balance_out_after.checked_sub(balance_out_before).unwrap();

        require!(amount_spent <= swap_amount, ErrorCode::FeeEvasion);
        require!(amount_received >= min_amount_out, ErrorCode::SlippageExceeded);

        // Phase 4: TraderState Accounting (Tx Fee Only)
        // Update current_value ONLY when swapping back to Base Asset.
        // We assume 'amount_received' represents the full value of the position being exited 
        // back into the Base Asset. Performance fees/HWM are explicitly deferred.
        if output_mint == base_mint {
            trader_state.current_value = amount_received;
            msg!("Updated TraderState current_value: {}", trader_state.current_value);
        }

        msg!("Swap Success. In: {}, Out: {}", amount_spent, amount_received);
        Ok(())
    }

    /// settlement: Validate that TraderState holds only Base Asset and amount >= current_value.
    /// Locks the state as 'Settled' to enable withdrawal.
    pub fn settle_trader_state(ctx: Context<SettleTraderState>) -> Result<()> {
        let trader_state = &mut ctx.accounts.trader_state;
        let trader_token_account = &ctx.accounts.trader_token_account;

        require!(trader_state.is_paused, ErrorCode::TraderNotPaused);
        require!(trader_token_account.mint == ctx.accounts.vault.base_mint, ErrorCode::MintMismatch);
        
        // Ensure solvency/full settlement
        // We require that the Base Asset holdings are at least the tracked equity.
        // This implicitly checks that we aren't hiding funds in other assets (if we assume strict accounting).
        require!(trader_token_account.amount >= trader_state.current_value, ErrorCode::InsufficientFunds);

        trader_state.is_settled = true;
        msg!("TraderState settled. Equity: {}", trader_state.current_value);
        Ok(())
    }

    /// withdraw: Exit flow.
    /// Prerequisites: Paused && Settled.
    /// Flow: TraderState -> UserVault -> User Wallet.
    /// Closes TraderState and its ATA.
    pub fn withdraw_trader_state(ctx: Context<WithdrawTraderState>) -> Result<()> {
        let trader_state = &ctx.accounts.trader_state;
        let vault = &ctx.accounts.vault;
        
        require!(trader_state.is_paused, ErrorCode::TraderNotPaused);
        require!(trader_state.is_settled, ErrorCode::NotSettled);

        // 1. Transfer TraderState -> UserVault
        let trader_seeds = &[
            b"trader_state",
            trader_state.owner.as_ref(),
            trader_state.trader.as_ref(),
            &[trader_state.bump],
        ];
        let trader_signer = &[&trader_seeds[..]];

        let amount = ctx.accounts.trader_token_account.amount;

        let cpi_accounts_trader = Transfer {
            from: ctx.accounts.trader_token_account.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: trader_state.to_account_info(),
        };
        let cpi_ctx_trader = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts_trader,
            trader_signer
        );
        token::transfer(cpi_ctx_trader, amount)?;
        
        // 2. Transfer UserVault -> Owner Wallet
        // UserVault seeds: [b"user_vault_v1", owner.key.as_ref()]
        let vault_seeds = &[
            b"user_vault_v1",
            vault.owner.as_ref(),
            &[vault.bump],
        ];
        let vault_signer = &[&vault_seeds[..]];

        let cpi_accounts_vault = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.owner_token_account.to_account_info(),
            authority: vault.to_account_info(),
        };
        let cpi_ctx_vault = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts_vault,
            vault_signer
        );
        token::transfer(cpi_ctx_vault, amount)?;

        // 3. Close TraderState ATA -> Owner
        let close_accounts = CloseAccount {
            account: ctx.accounts.trader_token_account.to_account_info(),
            destination: ctx.accounts.owner.to_account_info(),
            authority: trader_state.to_account_info(),
        };
        let close_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            close_accounts,
            trader_signer
        );
        token::close_account(close_ctx)?;

        msg!("Withdrawal complete. Amount: {}. TraderState closed.", amount);
        // TraderState Account itself is closed via `close = owner` in struct
        Ok(())
    }
}

#[account]
pub struct UserVault {
    pub owner: Pubkey,
    pub authority: Pubkey,
    pub bump: u8,
    pub is_paused: bool,
    pub trade_amount_lamports: u64, // Legacy field, kept for alignment
    pub base_mint: Pubkey,
    pub allowed_mints: Vec<Pubkey>,
}

impl UserVault {
    // Initial space buffer: 8 discriminator + 32 owner + 32 authority + 1 bump + 1 paused + 8 trade_amount + 32 base_mint + 4 vec_len + (32 * 10 initial capacity)
    pub const INIT_SPACE: usize = 8 + 32 + 32 + 1 + 1 + 8 + 32 + 4 + (32 * 10); 
}

#[account]
pub struct GlobalConfig {
    pub admin: Pubkey,
    pub platform_fee_bps: u16,
    pub performance_fee_bps: u16,
    /// If false, legacy execute_swap is disabled. Default: false.
    pub legacy_trading_enabled: bool,
}

impl GlobalConfig {
    // 8 discriminator + 32 admin + 2 platform_fee + 2 performance_fee + 1 legacy_flag
    pub const SPACE: usize = 8 + 32 + 2 + 2 + 1;
}

#[derive(Accounts)]
pub struct InitializeGlobalConfig<'info> {
    #[account(
        init,
        payer = admin,
        space = GlobalConfig::SPACE,
        seeds = [b"global_config"],
        bump
    )]
    pub global_config: Account<'info, GlobalConfig>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AdminGlobalConfig<'info> {
    #[account(
        mut,
        seeds = [b"global_config"],
        bump,
        has_one = admin @ ErrorCode::Unauthorized
    )]
    pub global_config: Account<'info, GlobalConfig>,
    pub admin: Signer<'info>,
}

#[event]
pub struct LegacyTradingToggled {
    pub enabled: bool,
    pub admin: Pubkey,
}

#[account]
pub struct TraderState {
    /// The user who owns this allocation and the funds.
    pub owner: Pubkey,
    
    /// The trader being followed (Strategy Identifier).
    pub trader: Pubkey,
    
    /// Reference to the UserVault this allocation is associated with.
    pub vault: Pubkey,
    
    /// PDA Bump.
    pub bump: u8,
    
    /// Current Value (Allocated Capital).
    pub current_value: u64,
    
    /// High Water Mark for performance fee calculation.
    pub high_water_mark: u64,
    
    /// Net realized PnL. Can be negative.
    /// Used for analytics and reporting.
    pub cumulative_profit: i64,
    
    /// Safety switch for this specific allocation.
    pub is_paused: bool, 

    /// True only when all TraderState funds are confirmed to be in base_mint.
    /// Required before withdrawal.
    pub is_settled: bool,

    /// True after initial portfolio sync is complete.
    /// Required for ongoing trading (Phase 7).
    pub is_initialized: bool,
}

impl TraderState {
    // 8 discriminator
    // + 32 (owner) + 32 (trader) + 32 (vault)
    // + 1 (bump)
    // + 8 (current_value)
    // + 8 (high_water_mark)
    // + 8 (cumulative_profit)
    // + 1 (is_paused)
    // + 1 (is_settled)
    // + 1 (is_initialized)
    pub const INIT_SPACE: usize = 8 + 32 + 32 + 32 + 1 + 8 + 8 + 8 + 1 + 1 + 1;
}

#[derive(Accounts)]
pub struct InitializeVault<'info> {
    #[account(
        init,
        payer = owner,
        space = UserVault::INIT_SPACE,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump
    )]
    pub vault: Account<'info, UserVault>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseVaultAta<'info> {
    #[account(
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    
    #[account(mut)]
    pub owner: Signer<'info>,
    
    /// The token account to close. Must be owned by vault and have zero balance.
    #[account(
        mut,
        token::authority = vault
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ManageWhitelist<'info> {
    #[account(
        mut,
        has_one = authority @ ErrorCode::Unauthorized,
        realloc = UserVault::INIT_SPACE + (vault.allowed_mints.len() + 1) * 32, // Dynamic realloc
        realloc::payer = authority,
        realloc::zero = false
    )]
    pub vault: Account<'info, UserVault>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct TogglePause<'info> {
    #[account(
        mut,
        has_one = authority @ ErrorCode::Unauthorized
    )]
    pub vault: Account<'info, UserVault>,
    pub authority: Signer<'info>,
}


#[derive(Accounts)]
pub struct DepositSol<'info> {
    #[account(
        mut,
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WithdrawSol<'info> {
    #[account(
        mut,
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositToken<'info> {
    #[account(
        has_one = owner @ ErrorCode::Unauthorized
    )]
    pub vault: Account<'info, UserVault>,
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(mut)]
    pub owner_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct WithdrawToken<'info> {
    #[account(
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(mut)]
    pub owner_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct InitVaultAta<'info> {
    #[account(
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    #[account(mut)]
    pub owner: Signer<'info>,
    
    pub mint: Account<'info, Mint>,

    #[account(
        init_if_needed,
        payer = owner,
        associated_token::mint = mint,
        associated_token::authority = vault
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ExecuteSwap<'info> {
    // GlobalConfig for legacy trading check
    #[account(
        seeds = [b"global_config"],
        bump
    )]
    pub global_config: Account<'info, GlobalConfig>,

    #[account(
        has_one = authority @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", vault.owner.as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    pub authority: Signer<'info>,
    
    // Explicit Input Token Account for Validation
    // MUST be owned by the Vault to prevent spending funds from other accounts.
    #[account(
        mut,
        token::authority = vault
    )]
    pub token_account_in: Account<'info, TokenAccount>,

    // Explicit Output Token Account for Validation
    // MUST be owned by the Vault to ensure funds stay in the system.
    #[account(
        mut,
        token::authority = vault
    )]
    pub token_account_out: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub platform_fee_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    
    /// CHECK: Jupiter Program ID checked in instruction.
    /// We strictly enforce the Mainnet Jupiter ID or Devnet Memo ID here.
    #[account(address = JUPITER_PROGRAM_ID)]
    pub jupiter_program: AccountInfo<'info>,

    /// CHECK: Instruction introspection sysvar
    #[account(address = anchor_lang::solana_program::sysvar::instructions::ID)]
    pub sysvar_instructions: AccountInfo<'info>,
}

#[error_code]
pub enum ErrorCode {
    #[msg("You are not authorized to perform this action.")]
    Unauthorized,
    #[msg("The vault is currently paused.")]
    Paused,
    #[msg("Swap output did not result in an increase in the vault's output token balance.")]
    InvalidSwapOutput,
    #[msg("Token is not allowed in this vault (not Base Asset or Whitelisted).")]
    TokenNotAllowed,
    #[msg("Invalid topology: Swap must start or end with the Base Asset.")]
    InvalidSwapTopology,
    #[msg("Invalid Fee Destination. Platform fee wallet mismatch.")]
    InvalidFeeDestination,
    #[msg("Slippage Exceeded. Amount received is less than min_amount_out.")]
    SlippageExceeded,
    #[msg("Fee Evasion Detected. Actual amount spent > declared amount_in.")]
    FeeEvasion,
    #[msg("Instruction data too short for arguments + logic.")]
    InvalidInstructionData,
    #[msg("TraderState must be paused to close.")]
    TraderNotPaused,
    #[msg("TraderState must be active to swap.")]
    TraderPaused,
    #[msg("Funds must be fully settled in Base Asset before withdrawal.")]
    NotSettled,
    #[msg("Funds held are less than current value. Insolvency risk or logic error.")]
    InsufficientFunds,
    #[msg("Mint mismatch. TraderState must hold Base Asset to settle.")]
    MintMismatch,
    #[msg("Legacy trading is disabled. Use TraderState execution.")]
    LegacyTradingDisabled,
    #[msg("Cannot close account with non-zero balance.")]
    NonZeroBalance,
    // Phase 7 error codes
    #[msg("Token account not owned by TraderState.")]
    InvalidTokenAccountOwner,
    #[msg("TraderState not initialized. Complete portfolio sync first.")]
    TraderNotInitialized,
    #[msg("TraderState already initialized.")]
    AlreadyInitialized,
}

#[derive(Accounts)]
pub struct CreateTraderState<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    
    /// CHECK: Used as seed for TraderState.
    pub trader: UncheckedAccount<'info>,
    
    #[account(
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    
    #[account(
        init,
        payer = owner,
        space = TraderState::INIT_SPACE,
        seeds = [b"trader_state", owner.key().as_ref(), trader.key().as_ref()],
        bump
    )]
    pub trader_state: Account<'info, TraderState>,
    
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = vault
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    #[account(
        init,
        payer = owner,
        associated_token::mint = mint,
        associated_token::authority = trader_state
    )]
    pub trader_token_account: Account<'info, TokenAccount>,
    
    #[account(address = vault.base_mint)]
    pub mint: Account<'info, Mint>,
    
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct UpdateTraderState<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(
        mut,
        has_one = owner @ ErrorCode::Unauthorized
    )]
    pub trader_state: Account<'info, TraderState>,
}

#[derive(Accounts)]
pub struct CloseTraderState<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(
        mut,
        close = owner,
        has_one = owner @ ErrorCode::Unauthorized,
        has_one = vault @ ErrorCode::Unauthorized
    )]
    pub trader_state: Account<'info, TraderState>,
    
    #[account(
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    
    #[account(
        mut,
        associated_token::mint = vault.base_mint,
        associated_token::authority = trader_state
    )]
    pub trader_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        associated_token::mint = vault.base_mint,
        associated_token::authority = vault
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ExecuteTraderSwap<'info> {
    #[account(mut)]
    pub authority: Signer<'info>, // Backend agent

    #[account(
        seeds = [b"user_vault_v1", trader_state.owner.as_ref()],
        bump = vault.bump,
        has_one = authority @ ErrorCode::Unauthorized,
    )]
    pub vault: Account<'info, UserVault>,

    #[account(
        mut,
        has_one = vault @ ErrorCode::Unauthorized,
        seeds = [b"trader_state", trader_state.owner.as_ref(), trader_state.trader.as_ref()],
        bump = trader_state.bump
    )]
    pub trader_state: Account<'info, TraderState>,

    #[account(mut)]
    pub input_token_account: Account<'info, TokenAccount>, // Owned by TraderState

    #[account(mut)]
    pub output_token_account: Account<'info, TokenAccount>, // Owned by TraderState

    #[account(mut)]
    pub platform_fee_account: Account<'info, TokenAccount>,

    #[account(
        seeds = [b"global_config"],
        bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,

    /// CHECK: Validated by Jupiter CPI or Memo check
    pub jupiter_program: UncheckedAccount<'info>,
    
    pub token_program: Program<'info, Token>,
    
    /// CHECK: Instructions sysvar for introspection
    #[account(address = sysvar::instructions::ID)]
    pub instructions: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct SettleTraderState<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,

    #[account(
        mut,
        has_one = owner @ ErrorCode::Unauthorized,
        has_one = vault @ ErrorCode::Unauthorized,
        seeds = [b"trader_state", owner.key().as_ref(), trader_state.trader.as_ref()],
        bump = trader_state.bump
    )]
    pub trader_state: Account<'info, TraderState>,
    
    // Explicit Token Account for Validation
    // Must be holding Base Asset (vault.base_mint)
    #[account(
        associated_token::mint = vault.base_mint,
        associated_token::authority = trader_state
    )]
    pub trader_token_account: Account<'info, TokenAccount>,
}

#[derive(Accounts)]
pub struct WithdrawTraderState<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,

    #[account(
        mut,
        close = owner,
        has_one = owner @ ErrorCode::Unauthorized,
        has_one = vault @ ErrorCode::Unauthorized,
        seeds = [b"trader_state", owner.key().as_ref(), trader_state.trader.as_ref()],
        bump = trader_state.bump
    )]
    pub trader_state: Account<'info, TraderState>,
    
    // Source: TraderState ATA
    #[account(
        mut,
        associated_token::mint = vault.base_mint,
        associated_token::authority = trader_state
    )]
    pub trader_token_account: Account<'info, TokenAccount>,
    
    // Transit: UserVault ATA
    #[account(
        mut,
        associated_token::mint = vault.base_mint,
        associated_token::authority = vault
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    // Destination: Owner Wallet ATA
    #[account(
        mut,
        associated_token::mint = vault.base_mint,
        associated_token::authority = owner
    )]
    pub owner_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

// =========================================================================
// PHASE 7: Multi-Asset Support Account Contexts
// =========================================================================

/// Phase 7A: Create additional ATA for TraderState to hold non-base assets.
#[derive(Accounts)]
pub struct CreateTraderAta<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"trader_state", owner.key().as_ref(), trader_state.trader.as_ref()],
        bump = trader_state.bump
    )]
    pub trader_state: Account<'info, TraderState>,
    
    pub mint: Account<'info, Mint>,
    
    #[account(
        init_if_needed,
        payer = owner,
        associated_token::mint = mint,
        associated_token::authority = trader_state
    )]
    pub trader_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// Phase 7D: Mark TraderState as initialized after portfolio sync.
/// Can be called by owner OR backend authority.
#[derive(Accounts)]
pub struct MarkTraderInitialized<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,  // Can be owner OR authority
    
    #[account(
        seeds = [b"user_vault_v1", trader_state.owner.as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    
    #[account(
        mut,
        has_one = vault @ ErrorCode::Unauthorized,
        seeds = [b"trader_state", trader_state.owner.as_ref(), trader_state.trader.as_ref()],
        bump = trader_state.bump
    )]
    pub trader_state: Account<'info, TraderState>,
}

/// Phase 7.1: Close a TraderState ATA to reclaim rent.
/// Owner-only. Requires is_paused = true all checked in instruction.
#[derive(Accounts)]
pub struct CloseTraderAtaContext<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"trader_state", owner.key().as_ref(), trader_state.trader.as_ref()],
        bump = trader_state.bump
    )]
    pub trader_state: Account<'info, TraderState>,
    
    /// The token account to close. Must be owned by TraderState.
    #[account(
        mut,
        token::authority = trader_state
    )]
    pub trader_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}
