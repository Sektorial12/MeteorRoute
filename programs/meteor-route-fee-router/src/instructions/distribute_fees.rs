use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use anchor_spl::associated_token::AssociatedToken;

use crate::{
    error::FeeRouterError,
    events::{QuoteFeesClaimed, InvestorPayoutPage, CreatorPayoutDayClosed},
    state::{InvestorFeePositionOwnerPda, PolicyPda, ProgressPda, DistributionMath},
    InvestorPage,
};

#[derive(Accounts)]
#[instruction(vault_seed: String)]
pub struct DistributeFees<'info> {
    /// Crank caller (permissionless)
    #[account(mut)]
    pub crank_caller: Signer<'info>,

    /// Policy configuration
    #[account(
        seeds = PolicyPda::seeds(&vault_seed),
        bump
    )]
    pub policy_pda: Account<'info, PolicyPda>,

    /// Progress tracking
    #[account(
        mut,
        seeds = ProgressPda::seeds(&vault_seed),
        bump
    )]
    pub progress_pda: Account<'info, ProgressPda>,

    /// Position owner PDA
    #[account(
        seeds = InvestorFeePositionOwnerPda::seeds(&vault_seed),
        bump
    )]
    pub position_owner_pda: Account<'info, InvestorFeePositionOwnerPda>,

    /// Honorary position account
    /// CHECK: This will be validated against position_owner_pda.position_pubkey
    pub honorary_position: UncheckedAccount<'info>,

    /// Program quote treasury ATA
    #[account(
        mut,
        associated_token::mint = policy_pda.quote_mint,
        associated_token::authority = position_owner_pda
    )]
    pub quote_treasury: Account<'info, TokenAccount>,

    /// Creator quote ATA (destination for remainder)
    #[account(
        mut,
        token::mint = policy_pda.quote_mint
    )]
    pub creator_quote_ata: Account<'info, TokenAccount>,

    /// CP-AMM program for claiming fees
    /// CHECK: This will be validated against known CP-AMM program ID
    pub cp_amm_program: UncheckedAccount<'info>,

    /// Streamflow program for reading locked amounts
    /// CHECK: This will be validated against known Streamflow program ID  
    pub streamflow_program: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<DistributeFees>,
    vault_seed: String,
    investor_pages: Vec<InvestorPage>,
    is_final_page: bool,
) -> Result<()> {
    let current_timestamp = Clock::get()?.unix_timestamp as u64;
    let progress_pda = &mut ctx.accounts.progress_pda;
    let policy_pda = &ctx.accounts.policy_pda;

    // Validate position matches PDA
    if ctx.accounts.honorary_position.key() != ctx.accounts.position_owner_pda.position_pubkey {
        return err!(FeeRouterError::PdaSeedMismatch);
    }

    // Check if day is finalized
    if progress_pda.day_finalized_flag {
        return err!(FeeRouterError::DayAlreadyFinalized);
    }

    // Check 24h gate for new day
    if progress_pda.is_new_day(current_timestamp) {
        if !progress_pda.can_start_new_day(current_timestamp) {
            return err!(FeeRouterError::DayGateNotPassed);
        }
        progress_pda.start_new_day(current_timestamp);
        msg!("Started new distribution day: epoch={}", progress_pda.day_epoch);
    }

    // STEP 1: Claim fees from honorary position
    let (claimed_quote, claimed_base) = claim_fees_from_position(
        &ctx.accounts.cp_amm_program,
        &ctx.accounts.honorary_position,
        &ctx.accounts.quote_treasury,
        &ctx.accounts.position_owner_pda,
        &vault_seed,
    )?;

    // CRITICAL: Verify no base fees claimed
    if claimed_base > 0 {
        msg!("ERROR: Base fees detected! claimed_base={}", claimed_base);
        return err!(FeeRouterError::BaseFeeDetected);
    }

    // Update progress with claimed amounts
    progress_pda.last_claimed_quote = claimed_quote;
    progress_pda.last_claimed_base = claimed_base;

    // Emit fee claim event
    emit!(QuoteFeesClaimed {
        claimed_quote,
        claimed_base,
        position: ctx.accounts.honorary_position.key(),
        treasury_ata: ctx.accounts.quote_treasury.key(),
        timestamp: current_timestamp,
    });

    // If no quote fees claimed, still need to finalize day if this is final page
    if claimed_quote == 0 {
        if is_final_page {
            finalize_day(
                progress_pda,
                &ctx.accounts.creator_quote_ata,
                &ctx.accounts.quote_treasury,
                &ctx.accounts.position_owner_pda,
                &vault_seed,
                current_timestamp,
                0, // total_claimed
                0, // creator_payout
            )?;
        }
        return Ok(());
    }

    // STEP 2: Calculate total locked amounts across all pages
    let total_locked = calculate_total_locked_amount(&investor_pages)?;

    // STEP 3: Calculate eligible investor share
    let eligible_bps = DistributionMath::calculate_eligible_bps(
        total_locked,
        policy_pda.y0_total_allocation,
        policy_pda.investor_fee_share_bps,
    )?;

    let investor_fee_quote = DistributionMath::calculate_investor_fee_quote(
        claimed_quote,
        eligible_bps,
    )?;

    // Apply daily cap
    let capped_investor_fee_quote = DistributionMath::apply_daily_cap(
        investor_fee_quote,
        policy_pda.daily_cap_quote_lamports,
        progress_pda.cumulative_distributed_today,
    );

    msg!(
        "Distribution calculation: total_locked={}, eligible_bps={}, investor_fee_quote={}, capped={}",
        total_locked,
        eligible_bps,
        investor_fee_quote,
        capped_investor_fee_quote
    );

    // STEP 4: Process investor pages and distribute
    let mut total_distributed_this_call = 0u128;
    let mut total_dust_this_call = 0u64;
    let mut total_processed_count = 0u64;

    for investor_page in investor_pages.iter() {
        let (page_distributed, page_dust, page_processed) = process_investor_page(
            investor_page,
            total_locked,
            capped_investor_fee_quote,
            policy_pda.min_payout_lamports,
            &ctx.accounts.quote_treasury,
            &ctx.accounts.position_owner_pda,
            &vault_seed,
            current_timestamp,
        )?;

        total_distributed_this_call += page_distributed;
        total_dust_this_call += page_dust;
        total_processed_count += page_processed;

        // Emit page event
        emit!(InvestorPayoutPage {
            page_index: investor_page.page_index,
            processed_count: page_processed,
            distributed: page_distributed,
            cumulative_distributed: progress_pda.cumulative_distributed_today + total_distributed_this_call,
            carry: progress_pda.carry_over_lamports + total_dust_this_call,
            timestamp: current_timestamp,
        });
    }

    // Update progress PDA
    progress_pda.cumulative_distributed_today += total_distributed_this_call;
    progress_pda.carry_over_lamports += total_dust_this_call;
    progress_pda.pages_processed_today += investor_pages.len() as u64;
    progress_pda.updated_at = current_timestamp;

    // STEP 5: Finalize day if this is the final page
    if is_final_page {
        let creator_remainder = claimed_quote
            .saturating_sub(progress_pda.cumulative_distributed_today)
            .saturating_sub(progress_pda.carry_over_lamports as u128);

        finalize_day(
            progress_pda,
            &ctx.accounts.creator_quote_ata,
            &ctx.accounts.quote_treasury,
            &ctx.accounts.position_owner_pda,
            &vault_seed,
            current_timestamp,
            claimed_quote,
            creator_remainder,
        )?;
    }

    msg!(
        "Distribution completed: distributed={}, dust={}, processed={}",
        total_distributed_this_call,
        total_dust_this_call,
        total_processed_count
    );

    Ok(())
}

