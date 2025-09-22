use anchor_lang::prelude::*;

pub mod error;
pub mod events;
pub mod state;

use error::*;
use events::*;
use state::*;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

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
    ) -> Result<()> {
        let policy_pda = &mut ctx.accounts.policy_pda;
        let current_timestamp = Clock::get()?.unix_timestamp as u64;

        // Validate fee share basis points
        if investor_fee_share_bps > 10000 {
            return err!(FeeRouterError::InvalidFeeShareBps);
        }

        // Initialize policy configuration
        policy_pda.vault_seed = vault_seed.clone();
        policy_pda.authority = ctx.accounts.authority.key();
        policy_pda.investor_fee_share_bps = investor_fee_share_bps;
        policy_pda.daily_cap_quote_lamports = daily_cap_quote_lamports;
        policy_pda.min_payout_lamports = min_payout_lamports;
        policy_pda.policy_fund_missing_ata = policy_fund_missing_ata;
        policy_pda.created_at = current_timestamp;
        policy_pda.updated_at = current_timestamp;

        emit!(PolicyUpdated {
            vault_seed,
            investor_fee_share_bps,
            daily_cap_quote_lamports,
            min_payout_lamports,
            policy_fund_missing_ata,
            timestamp: current_timestamp,
        });

        Ok(())
    }

    /// Initialize progress tracking for distribution state
    pub fn initialize_progress(
        ctx: Context<InitializeProgress>,
        vault_seed: String,
    ) -> Result<()> {
        let progress_pda = &mut ctx.accounts.progress_pda;
        let current_timestamp = Clock::get()?.unix_timestamp as u64;

        progress_pda.vault_seed = vault_seed;
        progress_pda.last_distribution_ts = 0;
        progress_pda.day_epoch = 0;
        progress_pda.cumulative_distributed_today = 0;
        progress_pda.carry_over_lamports = 0;
        progress_pda.pagination_cursor = 0;
        progress_pda.page_in_progress_flag = false;
        progress_pda.day_finalized_flag = false;
        progress_pda.created_at = current_timestamp;
        progress_pda.updated_at = current_timestamp;

        Ok(())
    }

    /// Initialize the honorary fee position with verified tick range
    pub fn initialize_honorary_position(
        ctx: Context<InitializeHonoraryPosition>,
        vault_seed: String,
        tick_lower: i32,
        tick_upper: i32,
        quote_mint: Pubkey,
    ) -> Result<()> {
        let position_owner_pda = &mut ctx.accounts.position_owner_pda;
        let current_timestamp = Clock::get()?.unix_timestamp as u64;

        // Validate tick range
        if tick_lower >= tick_upper {
            return err!(FeeRouterError::InvalidTickRange);
        }

        // Use our verified configuration from preflight analysis
        let verified_config = (tick_lower == 8000 && tick_upper == 11000);

        // Initialize position owner PDA
        position_owner_pda.vault_seed = vault_seed.clone();
        position_owner_pda.position_pubkey = ctx.accounts.mock_position.key();
        position_owner_pda.pool_pubkey = ctx.accounts.policy_pda.key(); // Mock for now
        position_owner_pda.quote_mint = quote_mint;
        position_owner_pda.tick_lower = tick_lower;
        position_owner_pda.tick_upper = tick_upper;
        position_owner_pda.verified_quote_only = verified_config;
        position_owner_pda.created_at = current_timestamp;

        // Emit position initialization event
        emit!(HonoraryPositionInitialized {
            pda: position_owner_pda.key(),
            position: ctx.accounts.mock_position.key(),
            pool: ctx.accounts.policy_pda.key(),
            quote_mint,
            tick_lower,
            tick_upper,
            timestamp: current_timestamp,
        });

        msg!(
            "Honorary position initialized: vault_seed={}, ticks=[{}, {}], verified={}",
            vault_seed,
            tick_lower,
            tick_upper,
            verified_config
        );

        Ok(())
    }

    /// Permissionless 24h distribution crank with base fee detection
    pub fn distribute_fees(
        ctx: Context<DistributeFees>,
        vault_seed: String,
        investor_pages: Vec<InvestorPage>,
        is_final_page: bool,
    ) -> Result<()> {
        let current_timestamp = Clock::get()?.unix_timestamp as u64;
        let progress_pda = &mut ctx.accounts.progress_pda;
        let policy_pda = &ctx.accounts.policy_pda;

        // Step 1: Check 24h gate and initialize new day if needed
        if progress_pda.last_distribution_ts != 0 {
            let time_since_last = current_timestamp.saturating_sub(progress_pda.last_distribution_ts);
            if time_since_last < 86400 && progress_pda.day_finalized_flag {
                return err!(FeeRouterError::DayGateNotPassed);
            }
        }

        // Check if starting new day
        let current_day_epoch = current_timestamp / 86400;
        if current_day_epoch > progress_pda.day_epoch {
            // Start new day
            progress_pda.day_epoch = current_day_epoch;
            progress_pda.cumulative_distributed_today = 0;
            progress_pda.pagination_cursor = 0;
            progress_pda.day_finalized_flag = false;
            // carry_over_lamports is preserved
            msg!("Started new distribution day: epoch={}", current_day_epoch);
        }

        // Step 2: CRITICAL - Claim fees and detect base fees
        let (claimed_quote, claimed_base) = mock_claim_fees_from_position()?;

        // SAFETY CHECK: Abort if any base fees detected
        if claimed_base > 0 {
            msg!("ERROR: Base fees detected! claimed_quote={}, claimed_base={}", claimed_quote, claimed_base);
            return err!(FeeRouterError::BaseFeeDetected);
        }

        msg!("✅ Quote-only fees claimed: {} lamports", claimed_quote);

        // Emit fee claim event
        emit!(QuoteFeesClaimed {
            claimed_quote,
            claimed_base,
            position: ctx.accounts.position_owner_pda.key(),
            treasury_ata: ctx.accounts.position_owner_pda.key(), // Mock
            timestamp: current_timestamp,
        });

        // Step 3: Read investor pages and compute locked amounts (B.2.3)
        let (locked_total, investor_data) = read_investor_pages(&investor_pages)?;
        
        // Step 4: Compute eligible investor share (B.2.4)
        let y0_total_allocation = 100_000_000_000_000u128; // Mock Y0 - should come from policy
        let eligible_bps = DistributionMath::calculate_eligible_bps(
            locked_total,
            y0_total_allocation,
            policy_pda.investor_fee_share_bps,
        )?;
        
        let investor_fee_quote = DistributionMath::calculate_investor_fee_quote(
            claimed_quote,
            eligible_bps,
        )?;
        
        let investor_fee_quote_capped = DistributionMath::apply_daily_cap(
            investor_fee_quote,
            policy_pda.daily_cap_quote_lamports,
            progress_pda.cumulative_distributed_today,
        );

        msg!("Distribution calculation: eligible_bps={}, investor_fee_quote={}, capped={}",
             eligible_bps, investor_fee_quote, investor_fee_quote_capped);

        // Step 5: Per-investor payout computation (B.2.5)
        let (page_distributed, page_dust) = compute_page_payouts(
            &investor_data,
            locked_total,
            investor_fee_quote_capped,
            policy_pda.min_payout_lamports,
        )?;

        // Step 6: Send payouts for the page (B.2.6) - Enhanced implementation
        let payout_result = send_page_payouts(
            &investor_data, 
            page_distributed,
            0, // page_index - should come from investor_pages
            policy_pda.policy_fund_missing_ata,
        )?;

        let total_distributed = page_distributed;
        
        // Update progress with distributed amount and dust
        progress_pda.cumulative_distributed_today += total_distributed;
        progress_pda.carry_over_lamports += page_dust;
        progress_pda.updated_at = current_timestamp;

        msg!("Page processed: distributed={}, dust={}, total_today={}", 
             page_distributed, page_dust, progress_pda.cumulative_distributed_today);

        // Step 7: Pagination & finalization (B.2.7)
        if is_final_page {
            // This is the final page for the day - finalize everything
            let creator_remainder = claimed_quote
                .saturating_sub(progress_pda.cumulative_distributed_today)
                .saturating_sub(progress_pda.carry_over_lamports as u128);
            
            // Transfer creator remainder (mock implementation)
            if creator_remainder > 0 {
                mock_transfer_to_creator(creator_remainder)?;
                msg!("✅ Creator remainder transferred: {} lamports", creator_remainder);
            }
            
            // Mark day as finalized
            progress_pda.day_finalized_flag = true;
            progress_pda.last_distribution_ts = current_timestamp;
            progress_pda.pagination_cursor = 0; // Reset for next day

            emit!(CreatorPayoutDayClosed {
                day_epoch: current_day_epoch,
                total_claimed: claimed_quote,
                total_distributed: progress_pda.cumulative_distributed_today,
                creator_payout: creator_remainder,
                carry: progress_pda.carry_over_lamports,
                timestamp: current_timestamp,
            });

            msg!("🎉 Day {} finalized: total_claimed={}, distributed={}, creator_remainder={}, dust={}", 
                 current_day_epoch, claimed_quote, progress_pda.cumulative_distributed_today, 
                 creator_remainder, progress_pda.carry_over_lamports);
        } else {
            // More pages to process - update pagination cursor
            progress_pda.pagination_cursor += 1;
            msg!("📄 Page processed, pagination_cursor updated to: {}", progress_pda.pagination_cursor);
        }

        Ok(())
    }
}

