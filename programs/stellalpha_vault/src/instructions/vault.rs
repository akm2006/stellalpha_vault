use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer, CloseAccount};
use crate::state::*;
use anchor_spl::associated_token::AssociatedToken;
use crate::errors::ErrorCode;

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

pub fn toggle_pause(ctx: Context<TogglePause>) -> Result<()> {
    let vault = &mut ctx.accounts.vault;
    vault.is_paused = !vault.is_paused;
    msg!("Vault pause state toggled to: {}", vault.is_paused);
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

#[derive(Accounts)]
pub struct InitializeVault<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(
        init,
        payer = owner,
        space = UserVault::INIT_SPACE,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump
    )]
    pub vault: Account<'info, UserVault>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CloseVaultAta<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,

    #[account(
        mut,
        token::authority = vault
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct TogglePause<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(
        mut,
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
}

#[derive(Accounts)]
pub struct DepositSol<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(
        mut,
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WithdrawSol<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(
        mut,
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositToken<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    
    #[account(
        mut,
        associated_token::mint = vault.base_mint,
        associated_token::authority = vault
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        associated_token::mint = vault.base_mint,
        associated_token::authority = owner
    )]
    pub owner_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct WithdrawToken<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    
    #[account(
        mut,
        associated_token::mint = vault.base_mint,
        associated_token::authority = vault
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        associated_token::mint = vault.base_mint,
        associated_token::authority = owner
    )]
    pub owner_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct InitVaultAta<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(
        has_one = owner @ ErrorCode::Unauthorized,
        seeds = [b"user_vault_v1", owner.key().as_ref()],
        bump = vault.bump
    )]
    pub vault: Account<'info, UserVault>,
    
    /// CHECK: Validated by constraint
    pub mint: AccountInfo<'info>,
    
    #[account(
        init,
        payer = owner,
        associated_token::mint = mint,
        associated_token::authority = vault
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}
