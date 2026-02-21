use anchor_lang::prelude::*;

/// User-owned vault for holding funds.
/// Non-custodial: owner retains full withdrawal rights.
/// Authority: can execute swaps but cannot withdraw.
#[account]
pub struct UserVault {
    pub owner: Pubkey,
    pub authority: Pubkey,
    pub bump: u8,
    pub is_paused: bool,
    pub base_mint: Pubkey,
    pub allowed_mints: Vec<Pubkey>,
}

impl UserVault {
    // Initial space buffer: 8 discriminator + 32 owner + 32 authority + 1 bump + 1 paused + 32 base_mint + 4 vec_len + (32 * 10 initial capacity)
    pub const INIT_SPACE: usize = 8 + 32 + 32 + 1 + 1 + 32 + 4 + (32 * 10); 
}