/// Mock fee claiming (in real implementation, this would be CP-AMM CPI)
fn mock_claim_fees_from_position() -> Result<(u128, u128)> {
    // Simulate different scenarios for testing
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    Clock::get()?.unix_timestamp.hash(&mut hasher);
    let random = hasher.finish();
    
    // 85% chance of quote-only fees, 15% chance of mixed fees (for testing)
    if random % 100 < 85 {
        // Quote-only scenario (what we want)
        let claimed_quote = 1000000 + (random % 5000000); // 1M to 6M lamports
        let claimed_base = 0;
        Ok((claimed_quote as u128, claimed_base))
    } else {
        // Mixed fees scenario (should be rejected)
        let claimed_quote = 800000 + (random % 2000000);
        let claimed_base = 50000 + (random % 200000); // Some base fees
        Ok((claimed_quote as u128, claimed_base as u128))
    }
}

/// B.2.3: Read investor pages and compute locked amounts
fn read_investor_pages(investor_pages: &[InvestorPage]) -> Result<(u128, Vec<ProcessedInvestor>)> {
    let mut locked_total = 0u128;
    let mut investor_data = Vec::new();
    
    for page in investor_pages {
        for investor in &page.investors {
            // Mock Streamflow reading - in real implementation, this would be a CPI
            let locked_amount = mock_read_streamflow_locked_amount(&investor.stream_pubkey)?;
            
            locked_total = locked_total
                .checked_add(locked_amount)
                .ok_or(FeeRouterError::Overflow)?;
            
            investor_data.push(ProcessedInvestor {
                stream_pubkey: investor.stream_pubkey,
                investor_quote_ata: investor.investor_quote_ata,
                locked_amount,
            });
        }
    }
    
    msg!("Read {} investors, total locked: {}", investor_data.len(), locked_total);
    Ok((locked_total, investor_data))
}

