use anchor_lang::prelude::*;

#[event]
pub struct HonoraryPositionInitialized {
    pub pda: Pubkey,
    pub position: Pubkey,
    pub pool: Pubkey,
    pub quote_mint: Pubkey,
    pub tick_lower: i32,
    pub tick_upper: i32,
    pub timestamp: u64,
}

#[event]
pub struct QuoteFeesClaimed {
    pub claimed_quote: u128,
    pub claimed_base: u128,
    pub position: Pubkey,
    pub treasury_ata: Pubkey,
    pub timestamp: u64,
}

#[event]
pub struct InvestorPayoutPage {
    pub page_index: u64,
    pub investors_processed: u32,
    pub successful_transfers: u32,
    pub failed_transfers: u32,
    pub total_distributed: u128,
    pub ata_creation_cost: u64,
    pub timestamp: u64,
}

#[event]
pub struct CreatorPayoutDayClosed {
    pub day_epoch: u64,
    pub total_claimed: u128,
    pub total_distributed: u128,
    pub creator_payout: u128,
    pub carry: u64,
    pub timestamp: u64,
}

#[event]
pub struct PolicyUpdated {
    pub vault_seed: String,
    pub investor_fee_share_bps: u16,
    pub daily_cap_quote_lamports: u64,
    pub min_payout_lamports: u64,
    pub policy_fund_missing_ata: bool,
    pub timestamp: u64,
}

#[event]
pub struct PreflightVerificationCompleted {
    pub pool: Pubkey,
    pub quote_mint: Pubkey,
    pub tick_lower: i32,
    pub tick_upper: i32,
    pub analytical_verified: bool,
    pub simulation_verified: bool,
    pub timestamp: u64,
}
