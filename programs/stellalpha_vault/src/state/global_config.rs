use anchor_lang::prelude::*;

/// Global configuration for the protocol.
/// Admin-controlled settings for fees and feature flags.
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

/// Event emitted when legacy trading is toggled.
#[event]
pub struct LegacyTradingToggled {
    pub enabled: bool,
    pub admin: Pubkey,
}