/// Claim fees from the honorary position via CP-AMM CPI
fn claim_fees_from_position(
    _cp_amm_program: &UncheckedAccount,
    _honorary_position: &UncheckedAccount,
    _quote_treasury: &Account<TokenAccount>,
    _position_owner_pda: &Account<InvestorFeePositionOwnerPda>,
    _vault_seed: &str,
) -> Result<(u128, u128)> {
    // PLACEHOLDER: In real implementation, this would:
    // 1. Make CPI call to CP-AMM program to claim fees
    // 2. Return the actual claimed amounts (quote, base)
    // 3. Ensure fees are transferred to quote_treasury
    
    // For testing/demo purposes, return mock values
    let claimed_quote = 1_000_000u128; // 1M lamports
    let claimed_base = 0u128; // Should always be 0 for quote-only
    
    msg!(
        "MOCK: Claimed fees from position - quote={}, base={}",
        claimed_quote,
        claimed_base
    );
    
    Ok((claimed_quote, claimed_base))
}

/// Calculate total locked amount across all investor pages
fn calculate_total_locked_amount(investor_pages: &[InvestorPage]) -> Result<u128> {
    let mut total_locked = 0u128;
    
    for page in investor_pages.iter() {
        for investor in page.investors.iter() {
            total_locked = total_locked
                .checked_add(investor.locked_amount)
                .ok_or(FeeRouterError::Overflow)?;
        }
    }
    
    Ok(total_locked)
}