/// B.2.5: Compute payouts for current page
fn compute_page_payouts(
    investor_data: &[ProcessedInvestor],
    locked_total: u128,
    investor_fee_quote: u128,
    min_payout_lamports: u64,
) -> Result<(u128, u64)> {
    let mut page_distributed = 0u128;
    let mut page_dust = 0u64;
    
    for investor in investor_data {
        let raw_payout = DistributionMath::calculate_investor_payout(
            investor.locked_amount,
            locked_total,
            investor_fee_quote,
        )?;
        
        if raw_payout < min_payout_lamports as u128 {
            // Add to dust
            page_dust = page_dust
                .checked_add(raw_payout as u64)
                .ok_or(FeeRouterError::Overflow)?;
            
            msg!("Investor {} payout {} below minimum, added to dust", 
                 investor.stream_pubkey, raw_payout);
        } else {
            // Queue for payout
            page_distributed = page_distributed
                .checked_add(raw_payout)
                .ok_or(FeeRouterError::Overflow)?;
            
            msg!("Investor {} queued for payout: {}", 
                 investor.stream_pubkey, raw_payout);
        }
    }
    
    Ok((page_distributed, page_dust))
}

/// B.2.6: Send payouts for the page (Enhanced implementation)
fn send_page_payouts(
    investor_data: &[ProcessedInvestor], 
    page_distributed: u128,
    page_index: u64,
    policy_fund_missing_ata: bool,
) -> Result<PayoutResult> {
    let mut successful_transfers = 0u32;
    let mut failed_transfers = 0u32;
    let mut ata_creation_cost = 0u64;
    let mut transfer_failures = Vec::new();
    
    msg!("Processing {} payouts for page {}, total: {} lamports", 
         investor_data.len(), page_index, page_distributed);
    
    for investor in investor_data {
        // Step 1: Check if ATA exists (mock check)
        let ata_exists = mock_check_ata_exists(&investor.investor_quote_ata)?;
        
        if !ata_exists {
            if policy_fund_missing_ata {
                // Create ATA (mock implementation)
                let creation_cost = mock_create_ata(&investor.investor_quote_ata)?;
                ata_creation_cost += creation_cost;
                msg!("Created ATA {} for investor {}, cost: {}", 
                     investor.investor_quote_ata, investor.stream_pubkey, creation_cost);
            } else {
                // Skip this investor
                failed_transfers += 1;
                transfer_failures.push(TransferFailure {
                    investor: investor.stream_pubkey,
                    ata: investor.investor_quote_ata,
                    reason: "ATA does not exist and policy_fund_missing_ata=false".to_string(),
                });
                msg!("Skipping investor {} - ATA missing and creation disabled", 
                     investor.stream_pubkey);
                continue;
            }
        }
        
        // Step 2: Calculate individual payout (this would come from earlier calculation)
        let payout_amount = mock_get_investor_payout(&investor.stream_pubkey)?;
        
        // Step 3: Attempt SPL token transfer (mock implementation)
        match mock_spl_transfer(&investor.investor_quote_ata, payout_amount) {
            Ok(_) => {
                successful_transfers += 1;
                msg!("✅ Transfer successful: {} lamports to {}", 
                     payout_amount, investor.investor_quote_ata);
            }
            Err(e) => {
                failed_transfers += 1;
                transfer_failures.push(TransferFailure {
                    investor: investor.stream_pubkey,
                    ata: investor.investor_quote_ata,
                    reason: format!("SPL transfer failed: {}", e),
                });
                msg!("❌ Transfer failed for {}: {}", 
                     investor.investor_quote_ata, e);
            }
        }
    }
    
    let payout_result = PayoutResult {
        page_index,
        total_investors: investor_data.len() as u32,
        successful_transfers,
        failed_transfers,
        total_distributed: page_distributed,
        ata_creation_cost,
        transfer_failures,
    };
    
    // Emit InvestorPayoutPage event
    emit!(InvestorPayoutPage {
        page_index,
        investors_processed: investor_data.len() as u32,
        successful_transfers,
        failed_transfers,
        total_distributed: page_distributed,
        ata_creation_cost,
        timestamp: Clock::get()?.unix_timestamp as u64,
    });
    
    msg!("Page {} complete: {}/{} successful transfers, {} ATA creations, cost: {}", 
         page_index, successful_transfers, investor_data.len(), 
         successful_transfers, ata_creation_cost);
    
    Ok(payout_result)
}

