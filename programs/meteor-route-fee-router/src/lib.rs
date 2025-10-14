use anchor_lang::prelude::*;

pub mod error;
pub mod events;
pub mod state;
pub mod instructions;
pub mod cp_amm;
pub mod streamflow;
// Re-export account types at crate root for clean Context<T> usage
pub use instructions::{
    DistributeFees,
    InitializeHonoraryPosition,
    InitializePolicy,
    InitializeProgress,
};

#[allow(non_snake_case)]
pub(crate) mod __client_accounts_initialize_policy {
    pub use crate::instructions::__client_accounts_initialize_policy::*;
}
#[allow(non_snake_case)]
pub(crate) mod __client_accounts_initialize_progress {
    pub use crate::instructions::__client_accounts_initialize_progress::*;
}
#[allow(non_snake_case)]
pub(crate) mod __client_accounts_initialize_honorary_position {
    pub use crate::instructions::__client_accounts_initialize_honorary_position::*;
}
#[allow(non_snake_case)]
pub(crate) mod __client_accounts_distribute_fees {
    pub use crate::instructions::__client_accounts_distribute_fees::*;
}

declare_id!("BK5eKYpvFhvnVTDX6ZV6zcpkAgmKTiSi2Q8z9ypGR23E");

#[program]
pub mod meteor_route_fee_router {
    use super::*;

    /// Initialize policy configuration for fee distribution
    pub fn initialize_policy(
        ctx: Context<InitializePolicy>,
        vault_seed: String,
        investor_fee_share_bps: u16,
        daily_cap_quote_lamports: u64,
        min_payout_lamports: u64,
        policy_fund_missing_ata: bool,
        y0_total_allocation: u128,
    ) -> Result<()> {
        instructions::initialize_policy::handler(
            ctx,
            vault_seed,
            investor_fee_share_bps,
            daily_cap_quote_lamports,
            min_payout_lamports,
            policy_fund_missing_ata,
            y0_total_allocation,
        )
    }

    /// Initialize progress tracking for distribution state
    pub fn initialize_progress(
        ctx: Context<InitializeProgress>,
        vault_seed: String,
    ) -> Result<()> {
        instructions::initialize_progress::handler(ctx, vault_seed)
    }

    /// Initialize the honorary fee position with verified tick range
    pub fn initialize_honorary_position(
        ctx: Context<InitializeHonoraryPosition>,
        vault_seed: String,
        tick_lower: i32,
        tick_upper: i32,
        quote_mint: Pubkey,
    ) -> Result<()> {
        instructions::initialize_honorary_position::handler(
            ctx,
            vault_seed,
            tick_lower,
            tick_upper,
            quote_mint,
        )
    }

    /// Permissionless 24h distribution crank with base fee detection
    pub fn distribute_fees<'a, 'info: 'a>(
        ctx: Context<'a, 'a, 'a, 'info, DistributeFees<'info>>,
        vault_seed: String,
        investor_pages: Vec<InvestorPage>,
        is_final_page: bool,
    ) -> Result<()> {
        instructions::distribute_fees::handler(
            ctx,
            vault_seed,
            investor_pages,
            is_final_page,
        )
    }
}

/// Investor page data for batch processing
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct InvestorPage {
    pub page_index: u64,
    /// Hash over (page_index LE || investors[i].stream || investors[i].investor) for all i
    pub page_hash: [u8; 32],
    pub investors: Vec<InvestorData>,
}

/// Individual investor data within a page
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct InvestorData {
    /// Streamflow stream account pubkey
    pub stream: Pubkey,
    /// Investor wallet address (recipient of the stream)
    pub investor: Pubkey,
    // Note: locked_amount is now read on-chain from Streamflow, not passed from client
}
