use anchor_lang::prelude::*;

use crate::state::{PolicyPda, ProgressPda};

#[derive(Accounts)]
#[instruction(vault_seed: String)]
pub struct InitializeProgress<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [vault_seed.as_bytes(), b"policy"],
        bump,
        has_one = authority
    )]
    pub policy_pda: Account<'info, PolicyPda>,

    #[account(
        init,
        payer = authority,
        space = ProgressPda::LEN,
        seeds = [vault_seed.as_bytes(), b"progress"],
        bump
    )]
    pub progress_pda: Account<'info, ProgressPda>,

    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<InitializeProgress>,
    vault_seed: String,
) -> Result<()> {
    let progress_pda = &mut ctx.accounts.progress_pda;
    let current_timestamp = Clock::get()?.unix_timestamp as u64;

    // Initialize progress tracking with zero state
    progress_pda.vault_seed = vault_seed.clone();
    progress_pda.last_distribution_ts = 0;
    progress_pda.day_epoch = 0;
    progress_pda.cumulative_distributed_today = 0;
    progress_pda.carry_over_lamports = 0;
    progress_pda.pagination_cursor = 0;
    progress_pda.page_in_progress_flag = false;
    progress_pda.day_finalized_flag = false;
    progress_pda.total_pages_expected = 0;
    progress_pda.pages_processed_today = 0;
    progress_pda.last_claimed_quote = 0;
    progress_pda.last_claimed_base = 0;
    progress_pda.created_at = current_timestamp;
    progress_pda.updated_at = current_timestamp;

    msg!(
        "Progress PDA initialized: vault_seed={}, timestamp={}",
        vault_seed,
        current_timestamp
    );

    Ok(())
}