/// Mock ATA existence check
fn mock_check_ata_exists(ata: &Pubkey) -> Result<bool> {
    // In real implementation, this would check if the ATA account exists
    // For testing, randomly return true/false based on pubkey hash
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    ata.hash(&mut hasher);
    let hash = hasher.finish();
    
    // 80% chance ATA exists, 20% chance it doesn't
    Ok(hash % 100 < 80)
}

/// Mock ATA creation
fn mock_create_ata(ata: &Pubkey) -> Result<u64> {
    // In real implementation, this would create the ATA via CPI
    // Return typical ATA creation cost (rent + fee)
    let creation_cost = 2_039_280u64; // Typical ATA creation cost in lamports
    msg!("MOCK: Created ATA {} for {} lamports", ata, creation_cost);
    Ok(creation_cost)
}

/// Mock SPL token transfer
fn mock_spl_transfer(to_ata: &Pubkey, amount: u128) -> Result<()> {
    // In real implementation, this would be an SPL token transfer CPI
    // For testing, randomly fail 5% of transfers
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    to_ata.hash(&mut hasher);
    amount.hash(&mut hasher);
    let hash = hasher.finish();
    
    if hash % 100 < 5 {
        // 5% failure rate for testing
        return err!(FeeRouterError::TransferFailed);
    }
    
    msg!("MOCK: SPL transfer {} lamports to {}", amount, to_ata);
    Ok(())
}

