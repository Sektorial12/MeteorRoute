use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token};

use crate::{
    error::FeeRouterError,
    events::PolicyUpdated,
    state::PolicyPda,
};

#[derive(Accounts)]
#[instruction(vault_seed: String)]
pub struct InitializePolicy<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = PolicyPda::LEN,
        seeds = [vault_seed.as_bytes(), b"policy"],
        bump
    )]
    pub policy_pda: Account<'info, PolicyPda>,

    /// Quote mint - must be verified during initialization
    pub quote_mint: Account<'info, Mint>,

    /// Base mint - must be verified during initialization
    pub base_mint: Account<'info, Mint>,

    /// CP-AMM pool account (for verification)
    /// CHECK: This will be verified against CP-AMM program
    pub pool: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

pub fn handler(
    ctx: Context<InitializePolicy>,
    vault_seed: String,
    investor_fee_share_bps: u16,
    daily_cap_quote_lamports: u64,
    min_payout_lamports: u64,
    policy_fund_missing_ata: bool,
) -> Result<()> {
    // Validate fee share basis points
    if investor_fee_share_bps > 10000 {
        return err!(FeeRouterError::InvalidFeeShareBps);
    }

    // Validate mints are different
    if ctx.accounts.quote_mint.key() == ctx.accounts.base_mint.key() {
        return err!(FeeRouterError::InvalidPoolOrder);
    }

    let policy_pda = &mut ctx.accounts.policy_pda;
    let current_timestamp = Clock::get()?.unix_timestamp as u64;

    // Initialize policy configuration
    policy_pda.vault_seed = vault_seed.clone();
    policy_pda.authority = ctx.accounts.authority.key();
    policy_pda.investor_fee_share_bps = investor_fee_share_bps;
    policy_pda.daily_cap_quote_lamports = daily_cap_quote_lamports;
    policy_pda.min_payout_lamports = min_payout_lamports;
    policy_pda.policy_fund_missing_ata = policy_fund_missing_ata;
    policy_pda.y0_total_allocation = 0; // Will be set during position initialization
    policy_pda.quote_mint = ctx.accounts.quote_mint.key();
    policy_pda.base_mint = ctx.accounts.base_mint.key();
    policy_pda.pool_pubkey = ctx.accounts.pool.key();
    policy_pda.created_at = current_timestamp;
    policy_pda.updated_at = current_timestamp;

    // Emit policy creation event
    emit!(PolicyUpdated {
        vault_seed: policy_pda.vault_seed.clone(),
        investor_fee_share_bps,
        daily_cap_quote_lamports,
        min_payout_lamports,
        policy_fund_missing_ata,
        timestamp: current_timestamp,
    });

    msg!(
        "Policy initialized: vault_seed={}, fee_share={}bps, daily_cap={}, min_payout={}",
        vault_seed,
        investor_fee_share_bps,
        daily_cap_quote_lamports,
        min_payout_lamports
    );

    Ok(())
}