/// Process a single investor page and distribute payouts
fn process_investor_page(
    investor_page: &InvestorPage,
    total_locked: u128,
    investor_fee_quote: u128,
    min_payout_lamports: u64,
    quote_treasury: &Account<TokenAccount>,
    position_owner_pda: &Account<InvestorFeePositionOwnerPda>,
    vault_seed: &str,
    current_timestamp: u64,
) -> Result<(u128, u64, u64)> {
    let mut page_distributed = 0u128;
    let mut page_dust = 0u64;
    let processed_count = investor_page.investors.len() as u64;

    for investor in investor_page.investors.iter() {
        // Calculate individual payout
        let raw_payout = DistributionMath::calculate_investor_payout(
            investor.locked_amount,
            total_locked,
            investor_fee_quote,
        )?;

        // Check minimum payout threshold
        if raw_payout < min_payout_lamports as u128 {
            page_dust += raw_payout as u64;
            msg!(
                "Investor {} payout {} below threshold {}, added to dust",
                investor.investor_quote_ata,
                raw_payout,
                min_payout_lamports
            );
            continue;
        }

        // Transfer payout to investor
        transfer_to_investor(
            quote_treasury,
            &investor.investor_quote_ata,
            position_owner_pda,
            vault_seed,
            raw_payout as u64,
        )?;

        page_distributed += raw_payout;
        
        msg!(
            "Paid investor {}: locked={}, payout={}",
            investor.investor_quote_ata,
            investor.locked_amount,
            raw_payout
        );
    }

    Ok((page_distributed, page_dust, processed_count))
}

/// Transfer quote tokens to an individual investor
fn transfer_to_investor(
    _quote_treasury: &Account<TokenAccount>,
    investor_ata: &Pubkey,
    _position_owner_pda: &Account<InvestorFeePositionOwnerPda>,
    _vault_seed: &str,
    amount: u64,
) -> Result<()> {
    // PLACEHOLDER: In real implementation, this would:
    // 1. Validate investor ATA exists (create if policy allows)
    // 2. Make SPL token transfer from quote_treasury to investor_ata
    // 3. Use position_owner_pda as authority with proper seeds
    
    msg!(
        "MOCK: Transfer {} lamports from treasury to investor {}",
        amount,
        investor_ata
    );
    
    // Real implementation would use:
    // let seeds = &[
    //     vault_seed.as_bytes(),
    //     b"investor_fee_pos_owner",
    //     &[bump],
    // ];
    // let signer = &[&seeds[..]];
    // 
    // let cpi_accounts = Transfer {
    //     from: quote_treasury.to_account_info(),
    //     to: investor_ata_account.to_account_info(),
    //     authority: position_owner_pda.to_account_info(),
    // };
    // let cpi_program = token_program.to_account_info();
    // let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
    // token::transfer(cpi_ctx, amount)?;
    
    Ok(())
}

/// Finalize the distribution day and transfer remainder to creator
fn finalize_day(
    progress_pda: &mut ProgressPda,
    creator_quote_ata: &Account<TokenAccount>,
    quote_treasury: &Account<TokenAccount>,
    position_owner_pda: &Account<InvestorFeePositionOwnerPda>,
    vault_seed: &str,
    current_timestamp: u64,
    total_claimed: u128,
    creator_payout: u128,
) -> Result<()> {
    // Transfer remainder to creator if > 0
    if creator_payout > 0 {
        transfer_to_creator(
            quote_treasury,
            creator_quote_ata,
            position_owner_pda,
            vault_seed,
            creator_payout as u64,
        )?;
    }

    // Mark day as finalized
    progress_pda.finalize_day(current_timestamp, total_claimed, creator_payout);

    // Emit day closed event
    emit!(CreatorPayoutDayClosed {
        day_epoch: progress_pda.day_epoch,
        total_claimed,
        total_distributed: progress_pda.cumulative_distributed_today,
        creator_payout,
        carry: progress_pda.carry_over_lamports,
        timestamp: current_timestamp,
    });

    msg!(
        "Day finalized: epoch={}, total_claimed={}, distributed={}, creator_payout={}, carry={}",
        progress_pda.day_epoch,
        total_claimed,
        progress_pda.cumulative_distributed_today,
        creator_payout,
        progress_pda.carry_over_lamports
    );

    Ok(())
}

/// Transfer remainder to creator
fn transfer_to_creator(
    _quote_treasury: &Account<TokenAccount>,
    creator_quote_ata: &Account<TokenAccount>,
    _position_owner_pda: &Account<InvestorFeePositionOwnerPda>,
    _vault_seed: &str,
    amount: u64,
) -> Result<()> {
    // PLACEHOLDER: Similar to transfer_to_investor but to creator
    msg!(
        "MOCK: Transfer {} lamports from treasury to creator {}",
        amount,
        creator_quote_ata.key()
    );
    
    Ok(())
}
