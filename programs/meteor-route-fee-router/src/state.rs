use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token};

/// Initialize policy configuration
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

    pub system_program: Program<'info, System>,
}

/// Initialize progress tracking
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

/// Policy configuration for fee distribution
#[account]
pub struct PolicyPda {
    pub vault_seed: String,
    pub authority: Pubkey,
    pub investor_fee_share_bps: u16,      // 0-10000 basis points
    pub daily_cap_quote_lamports: u64,    // 0 = no cap
    pub min_payout_lamports: u64,         // minimum payout threshold
    pub policy_fund_missing_ata: bool,    // whether to fund missing ATAs
    pub created_at: u64,
    pub updated_at: u64,
}

impl PolicyPda {
    pub const LEN: usize = 8 + // discriminator
        4 + 32 + // vault_seed (String)
        32 + // authority
        2 + // investor_fee_share_bps
        8 + // daily_cap_quote_lamports
        8 + // min_payout_lamports
        1 + // policy_fund_missing_ata
        8 + // created_at
        8 + // updated_at
        64; // padding for future fields
}

/// Progress tracking for daily distribution state
#[account]
pub struct ProgressPda {
    pub vault_seed: String,
    pub last_distribution_ts: u64,
    pub day_epoch: u64,                   // floor(timestamp / 86400)
    pub cumulative_distributed_today: u128,
    pub carry_over_lamports: u64,
    pub pagination_cursor: u64,
    pub page_in_progress_flag: bool,
    pub day_finalized_flag: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

impl ProgressPda {
    pub const LEN: usize = 8 + // discriminator
        4 + 32 + // vault_seed (String)
        8 + // last_distribution_ts
        8 + // day_epoch
        16 + // cumulative_distributed_today
        8 + // carry_over_lamports
        8 + // pagination_cursor
        1 + // page_in_progress_flag
        1 + // day_finalized_flag
        8 + // created_at
        8 + // updated_at
        64; // padding for future fields
}

/// Owner PDA for the honorary DLMM position
#[account]
pub struct InvestorFeePositionOwnerPda {
    pub vault_seed: String,
    pub position_pubkey: Pubkey,
    pub pool_pubkey: Pubkey,
    pub quote_mint: Pubkey,
    pub tick_lower: i32,
    pub tick_upper: i32,
    pub verified_quote_only: bool,
    pub created_at: u64,
}

impl InvestorFeePositionOwnerPda {
    pub const LEN: usize = 8 + // discriminator
        4 + 32 + // vault_seed (String)
        32 + // position_pubkey
        32 + // pool_pubkey
        32 + // quote_mint
        4 + // tick_lower
        4 + // tick_upper
        1 + // verified_quote_only
        8 + // created_at
        32; // padding
}

/// Distribution math utilities
pub struct DistributionMath;

impl DistributionMath {
    /// Calculate eligible investor share in basis points
    /// f_locked = locked_total / Y0, clamped to [0,1]
    /// eligible_bps = min(investor_fee_share_bps, floor(f_locked * 10000))
    pub fn calculate_eligible_bps(
        locked_total: u128,
        y0_total_allocation: u128,
        investor_fee_share_bps: u16,
    ) -> Result<u16> {
        if y0_total_allocation == 0 {
            return err!(crate::error::FeeRouterError::InvalidY0);
        }

        if locked_total > y0_total_allocation {
            return err!(crate::error::FeeRouterError::LockedExceedsAllocation);
        }

        // Calculate f_locked with precision
        let f_locked_bps = (locked_total as u128)
            .checked_mul(10000)
            .ok_or(crate::error::FeeRouterError::Overflow)?
            .checked_div(y0_total_allocation)
            .ok_or(crate::error::FeeRouterError::Overflow)?;

        let f_locked_bps_u16 = std::cmp::min(f_locked_bps, 10000) as u16;
        let eligible_bps = std::cmp::min(investor_fee_share_bps, f_locked_bps_u16);

        Ok(eligible_bps)
    }

    /// Calculate investor fee quote amount
    /// investor_fee_quote = floor(claimed_quote * eligible_bps / 10000)
    pub fn calculate_investor_fee_quote(
        claimed_quote: u128,
        eligible_bps: u16,
    ) -> Result<u128> {
        let investor_fee_quote = claimed_quote
            .checked_mul(eligible_bps as u128)
            .ok_or(crate::error::FeeRouterError::Overflow)?
            .checked_div(10000)
            .ok_or(crate::error::FeeRouterError::Overflow)?;

        Ok(investor_fee_quote)
    }

    /// Apply daily cap to investor fee quote
    pub fn apply_daily_cap(
        investor_fee_quote: u128,
        daily_cap: u64,
        cumulative_distributed_today: u128,
    ) -> u128 {
        if daily_cap == 0 {
            return investor_fee_quote; // No cap
        }

        let remaining_cap = (daily_cap as u128).saturating_sub(cumulative_distributed_today);
        std::cmp::min(investor_fee_quote, remaining_cap)
    }

    /// Calculate individual investor payout
    /// weight_i = locked_i / locked_total
    /// raw_payout_i = floor(investor_fee_quote * weight_i)
    pub fn calculate_investor_payout(
        locked_amount: u128,
        locked_total: u128,
        investor_fee_quote: u128,
    ) -> Result<u128> {
        if locked_total == 0 {
            return Ok(0);
        }

        let payout = locked_amount
            .checked_mul(investor_fee_quote)
            .ok_or(crate::error::FeeRouterError::Overflow)?
            .checked_div(locked_total)
            .ok_or(crate::error::FeeRouterError::Overflow)?;

        Ok(payout)
    }
}

/// Initialize position owner PDA
#[derive(Accounts)]
#[instruction(vault_seed: String)]
pub struct InitializeHonoraryPosition<'info> {
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
        space = InvestorFeePositionOwnerPda::LEN,
        seeds = [vault_seed.as_bytes(), b"investor_fee_pos_owner"],
        bump
    )]
    pub position_owner_pda: Account<'info, InvestorFeePositionOwnerPda>,

    /// Mock position account (in real implementation, this would be created via CP-AMM CPI)
    #[account(mut)]
    pub mock_position: Signer<'info>,

    pub system_program: Program<'info, System>,
}
