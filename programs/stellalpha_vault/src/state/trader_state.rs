use anchor_lang::prelude::*;

/// Per-trader allocation managed by backend authority.
/// 
/// # Lifecycle
/// 1. Created by owner with initial funding
/// 2. Backend starts sync phase (is_syncing = true)
/// 3. Backend performs portfolio sync swaps
/// 4. Backend finishes sync (is_initialized = true)
/// 5. Automated trading enabled
/// 6. Owner pauses when ready to exit
/// 7. Backend settles to base asset
/// 8. Owner withdraws funds
/// 
/// # Authority Model
/// - Owner: create, pause, resume, close, withdraw
/// - Backend (vault.authority): start_sync, finish_sync, execute swaps, settle
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
    /// Required for automated trading (Phase 7).
    pub is_initialized: bool,

    /// True during portfolio sync phase.
    /// Backend can execute swaps during sync, but automation is disabled.
    /// Only backend authority can transition into/out of sync phase.
    pub is_syncing: bool,
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
    // + 1 (is_syncing)
    pub const INIT_SPACE: usize = 8 + 32 + 32 + 32 + 1 + 8 + 8 + 8 + 1 + 1 + 1 + 1;
}
