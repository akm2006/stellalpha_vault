use anchor_lang::prelude::*;
use instructions::*;


// Re-export modules for convenience or testing access if needed
pub mod state;
pub mod instructions;
pub mod errors;
pub mod constants;

declare_id!("64XogE2RvY7g4fDp8XxWZxFTycANjDK37n88GZizm5nx");

#[program]
pub mod stellalpha_vault {
    use super::*;

    // ===================================
    // Vault Instructions
    // ===================================

    pub fn initialize_vault(ctx: Context<InitializeVault>, authority: Pubkey, base_mint: Pubkey) -> Result<()> {
        instructions::vault::initialize_vault(ctx, authority, base_mint)
    }

    pub fn initialize_global_config(ctx: Context<InitializeGlobalConfig>) -> Result<()> {
        instructions::admin::initialize_global_config(ctx)
    }

    pub fn add_allowed_mint(ctx: Context<ManageWhitelist>, mint: Pubkey) -> Result<()> {
        instructions::admin::add_allowed_mint(ctx, mint)
    }

    pub fn remove_allowed_mint(ctx: Context<ManageWhitelist>, mint: Pubkey) -> Result<()> {
        instructions::admin::remove_allowed_mint(ctx, mint)
    }

    pub fn toggle_pause(ctx: Context<TogglePause>) -> Result<()> {
        instructions::vault::toggle_pause(ctx)
    }

    /// Toggle legacy trading enabled/disabled. Admin only.
    pub fn toggle_legacy_trading(ctx: Context<AdminGlobalConfig>) -> Result<()> {
        instructions::admin::toggle_legacy_trading(ctx)
    }

    pub fn deposit_sol(ctx: Context<DepositSol>, amount: u64) -> Result<()> {
        instructions::vault::deposit_sol(ctx, amount)
    }

    pub fn withdraw_sol(ctx: Context<WithdrawSol>, amount: u64) -> Result<()> {
        instructions::vault::withdraw_sol(ctx, amount)
    }

    pub fn deposit_token(ctx: Context<DepositToken>, amount: u64) -> Result<()> {
        instructions::vault::deposit_token(ctx, amount)
    }

    pub fn withdraw_token(ctx: Context<WithdrawToken>, amount: u64) -> Result<()> {
        instructions::vault::withdraw_token(ctx, amount)
    }

    /// Close a Vault Token Account (ATA) if its balance is zero.
    /// Only the owner can close, and rent is returned to owner.
    pub fn close_vault_ata(ctx: Context<CloseVaultAta>) -> Result<()> {
        instructions::vault::close_vault_ata(ctx)
    }

    pub fn init_vault_ata(ctx: Context<InitVaultAta>) -> Result<()> {
        instructions::vault::init_vault_ata(ctx)
    }

    // =========================================================================
    // LEGACY PATH — DEPRECATED
    // Disabled by GlobalConfig.legacy_trading_enabled
    // DO NOT EXTEND. All new execution must use TraderState (execute_trader_swap).
    // =========================================================================
    pub fn execute_swap(ctx: Context<ExecuteSwap>, amount_in: u64, min_amount_out: u64) -> Result<()> {
        instructions::swap::execute_swap(ctx, amount_in, min_amount_out)
    }

    // ===================================
    // Trader Instructions
    // ===================================

    pub fn create_trader_state(ctx: Context<CreateTraderState>, amount: u64) -> Result<()> {
        instructions::trader::create_trader_state(ctx, amount)
    }

    pub fn pause_trader_state(ctx: Context<UpdateTraderState>) -> Result<()> {
        instructions::trader::pause_trader_state(ctx)
    }

    pub fn resume_trader_state(ctx: Context<UpdateTraderState>) -> Result<()> {
        instructions::trader::resume_trader_state(ctx)
    }

    pub fn close_trader_state(ctx: Context<CloseTraderState>) -> Result<()> {
        instructions::trader::close_trader_state(ctx)
    }

    // =========================================================================
    // PHASE 7: Multi-Asset Support
    // =========================================================================

    /// Phase 7A: Create additional token account for TraderState to hold non-base assets.
    /// Owner-only. No funds transferred.
    pub fn create_trader_ata(ctx: Context<CreateTraderAta>) -> Result<()> {
        instructions::trader::create_trader_ata(ctx)
    }

    /// Phase 7D: Mark TraderState as initialized after portfolio sync.
    /// Can be called by owner OR backend authority (one-time only).
    /// NOTE: Prefer finish_trader_sync for explicit sync lifecycle.
    pub fn mark_trader_initialized(ctx: Context<MarkTraderInitialized>) -> Result<()> {
        instructions::trader::mark_trader_initialized(ctx)
    }

    /// Phase 7C: Start portfolio sync phase.
    /// Backend authority only. Enables swaps without full automation.
    /// INVARIANT: Cannot start sync if already initialized (irreversible).
    pub fn start_trader_sync(ctx: Context<StartTraderSync>) -> Result<()> {
        instructions::trader::start_trader_sync(ctx)
    }

    /// Phase 7C: Finish portfolio sync and transition to automated trading.
    /// Backend authority only. is_syncing → is_initialized.
    pub fn finish_trader_sync(ctx: Context<FinishTraderSync>) -> Result<()> {
        instructions::trader::finish_trader_sync(ctx)
    }

    /// Phase 7.1: Close a non-base TraderState ATA to reclaim rent.
    /// Owner-only. Requires is_paused = true. ATA balance must be 0.
    /// Rent returned to owner.
    pub fn close_trader_ata(ctx: Context<CloseTraderAtaContext>) -> Result<()> {
        instructions::trader::close_trader_ata(ctx)
    }

    /// Execute a swap on behalf of a TraderState via Jupiter CPI.
    /// amount_in: Total amount to spend, including platform fee.
    /// min_amount_out: Minimum amount to receive (slippage protection).
    /// data: Opaque data blob for Jupiter swap instruction.
    pub fn execute_trader_swap(ctx: Context<ExecuteTraderSwap>, amount_in: u64, min_amount_out: u64, data: Vec<u8>) -> Result<()> {
        instructions::swap::execute_trader_swap(ctx, amount_in, min_amount_out, data)
    }

    /// settlement: Validate that TraderState holds only Base Asset and amount >= current_value.
    /// Locks the state as 'Settled' to enable withdrawal.
    pub fn settle_trader_state(ctx: Context<SettleTraderState>) -> Result<()> {
        instructions::trader::settle_trader_state(ctx)
    }

    /// withdraw: Exit flow.
    /// Prerequisites: Paused && Settled.
    /// Flow: TraderState -> UserVault -> User Wallet.
    /// Closes TraderState and its ATA.
    pub fn withdraw_trader_state(ctx: Context<WithdrawTraderState>) -> Result<()> {
        instructions::trader::withdraw_trader_state(ctx)
    }
}
