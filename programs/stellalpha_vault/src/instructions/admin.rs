use anchor_lang::prelude::*;
use crate::state::*;
use crate::errors::ErrorCode;

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

#[derive(Accounts)]
pub struct ManageWhitelist<'info> {
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
