use anchor_lang::prelude::*;

// NOTE: Account context structs are defined in `src/instructions/*` and not duplicated here.

/// Policy configuration for fee distribution
#[account]
pub struct PolicyPda {
    pub vault_seed: String,
    pub authority: Pubkey,
    pub investor_fee_share_bps: u16,      // 0-10000 basis points
    pub daily_cap_quote_lamports: u64,    // 0 = no cap
    pub min_payout_lamports: u64,         // minimum payout threshold
    pub policy_fund_missing_ata: bool,    // whether to fund missing ATAs
    pub y0_total_allocation: u128,        // total investor allocation (Y0)
    pub quote_mint: Pubkey,               // quote token mint
    pub base_mint: Pubkey,                // base token mint
    pub pool_pubkey: Pubkey,              // CP-AMM pool
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
        16 + // y0_total_allocation
        32 + // quote_mint
        32 + // base_mint
        32 + // pool_pubkey
        8 + // created_at
        8 + // updated_at
        64; // padding for future fields

    pub fn seeds(vault_seed: &str) -> [&[u8]; 2] {
        [vault_seed.as_bytes(), b"policy"]
    }
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
    pub total_pages_expected: u64,
    pub pages_processed_today: u64,
    pub last_claimed_quote: u128,
    pub last_claimed_base: u128,
    
    // Per-day targets (Phase 5)
    pub day_total_locked: u64,            // Total locked amount at day start
    pub day_investor_pool_target: u64,    // Target investor pool for the day
    pub day_investor_distributed: u64,    // Amount distributed to investors so far
    pub day_creator_remainder_target: u64, // Target creator remainder
    
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
        8 + // total_pages_expected
        8 + // pages_processed_today
        16 + // last_claimed_quote
        16 + // last_claimed_base
        8 + // day_total_locked
        8 + // day_investor_pool_target
        8 + // day_investor_distributed
        8 + // day_creator_remainder_target
        8 + // created_at
        8 + // updated_at
        32; // padding for future fields

    pub fn seeds(vault_seed: &str) -> [&[u8]; 2] {
        [vault_seed.as_bytes(), b"progress"]
    }

    pub fn is_new_day(&self, current_ts: u64) -> bool {
        (current_ts / 86_400) > self.day_epoch
    }

    pub fn can_start_new_day(&self, current_ts: u64) -> bool {
        self.last_distribution_ts == 0 || current_ts.saturating_sub(self.last_distribution_ts) >= 86_400
    }

    pub fn start_new_day(&mut self, current_ts: u64) {
        self.day_epoch = current_ts / 86_400;
        self.cumulative_distributed_today = 0;
        self.pagination_cursor = 0;
        self.day_finalized_flag = false;
        self.pages_processed_today = 0;
        
        // Reset per-day targets
        self.day_total_locked = 0;
        self.day_investor_pool_target = 0;
        self.day_investor_distributed = 0;
        self.day_creator_remainder_target = 0;
        
        self.updated_at = current_ts;
    }
    
    /// Set the day targets after calculating total locked and distribution amounts
    pub fn set_day_targets(
        &mut self,
        total_locked: u64,
        investor_pool_target: u64,
        creator_remainder_target: u64,
    ) {
        self.day_total_locked = total_locked;
        self.day_investor_pool_target = investor_pool_target;
        self.day_creator_remainder_target = creator_remainder_target;
    }
    
    /// Track investor distribution progress
    pub fn add_investor_distribution(&mut self, amount: u64) -> Result<()> {
        self.day_investor_distributed = self.day_investor_distributed
            .checked_add(amount)
            .ok_or(crate::error::FeeRouterError::Overflow)?;
        Ok(())
    }

    pub fn finalize_day(&mut self, current_ts: u64, _total_claimed: u128, _creator_payout: u128) {
        self.day_finalized_flag = true;
        self.last_distribution_ts = current_ts;
        self.pagination_cursor = 0;
        self.updated_at = current_ts;
    }
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

    pub fn seeds(vault_seed: &str) -> [&[u8]; 2] {
        [vault_seed.as_bytes(), b"investor_fee_pos_owner"]
    }
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

// NOTE: `InitializeHonoraryPosition` Accounts is defined under `instructions/initialize_honorary_position.rs`.

#[cfg(test)]
mod tests {
    use super::*;

    fn default_progress() -> ProgressPda {
        ProgressPda {
            vault_seed: "vault".to_string(),
            last_distribution_ts: 0,
            day_epoch: 0,
            cumulative_distributed_today: 0,
            carry_over_lamports: 0,
            pagination_cursor: 0,
            page_in_progress_flag: false,
            day_finalized_flag: false,
            total_pages_expected: 0,
            pages_processed_today: 0,
            last_claimed_quote: 0,
            last_claimed_base: 0,
            day_total_locked: 0,
            day_investor_pool_target: 0,
            day_investor_distributed: 0,
            day_creator_remainder_target: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn test_eligible_bps_calc() {
        // 0/1000 -> 0 bps
        let bps0 = DistributionMath::calculate_eligible_bps(0, 1000, 9000).unwrap();
        assert_eq!(bps0, 0);

        // 600/1000 -> 6000 bps, min with 9000 -> 6000
        let bps1 = DistributionMath::calculate_eligible_bps(600, 1000, 9000).unwrap();
        assert_eq!(bps1, 6000);

        // 1000/1000 with share 10_000 -> 10_000
        let bps2 = DistributionMath::calculate_eligible_bps(1000, 1000, 10_000).unwrap();
        assert_eq!(bps2, 10_000);
    }

    #[test]
    fn test_investor_fee_and_payouts() {
        // claimed 1_000_000, eligible 9000 bps -> 900,000 investor pool
        let pool = DistributionMath::calculate_investor_fee_quote(1_000_000, 9000).unwrap();
        assert_eq!(pool, 900_000);

        // Split 60/40
        let p60 = DistributionMath::calculate_investor_payout(60, 100, pool).unwrap();
        let p40 = DistributionMath::calculate_investor_payout(40, 100, pool).unwrap();
        assert_eq!(p60, 540_000);
        assert_eq!(p40, 360_000);
        assert_eq!(p60 + p40, pool);
    }

    #[test]
    fn test_daily_cap() {
        // 900k desired, cap 800k, none distributed yet -> 800k
        let capped = DistributionMath::apply_daily_cap(900_000, 800_000, 0);
        assert_eq!(capped, 800_000);
    }

    #[test]
    fn test_progress_targets_and_distribution() {
        let mut p = default_progress();
        // Start new day resets fields
        p.start_new_day(86_400);
        assert_eq!(p.day_total_locked, 0);
        assert_eq!(p.day_investor_distributed, 0);
        assert_eq!(p.pages_processed_today, 0);

        // Set day targets
        p.set_day_targets(1_000, 900_000, 100_000);
        assert_eq!(p.day_total_locked, 1_000);
        assert_eq!(p.day_investor_pool_target, 900_000);
        assert_eq!(p.day_creator_remainder_target, 100_000);

        // Track distribution
        p.add_investor_distribution(540_000).unwrap();
        p.add_investor_distribution(360_000).unwrap();
        assert_eq!(p.day_investor_distributed, 900_000);
    }
}
