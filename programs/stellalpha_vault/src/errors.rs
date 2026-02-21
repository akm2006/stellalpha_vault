use anchor_lang::prelude::*;

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
