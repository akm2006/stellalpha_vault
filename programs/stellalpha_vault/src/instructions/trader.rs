use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer, Mint, CloseAccount};
use anchor_spl::associated_token::AssociatedToken;
use crate::state::*;
use crate::errors::ErrorCode;

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
    trader_state.is_settled = false;
    
    // Phase 7C: Default to uninitialized
    trader_state.is_initialized = false;

    // Transfer initial funding from UserVault to TraderState
    let seeds = &[
        b"user_vault_v1",
        ctx.accounts.owner.key.as_ref(),
        &[ctx.accounts.vault.bump],
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

    let amount = ctx.accounts.trader_token_account.amount;

    // Refund vault rent + remaining funds to owner
    let cpi_accounts = Transfer {
        from: ctx.accounts.trader_token_account.to_account_info(),
        to: ctx.accounts.vault_token_account.to_account_info(),
        authority: trader_state.to_account_info(),
    };

    // Close TraderState ATA
    let seeds = &[
        b"trader_state",
        trader_state.owner.as_ref(),
        trader_state.trader.as_ref(),
        &[trader_state.bump],
    ];
    let signer = &[&seeds[..]];

    let cpi_ctx = CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), cpi_accounts, signer);
    
    if amount > 0 {
        token::transfer(cpi_ctx, amount)?;
    }

    let cpi_accounts_close = token::CloseAccount {
        account: ctx.accounts.trader_token_account.to_account_info(),
        destination: ctx.accounts.owner.to_account_info(), // Rent to owner
        authority: trader_state.to_account_info(),
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
        "Created additional TraderState ATA for mint: {}",
        ctx.accounts.mint.key()
    );
    Ok(())
}

/// Phase 7D: Mark TraderState as initialized after portfolio sync.
/// Can be called by owner OR backend authority (one-time only).
/// NOTE: Prefer finish_trader_sync for explicit sync lifecycle.
pub fn mark_trader_initialized(ctx: Context<MarkTraderInitialized>) -> Result<()> {
    let trader_state = &mut ctx.accounts.trader_state;
    let vault = &ctx.accounts.vault;
    let signer = ctx.accounts.signer.key();

    require!(
        signer == vault.owner || signer == vault.authority,
        ErrorCode::Unauthorized
    );
    require!(!trader_state.is_initialized, ErrorCode::AlreadyInitialized);

    trader_state.is_initialized = true;
    
    msg!("TraderState marked as initialized by {}", signer);
    Ok(())
}


/// Phase 7.1: Close a non-base TraderState ATA to reclaim rent.
/// Owner-only. Requires is_paused = true. ATA balance must be 0.
/// Rent returned to owner.
pub fn close_trader_ata(ctx: Context<CloseTraderAtaContext>) -> Result<()> {
    let trader_state = &ctx.accounts.trader_state;
    
    // Safety: Must be paused to prevent closing ATAs mid-trade
    require!(trader_state.is_paused, ErrorCode::TraderNotPaused);
    
    // Safety: Cannot close if there is a balance
    require!(ctx.accounts.trader_token_account.amount == 0, ErrorCode::NonZeroBalance);

    let seeds = &[
        b"trader_state",
        trader_state.owner.as_ref(),
        trader_state.trader.as_ref(),
        &[trader_state.bump],
    ];
    let signer = &[&seeds[..]];

    let cpi_accounts_close = token::CloseAccount {
        account: ctx.accounts.trader_token_account.to_account_info(),
        destination: ctx.accounts.owner.to_account_info(), // Rent returned to owner
        authority: trader_state.to_account_info(),
    };
    
    let cpi_ctx_close = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(), 
        cpi_accounts_close, 
        signer
    );
    token::close_account(cpi_ctx_close)?;
    
    msg!("Closed TraderState ATA for mint: {}. Rent returned to owner.", 
        ctx.accounts.trader_token_account.mint);
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
