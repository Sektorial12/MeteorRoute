use anchor_lang::prelude::*;

use crate::{
    error::FeeRouterError,
    events::PolicyUpdated,
    state::PolicyPda,
};

#[derive(Accounts)]
#[instruction(vault_seed: String)]
pub struct UpdatePolicy<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [vault_seed.as_bytes(), b"policy"],
        bump,
        has_one = authority
    )]
    pub policy_pda: Account<'info, PolicyPda>,
}

pub fn handler(
    ctx: Context<UpdatePolicy>,
    vault_seed: String,
    new_investor_fee_share_bps: Option<u16>,
    new_daily_cap_quote_lamports: Option<u64>,
    new_min_payout_lamports: Option<u64>,
    new_policy_fund_missing_ata: Option<bool>,
) -> Result<()> {
    let policy_pda = &mut ctx.accounts.policy_pda;
    let current_timestamp = Clock::get()?.unix_timestamp as u64;
    let mut updated = false;

    // Update investor fee share if provided
    if let Some(fee_share_bps) = new_investor_fee_share_bps {
        if fee_share_bps > 10000 {
            return err!(FeeRouterError::InvalidFeeShareBps);
        }
        policy_pda.investor_fee_share_bps = fee_share_bps;
        updated = true;
        msg!("Updated investor_fee_share_bps to {}", fee_share_bps);
    }

    // Update daily cap if provided
    if let Some(daily_cap) = new_daily_cap_quote_lamports {
        policy_pda.daily_cap_quote_lamports = daily_cap;
        updated = true;
        msg!("Updated daily_cap_quote_lamports to {}", daily_cap);
    }

    // Update minimum payout if provided
    if let Some(min_payout) = new_min_payout_lamports {
        policy_pda.min_payout_lamports = min_payout;
        updated = true;
        msg!("Updated min_payout_lamports to {}", min_payout);
    }

    // Update ATA funding policy if provided
    if let Some(fund_missing_ata) = new_policy_fund_missing_ata {
        policy_pda.policy_fund_missing_ata = fund_missing_ata;
        updated = true;
        msg!("Updated policy_fund_missing_ata to {}", fund_missing_ata);
    }

    if updated {
        policy_pda.updated_at = current_timestamp;

        // Emit policy update event
        emit!(PolicyUpdated {
            vault_seed,
            investor_fee_share_bps: policy_pda.investor_fee_share_bps,
            daily_cap_quote_lamports: policy_pda.daily_cap_quote_lamports,
            min_payout_lamports: policy_pda.min_payout_lamports,
            policy_fund_missing_ata: policy_pda.policy_fund_missing_ata,
            timestamp: current_timestamp,
        });

        msg!("Policy updated successfully at timestamp {}", current_timestamp);
    } else {
        msg!("No policy changes provided");
    }

    Ok(())
}
