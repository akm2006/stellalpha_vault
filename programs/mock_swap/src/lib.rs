use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

declare_id!("DcVa1Kxo9DCUuvj6E8eJpUv9pARdGwWTM72MCT2vC3rS");

#[program]
pub mod mock_swap {
    use super::*;

    /// Mock swap instruction for Localnet verification.
    /// Simulates a swap with deterministic 95% output ratio.
    /// 
    /// NOTE: This is for LOCALNET TESTING ONLY.
    /// 
    /// For simplicity, this mock expects SAME MINT for input and output.
    /// This is sufficient to prove:
    /// 1. CPI invocation works correctly
    /// 2. TraderState PDA can sign via invoke_signed
    /// 3. Token transfers work with PDA authority
    /// 
    /// Production swaps use Jupiter which handles cross-mint.
    pub fn swap(ctx: Context<Swap>, amount_in: u64, min_amount_out: u64) -> Result<()> {
        let authority_key = ctx.accounts.authority.key();
        
        // 1. Ownership Checks
        require!(
            ctx.accounts.input.owner == authority_key,
            MockSwapError::InvalidInputOwner
        );
        require!(
            ctx.accounts.output.owner == authority_key,
            MockSwapError::InvalidOutputOwner
        );

        // 2. Mint Check - same mint required for this mock
        require!(
            ctx.accounts.input.mint == ctx.accounts.output.mint,
            MockSwapError::MintMismatch
        );

        // 3. Deterministic Output Calculation (95% of input)
        let amount_out = amount_in
            .checked_mul(9500)
            .ok_or(MockSwapError::MathOverflow)?
            .checked_div(10000)
            .ok_or(MockSwapError::MathOverflow)?;

        // 4. Slippage Check
        require!(
            amount_out >= min_amount_out,
            MockSwapError::SlippageExceeded
        );

        // 5. Transfer from Input to Output
        let cpi_accounts = Transfer {
            from: ctx.accounts.input.to_account_info(),
            to: ctx.accounts.output.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
        );
        token::transfer(cpi_ctx, amount_out)?;

        msg!(
            "MockSwap: amount_in={}, amount_out={}, min_out={}",
            amount_in,
            amount_out,
            min_amount_out
        );

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Swap<'info> {
    /// The authority (TraderState PDA).
    /// CHECK: Authority passed via CPI from stellalpha_vault.
    pub authority: AccountInfo<'info>,

    /// Input token account. Must be owned by authority.
    #[account(mut)]
    pub input: Account<'info, TokenAccount>,

    /// Output token account. Must be owned by authority.
    #[account(mut)]
    pub output: Account<'info, TokenAccount>,

    /// SPL Token Program.
    pub token_program: Program<'info, Token>,
}

#[error_code]
pub enum MockSwapError {
    #[msg("Input token account not owned by authority.")]
    InvalidInputOwner,
    #[msg("Output token account not owned by authority.")]
    InvalidOutputOwner,
    #[msg("Input and output mints must match for this mock.")]
    MintMismatch,
    #[msg("Slippage exceeded: amount_out < min_amount_out.")]
    SlippageExceeded,
    #[msg("Math overflow in amount calculation.")]
    MathOverflow,
}
