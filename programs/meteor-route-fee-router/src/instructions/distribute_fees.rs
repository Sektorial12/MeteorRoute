use anchor_lang::prelude::*;
use anchor_lang::solana_program::hash::hashv;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked},
};
use cp_amm::{
    program::CpAmm,
    state::{Pool, Position},
};

use crate::{
    cp_amm::CP_AMM_PROGRAM_ID,
    error::FeeRouterError,
    events::{QuoteFeesClaimed, InvestorPayoutPage, CreatorPayoutDayClosed},
    state::{InvestorFeePositionOwnerPda, PolicyPda, ProgressPda, DistributionMath},
    streamflow::{STREAMFLOW_PROGRAM_ID, parse_streamflow_account, validate_stream_for_investor, calculate_locked_amount},
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
        seeds = [vault_seed.as_bytes(), b"policy"],
        bump
    )]
    pub policy_pda: Account<'info, PolicyPda>,

    /// Progress tracking
    #[account(
        mut,
        seeds = [vault_seed.as_bytes(), b"progress"],
        bump
    )]
    pub progress_pda: Account<'info, ProgressPda>,

    /// Position owner PDA
    #[account(
        seeds = [vault_seed.as_bytes(), b"investor_fee_pos_owner"],
        bump
    )]
    pub position_owner_pda: Account<'info, InvestorFeePositionOwnerPda>,

    /// CP-AMM pool account
    #[account(
        has_one = token_a_mint,
        has_one = token_b_mint,
        has_one = token_a_vault,
        has_one = token_b_vault,
    )]
    pub pool: AccountLoader<'info, Pool>,

    /// Honorary position account
    #[account(mut, has_one = pool)]
    pub position: AccountLoader<'info, Position>,

    /// Position NFT account (proves ownership)
    #[account(
        token::authority = position_owner_pda
    )]
    pub position_nft_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: CP-AMM pool authority PDA
    #[account(address = cp_amm::const_pda::pool_authority::ID)]
    pub pool_authority: UncheckedAccount<'info>,

    /// Pool token A vault
    #[account(mut)]
    pub token_a_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Pool token B vault
    #[account(mut)]
    pub token_b_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Token A mint
    pub token_a_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Token B mint
    pub token_b_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Quote mint (must be either token_a or token_b)
    pub quote_mint: Box<InterfaceAccount<'info, Mint>>,

    /// Temporary token A account for receiving claimed fees
    #[account(
        init_if_needed,
        payer = crank_caller,
        associated_token::mint = token_a_mint,
        associated_token::authority = position_owner_pda,
        associated_token::token_program = token_a_program,
    )]
    pub temp_a_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Temporary token B account for receiving claimed fees
    #[account(
        init_if_needed,
        payer = crank_caller,
        associated_token::mint = token_b_mint,
        associated_token::authority = position_owner_pda,
        associated_token::token_program = token_b_program,
    )]
    pub temp_b_account: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Program quote treasury ATA (final destination for quote fees)
    #[account(
        mut,
        associated_token::mint = quote_mint,
        associated_token::authority = position_owner_pda
    )]
    pub quote_treasury: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Creator quote ATA (destination for remainder)
    #[account(
        mut,
        token::mint = quote_mint
    )]
    pub creator_quote_ata: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Streamflow program for reading locked amounts
    /// CHECK: This will be validated against known Streamflow program ID  
    pub streamflow_program: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
    pub token_a_program: Interface<'info, TokenInterface>,
    pub token_b_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    #[account(address = CP_AMM_PROGRAM_ID)]
    pub cp_amm_program: Program<'info, CpAmm>,
    /// CHECK: CP-AMM event authority PDA required for CPI events
    pub cp_amm_event_authority: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