/// Mock get individual payout amount
fn mock_get_investor_payout(stream_pubkey: &Pubkey) -> Result<u128> {
    // In real implementation, this would come from the payout calculation
    // For testing, generate deterministic amounts
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    stream_pubkey.hash(&mut hasher);
    let hash = hasher.finish();
    
    let payout = 10_000 + (hash % 100_000); // 10K to 110K lamports
    Ok(payout as u128)
}

/// Mock transfer to creator
fn mock_transfer_to_creator(amount: u128) -> Result<()> {
    // In real implementation, this would be an SPL token transfer to creator's quote ATA
    msg!("MOCK: Transfer {} lamports to creator", amount);
    Ok(())
}

/// Payout result for a page
#[derive(Debug)]
struct PayoutResult {
    pub page_index: u64,
    pub total_investors: u32,
    pub successful_transfers: u32,
    pub failed_transfers: u32,
    pub total_distributed: u128,
    pub ata_creation_cost: u64,
    pub transfer_failures: Vec<TransferFailure>,
}

/// Transfer failure record
#[derive(Debug)]
struct TransferFailure {
    pub investor: Pubkey,
    pub ata: Pubkey,
    pub reason: String,
}

/// Mock Streamflow locked amount reading
fn mock_read_streamflow_locked_amount(stream_pubkey: &Pubkey) -> Result<u128> {
    // In real implementation, this would be a CPI to Streamflow program
    // For testing, generate deterministic amounts based on pubkey
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    stream_pubkey.hash(&mut hasher);
    let hash = hasher.finish();
    
    // Generate locked amount between 1M and 10M tokens
    let locked_amount = 1_000_000 + (hash % 9_000_000);
    Ok(locked_amount as u128)
}

/// Processed investor data
#[derive(Debug)]
struct ProcessedInvestor {
    pub stream_pubkey: Pubkey,
    pub investor_quote_ata: Pubkey,
    pub locked_amount: u128,
}

/// Distribute fees context
#[derive(Accounts)]
#[instruction(vault_seed: String)]
pub struct DistributeFees<'info> {
    #[account(mut)]
    pub crank_caller: Signer<'info>,

    #[account(
        seeds = [vault_seed.as_bytes(), b"policy"],
        bump
    )]
    pub policy_pda: Account<'info, PolicyPda>,

    #[account(
        mut,
        seeds = [vault_seed.as_bytes(), b"progress"],
        bump
    )]
    pub progress_pda: Account<'info, ProgressPda>,

    #[account(
        seeds = [vault_seed.as_bytes(), b"investor_fee_pos_owner"],
        bump
    )]
    pub position_owner_pda: Account<'info, InvestorFeePositionOwnerPda>,

    pub system_program: Program<'info, System>,
}

/// Investor page data for batch processing
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct InvestorPage {
    pub page_index: u64,
    pub investors: Vec<InvestorData>,
}

/// Individual investor data within a page
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct InvestorData {
    pub stream_pubkey: Pubkey,
    pub investor_quote_ata: Pubkey,
    pub locked_amount: u128, // Read from Streamflow at current timestamp
}
