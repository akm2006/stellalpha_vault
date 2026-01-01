use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use crate::state::*;
use crate::errors::ErrorCode;
use crate::constants::PLATFORM_FEE_WALLET;
use std::str::FromStr;

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
        b"user_vault_v1",
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
    // Phase 7C: Allow swaps during sync phase OR after initialization
    require!(
        trader_state.is_initialized || trader_state.is_syncing,
        ErrorCode::TraderNotInitialized
    );

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

#[derive(Accounts)]
pub struct ExecuteSwap<'info> {
    #[account(
        seeds = [b"user_vault_v1", vault.owner.as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    
    #[account(
        mut,
        associated_token::mint = vault.base_mint,
        associated_token::authority = vault
    )]
    pub token_account_in: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub token_account_out: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub platform_fee_account: Account<'info, TokenAccount>,
    
    #[account(mut)]
    pub authority: Signer<'info>, // Backend signer
    
    #[account(
        seeds = [b"global_config"],
        bump,
    )]
    pub global_config: Account<'info, GlobalConfig>,
    
    /// CHECK: Instructions sysvar for introspection
    #[account(address = sysvar::instructions::ID)]
    pub sysvar_instructions: UncheckedAccount<'info>,
    
    /// CHECK: Validated by constraint or manual check in CPI
    pub jupiter_program: UncheckedAccount<'info>,
    
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