pub fn handler<'a, 'info: 'a>(
    mut ctx: Context<'a, 'a, 'a, 'info, DistributeFees<'info>>,
    vault_seed: String,
    investor_pages: Vec<InvestorPage>,
    is_final_page: bool,
) -> Result<()> {
    let current_timestamp = Clock::get()?.unix_timestamp as u64;

    // Validate position matches PDA record
    if ctx.accounts.position.key() != ctx.accounts.position_owner_pda.position_pubkey {
        return err!(FeeRouterError::InvalidPositionOwner);
    }

    // Check if day is finalized
    if ctx.accounts.progress_pda.day_finalized_flag {
        return err!(FeeRouterError::DayAlreadyFinalized);
    }

    // Check 24h gate for new day
    if ctx.accounts.progress_pda.is_new_day(current_timestamp) {
        if !ctx.accounts.progress_pda.can_start_new_day(current_timestamp) {
            return err!(FeeRouterError::DayGateNotPassed);
        }
        ctx.accounts.progress_pda.start_new_day(current_timestamp);
        msg!("Started new distribution day: epoch={}", ctx.accounts.progress_pda.day_epoch);
    }

    // Validate CP-AMM program ID
    require_keys_eq!(
        ctx.accounts.cp_amm_program.key(),
        CP_AMM_PROGRAM_ID,
        FeeRouterError::InvalidCpAmmProgram
    );

    // Validate Streamflow program ID
    require_keys_eq!(
        ctx.accounts.streamflow_program.key(),
        STREAMFLOW_PROGRAM_ID,
        FeeRouterError::MissingRequiredInput
    );

    // Validate remaining_accounts: expect 2 accounts per investor (stream + quote ATA)
    let expected_remaining = investor_pages.iter().map(|p| p.investors.len()).sum::<usize>() * 2;
    require!(
        ctx.remaining_accounts.len() == expected_remaining,
        FeeRouterError::MissingRequiredInput
    );

    // STEP 1: Claim fees from honorary position via CP-AMM CPI
    let claimed_quote = claim_fees_from_position(&mut ctx, &vault_seed)?;

    // Update progress with claimed amounts
    ctx.accounts.progress_pda.last_claimed_quote = claimed_quote as u128;
    ctx.accounts.progress_pda.last_claimed_base = 0; // Always 0 for quote-only positions

    // Emit fee claim event
    emit!(QuoteFeesClaimed {
        claimed_quote: claimed_quote as u128,
        claimed_base: 0,
        position: ctx.accounts.position.key(),
        treasury_ata: ctx.accounts.quote_treasury.key(),
        timestamp: current_timestamp,
    });

    // If no quote fees claimed, still need to finalize day if this is final page
    if claimed_quote == 0 {
        if is_final_page {
            finalize_day(
                &mut ctx.accounts.progress_pda,
                &ctx.accounts.creator_quote_ata,
                &ctx.accounts.quote_treasury,
                &ctx.accounts.position_owner_pda,
                &ctx.accounts.quote_mint,
                &ctx.accounts.token_program,
                &vault_seed,
                ctx.bumps.position_owner_pda,
                current_timestamp,
                0, // total_claimed
                0, // creator_payout
            )?;
        }
        return Ok(());
    }

    // Enforce pagination invariants: pages must be contiguous starting at the cursor
    let cursor = ctx.accounts.progress_pda.pagination_cursor;
    if !investor_pages.is_empty() {
        let mut expected = cursor;
        for page in investor_pages.iter() {
            require!(
                page.page_index == expected,
                FeeRouterError::InvalidPaginationState
            );

            // Verify page hash: H( page_index_le || investors[i].stream || investors[i].investor )
            let index_le = page.page_index.to_le_bytes();
            let mut chunks: Vec<&[u8]> = Vec::with_capacity(1 + page.investors.len() * 2);
            chunks.push(&index_le);
            for inv in page.investors.iter() {
                chunks.push(inv.stream.as_ref());
                chunks.push(inv.investor.as_ref());
            }
            let computed = hashv(&chunks);
            require!(
                page.page_hash == computed.to_bytes(),
                FeeRouterError::InvalidPaginationState
            );
            expected = expected
                .checked_add(1)
                .ok_or(FeeRouterError::Overflow)?;
        }
    }

    // STEP 2: Calculate total locked amounts by reading Streamflow accounts
    let total_locked = calculate_total_locked_from_streamflow(
        &investor_pages,
        &ctx.remaining_accounts,
        &ctx.accounts.streamflow_program.key(),
    )?;

    // STEP 3: Calculate eligible investor share
    let eligible_bps = DistributionMath::calculate_eligible_bps(
        total_locked,
        ctx.accounts.policy_pda.y0_total_allocation,
        ctx.accounts.policy_pda.investor_fee_share_bps,
    )?;

    let investor_fee_quote = DistributionMath::calculate_investor_fee_quote(
        claimed_quote as u128,
        eligible_bps,
    )?;

    // Apply daily cap
    let capped_investor_fee_quote = DistributionMath::apply_daily_cap(
        investor_fee_quote,
        ctx.accounts.policy_pda.daily_cap_quote_lamports,
        ctx.accounts.progress_pda.cumulative_distributed_today,
    );

    msg!(
        "Distribution calculation: total_locked={}, eligible_bps={}, investor_fee_quote={}, capped={}",
        total_locked,
        eligible_bps,
        investor_fee_quote,
        capped_investor_fee_quote
    );

    // Set day targets if this is the first page of the day
    if ctx.accounts.progress_pda.pages_processed_today == 0 {
        let creator_remainder = (claimed_quote as u64)
            .saturating_sub(capped_investor_fee_quote as u64);
        
        ctx.accounts.progress_pda.set_day_targets(
            total_locked as u64,
            capped_investor_fee_quote as u64,
            creator_remainder,
        );
        
        msg!(
            "Day targets set: total_locked={}, investor_pool={}, creator_remainder={}",
            total_locked,
            capped_investor_fee_quote,
            creator_remainder
        );
    }

    // STEP 4: Process investor pages with Streamflow validation
    let mut total_distributed_this_call = 0u128;
    let mut total_dust_this_call = 0u64;
    let mut total_processed_count = 0u64;
    let mut remaining_accounts_index = 0usize;

    for page in investor_pages.iter() {
        let (page_distributed, page_dust, page_processed) = process_investor_page(
            page,
            total_locked,
            capped_investor_fee_quote,
            ctx.accounts.policy_pda.min_payout_lamports,
            &ctx.accounts.quote_treasury,
            &ctx.accounts.position_owner_pda,
            &ctx.accounts.quote_mint,
            &ctx.accounts.token_program,
            &vault_seed,
            ctx.bumps.position_owner_pda,
            current_timestamp,
            &ctx.remaining_accounts,
            &mut remaining_accounts_index,
            &ctx.accounts.streamflow_program.key(),
        )?;

        total_distributed_this_call += page_distributed;
        total_dust_this_call += page_dust;
        total_processed_count += page_processed;

        emit!(InvestorPayoutPage {
            page_index: total_processed_count / page_processed,
            investors_processed: page_processed as u32,
            successful_transfers: page_processed as u32,
            failed_transfers: 0,
            total_distributed: page_distributed,
            ata_creation_cost: 0,
            timestamp: current_timestamp,
        });
    }

    // Update progress PDA with investor distribution tracking
    ctx.accounts.progress_pda.cumulative_distributed_today += total_distributed_this_call;
    ctx.accounts.progress_pda.carry_over_lamports += total_dust_this_call;
    ctx.accounts.progress_pda.pages_processed_today += investor_pages.len() as u64;
    ctx.accounts.progress_pda.add_investor_distribution(total_distributed_this_call as u64)?;
    ctx.accounts.progress_pda.updated_at = current_timestamp;
    // Advance pagination cursor
    ctx.accounts.progress_pda.pagination_cursor = ctx
        .accounts
        .progress_pda
        .pagination_cursor
        .checked_add(investor_pages.len() as u64)
        .ok_or(FeeRouterError::Overflow)?;
    
    // Validate we haven't exceeded the day's investor pool target
    require!(
        ctx.accounts.progress_pda.day_investor_distributed <= ctx.accounts.progress_pda.day_investor_pool_target,
        FeeRouterError::Overflow
    );

    // STEP 5: Finalize day if this is the final page
    if is_final_page {
        // On finalization, either set expected total pages (if unset) or validate it matches
        if ctx.accounts.progress_pda.total_pages_expected == 0 {
            ctx.accounts.progress_pda.total_pages_expected = ctx.accounts.progress_pda.pagination_cursor;
        } else {
            require!(
                ctx.accounts.progress_pda.total_pages_expected == ctx.accounts.progress_pda.pagination_cursor,
                FeeRouterError::InvalidPaginationState
            );
        }

        let creator_remainder = (claimed_quote as u128)
            .saturating_sub(ctx.accounts.progress_pda.cumulative_distributed_today)
            .saturating_sub(ctx.accounts.progress_pda.carry_over_lamports as u128);

        finalize_day(
            &mut ctx.accounts.progress_pda,
            &ctx.accounts.creator_quote_ata,
            &ctx.accounts.quote_treasury,
            &ctx.accounts.position_owner_pda,
            &ctx.accounts.quote_mint,
            &ctx.accounts.token_program,
            &vault_seed,
            ctx.bumps.position_owner_pda,
            current_timestamp,
            claimed_quote as u128,
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
/// 
/// This function:
/// 1. Validates CP-AMM event authority PDA
/// 2. Calls CP-AMM claim_position_fee instruction with PDA signer
/// 3. Reloads temp accounts to get claimed amounts
/// 4. Validates quote-only (base_amount must be 0)
/// 5. Transfers quote fees to treasury
/// 6. Returns claimed quote amount
fn claim_fees_from_position<'a, 'info: 'a>(
    ctx: &mut Context<'a, 'a, 'a, 'info, DistributeFees<'info>>,
    vault_seed: &str,
) -> Result<u64> {
    // Validate CP-AMM event authority PDA
    let (expected_event_authority, _) =
        Pubkey::find_program_address(&[b"__event_authority"], &CP_AMM_PROGRAM_ID);
    require_keys_eq!(
        ctx.accounts.cp_amm_event_authority.key(),
        expected_event_authority,
        FeeRouterError::InvalidCpAmmPda
    );

    // Prepare PDA signer seeds
    let position_owner_bump = ctx.bumps.position_owner_pda;
    let seeds = &[
        vault_seed.as_bytes(),
        b"investor_fee_pos_owner",
        &[position_owner_bump],
    ];
    let signer = &[&seeds[..]];

    // Build CPI accounts for claim_position_fee
    let cpi_accounts = cp_amm::cpi::accounts::ClaimPositionFeeCtx {
        pool_authority: ctx.accounts.pool_authority.to_account_info(),
        pool: ctx.accounts.pool.to_account_info(),
        position: ctx.accounts.position.to_account_info(),
        token_a_account: ctx.accounts.temp_a_account.to_account_info(),
        token_b_account: ctx.accounts.temp_b_account.to_account_info(),
        token_a_vault: ctx.accounts.token_a_vault.to_account_info(),
        token_b_vault: ctx.accounts.token_b_vault.to_account_info(),
        token_a_mint: ctx.accounts.token_a_mint.to_account_info(),
        token_b_mint: ctx.accounts.token_b_mint.to_account_info(),
        position_nft_account: ctx.accounts.position_nft_account.to_account_info(),
        owner: ctx.accounts.position_owner_pda.to_account_info(),
        token_a_program: ctx.accounts.token_a_program.to_account_info(),
        token_b_program: ctx.accounts.token_b_program.to_account_info(),
        event_authority: ctx.accounts.cp_amm_event_authority.to_account_info(),
        program: ctx.accounts.cp_amm_program.to_account_info(),
    };

    // Execute CPI call with PDA signer
    let cpi_program = ctx.accounts.cp_amm_program.to_account_info();
    let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
    cp_amm::cpi::claim_position_fee(cpi_ctx)?;

    // Reload temp accounts to get claimed amounts
    ctx.accounts.temp_a_account.reload()?;
    ctx.accounts.temp_b_account.reload()?;

    let fee_a_amount = ctx.accounts.temp_a_account.amount;
    let fee_b_amount = ctx.accounts.temp_b_account.amount;

    // Determine which is quote and which is base
    let (quote_amount, base_amount) = if ctx.accounts.quote_mint.key() == ctx.accounts.token_b_mint.key() {
        (fee_b_amount, fee_a_amount)
    } else {
        (fee_a_amount, fee_b_amount)
    };

    // CRITICAL: Enforce quote-only - fail if any base fees claimed
    require!(
        base_amount == 0,
        FeeRouterError::BaseFeeDetected
    );

    // If no quote fees, return early
    if quote_amount == 0 {
        return Ok(0);
    }

    // Determine which temp account and program to use for transfer
    let (quote_source_info, quote_program_info) = if ctx.accounts.quote_mint.key() == ctx.accounts.token_b_mint.key() {
        (
            ctx.accounts.temp_b_account.to_account_info(),
            ctx.accounts.token_b_program.to_account_info(),
        )
    } else {
        (
            ctx.accounts.temp_a_account.to_account_info(),
            ctx.accounts.token_a_program.to_account_info(),
        )
    };

    // Transfer claimed quote fees to treasury
    transfer_checked(
        CpiContext::new_with_signer(
            quote_program_info,
            TransferChecked {
                from: quote_source_info,
                mint: ctx.accounts.quote_mint.to_account_info(),
                to: ctx.accounts.quote_treasury.to_account_info(),
                authority: ctx.accounts.position_owner_pda.to_account_info(),
            },
            signer,
        ),
        quote_amount,
        ctx.accounts.quote_mint.decimals,
    )?;

    msg!(
        "Claimed {} quote fees from position (base fees: 0)",
        quote_amount
    );

    Ok(quote_amount)
}

/// Calculate total locked amount by reading Streamflow accounts from remaining_accounts
/// 
/// remaining_accounts layout: [stream0, ata0, stream1, ata1, ...]
fn calculate_total_locked_from_streamflow(
    investor_pages: &[InvestorPage],
    remaining_accounts: &[AccountInfo],
    streamflow_program_id: &Pubkey,
) -> Result<u128> {
    let mut total_locked = 0u128;
    let mut remaining_iter = remaining_accounts.iter();
    
    for page in investor_pages.iter() {
        for investor_data in page.investors.iter() {
            // Get stream account (every 2nd account starting at 0)
            let stream_account_info = remaining_iter
                .next()
                .ok_or(FeeRouterError::MissingRequiredInput)?;
            
            // Skip the investor quote ATA (we'll use it later in process_investor_page)
            let _investor_quote_ata = remaining_iter
                .next()
                .ok_or(FeeRouterError::MissingRequiredInput)?;
            
            // Validate stream account key matches
            require_keys_eq!(
                stream_account_info.key(),
                investor_data.stream,
                FeeRouterError::MissingRequiredInput
            );
            
            #[cfg(not(feature = "local"))]
            {
                require_keys_eq!(
                    *stream_account_info.owner,
                    *streamflow_program_id,
                    FeeRouterError::MissingRequiredInput
                );
            }
            
            // Parse stream and validate recipient
            let stream = parse_streamflow_account(stream_account_info)?;
            validate_stream_for_investor(&stream, &investor_data.investor)?;
            
            // Calculate locked amount
            let locked_amount = calculate_locked_amount(&stream)?;
            
            total_locked = total_locked
                .checked_add(locked_amount as u128)
                .ok_or(FeeRouterError::Overflow)?;
        }
    }
    
    Ok(total_locked)
}

/// Process a single investor page and distribute payouts
/// Reads locked amounts from Streamflow on-chain
fn process_investor_page<'info>(
    investor_page: &InvestorPage,
    total_locked: u128,
    investor_fee_quote: u128,
    min_payout_lamports: u64,
    quote_treasury: &InterfaceAccount<'info, TokenAccount>,
    position_owner_pda: &Account<'info, InvestorFeePositionOwnerPda>,
    quote_mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
    vault_seed: &str,
    position_owner_bump: u8,
    _current_timestamp: u64,
    remaining_accounts: &[AccountInfo<'info>],
    remaining_accounts_index: &mut usize,
    streamflow_program_id: &Pubkey,
) -> Result<(u128, u64, u64)> {
    let mut page_distributed = 0u128;
    let mut page_dust = 0u64;
    let processed_count = investor_page.investors.len() as u64;

    for investor_data in investor_page.investors.iter() {
        // Get stream account from remaining_accounts
        let stream_account_info = remaining_accounts
            .get(*remaining_accounts_index)
            .ok_or(FeeRouterError::MissingRequiredInput)?;
        *remaining_accounts_index += 1;
        
        // Get investor quote ATA from remaining_accounts
        let investor_quote_ata_info = remaining_accounts
            .get(*remaining_accounts_index)
            .ok_or(FeeRouterError::MissingRequiredInput)?;
        *remaining_accounts_index += 1;
        
        // Validate stream account
        require_keys_eq!(
            stream_account_info.key(),
            investor_data.stream,
            FeeRouterError::MissingRequiredInput
        );
        #[cfg(not(feature = "local"))]
        {
            require_keys_eq!(
                *stream_account_info.owner,
                *streamflow_program_id,
                FeeRouterError::MissingRequiredInput
            );
        }
        
        // Parse stream and get locked amount
        let stream = parse_streamflow_account(stream_account_info)?;
        validate_stream_for_investor(&stream, &investor_data.investor)?;
        let locked_amount = calculate_locked_amount(&stream)? as u128;
        
        // Skip if no locked amount
        if locked_amount == 0 {
            msg!(
                "Investor {} has zero locked amount; skipping payout",
                investor_data.investor
            );
            continue;
        }
        
        // Calculate individual payout
        let raw_payout = DistributionMath::calculate_investor_payout(
            locked_amount,
            total_locked,
            investor_fee_quote,
        )?;

        // Check minimum payout threshold
        if raw_payout < min_payout_lamports as u128 {
            page_dust += raw_payout as u64;
            msg!(
                "Investor {} payout {} below threshold {}, added to dust",
                investor_data.investor,
                raw_payout,
                min_payout_lamports
            );
            continue;
        }

        // Transfer payout to investor via transfer_checked
        let seeds = &[
            vault_seed.as_bytes(),
            b"investor_fee_pos_owner",
            &[position_owner_bump],
        ];
        let signer = &[&seeds[..]];

        transfer_checked(
            CpiContext::new_with_signer(
                token_program.to_account_info(),
                TransferChecked {
                    from: quote_treasury.to_account_info(),
                    mint: quote_mint.to_account_info(),
                    to: investor_quote_ata_info.clone(),
                    authority: position_owner_pda.to_account_info(),
                },
                signer,
            ),
            raw_payout as u64,
            quote_mint.decimals,
        )?;

        page_distributed += raw_payout;

        msg!(
            "Paid investor {}: locked={}, payout={}",
            investor_data.investor,
            locked_amount,
            raw_payout
        );
    }

    Ok((page_distributed, page_dust, processed_count))
}

/// Finalize the distribution day and transfer remainder to creator
fn finalize_day<'info>(
    progress_pda: &mut ProgressPda,
    creator_quote_ata: &InterfaceAccount<'info, TokenAccount>,
    quote_treasury: &InterfaceAccount<'info, TokenAccount>,
    position_owner_pda: &Account<'info, InvestorFeePositionOwnerPda>,
    quote_mint: &InterfaceAccount<'info, Mint>,
    token_program: &Interface<'info, TokenInterface>,
    vault_seed: &str,
    position_owner_bump: u8,
    current_timestamp: u64,
    total_claimed: u128,
    creator_payout: u128,
) -> Result<()> {
    // Transfer remainder to creator if > 0
    if creator_payout > 0 {
        // Transfer using transfer_checked
        let seeds = &[
            vault_seed.as_bytes(),
            b"investor_fee_pos_owner",
            &[position_owner_bump],
        ];
        let signer = &[&seeds[..]];

        transfer_checked(
            CpiContext::new_with_signer(
                token_program.to_account_info(),
                TransferChecked {
                    from: quote_treasury.to_account_info(),
                    mint: quote_mint.to_account_info(),
                    to: creator_quote_ata.to_account_info(),
                    authority: position_owner_pda.to_account_info(),
                },
                signer,
            ),
            creator_payout as u64,
            quote_mint.decimals,
        )?;

        msg!(
            "Transferred {} quote tokens to creator {}",
            creator_payout,
            creator_quote_ata.key()
        );
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
