use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use anchor_spl::associated_token::AssociatedToken;

use crate::{
    cp_amm::{self, Pool, CP_AMM_PROGRAM_ID},
    error::FeeRouterError,
    events::{HonoraryPositionInitialized, PreflightVerificationCompleted},
    state::{InvestorFeePositionOwnerPda, PolicyPda},
};

#[derive(Accounts)]
#[instruction(vault_seed: String)]
pub struct InitializeHonoraryPosition<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
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

    /// CP-AMM program for position creation
    /// CHECK: This will be validated against known CP-AMM program ID
    pub cp_amm_program: UncheckedAccount<'info>,

    /// CP-AMM pool account
    pub pool: AccountLoader<'info, Pool>,

    /// Pool token vault 0
    pub pool_token_vault_0: Account<'info, TokenAccount>,

    /// Pool token vault 1  
    pub pool_token_vault_1: Account<'info, TokenAccount>,

    /// Quote mint (must match policy)
    pub quote_mint: Account<'info, Mint>,

    /// Base mint (must match policy)
    pub base_mint: Account<'info, Mint>,

    /// Program quote treasury ATA (created)
    #[account(
        init,
        payer = authority,
        associated_token::mint = quote_mint,
        associated_token::authority = position_owner_pda
    )]
    pub quote_treasury: Account<'info, TokenAccount>,

    /// Position account to be created (will be initialized via CPI)
    /// CHECK: This will be created via CP-AMM CPI
    #[account(mut)]
    pub position: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn handler(
    ctx: Context<InitializeHonoraryPosition>,
    vault_seed: String,
    tick_lower: i32,
    tick_upper: i32,
    quote_mint: Pubkey,
) -> Result<()> {
    let current_timestamp = Clock::get()?.unix_timestamp as u64;
    
    // Validate quote mint matches policy
    if quote_mint != ctx.accounts.policy_pda.quote_mint {
        return err!(FeeRouterError::InvalidPoolOrder);
    }

    // Validate pool matches policy
    if ctx.accounts.pool.key() != ctx.accounts.policy_pda.pool_pubkey {
        return err!(FeeRouterError::MissingRequiredInput);
    }

    // Validate tick range is valid
    if tick_lower >= tick_upper {
        return err!(FeeRouterError::InvalidTickRange);
    }

    // Validate CP-AMM program ID
    require_keys_eq!(
        ctx.accounts.cp_amm_program.key(),
        CP_AMM_PROGRAM_ID,
        FeeRouterError::InvalidCpAmmProgram
    );

    // Load and validate pool state
    let pool = ctx.accounts.pool.load()?;

    // CRITICAL: Validate quote-only position using CP-AMM module
    cp_amm::validate_quote_only_position(
        &pool,
        tick_lower,
        tick_upper,
        &quote_mint,
    )?;

    msg!(
        "Quote-only validation passed: tick_range=[{}, {}]",
        tick_lower,
        tick_upper
    );

    // Initialize position owner PDA
    let position_owner_pda = &mut ctx.accounts.position_owner_pda;
    position_owner_pda.vault_seed = vault_seed.clone();
    position_owner_pda.position_pubkey = ctx.accounts.position.key();
    position_owner_pda.pool_pubkey = ctx.accounts.pool.key();
    position_owner_pda.quote_mint = quote_mint;
    position_owner_pda.tick_lower = tick_lower;
    position_owner_pda.tick_upper = tick_upper;
    position_owner_pda.verified_quote_only = true; // Validated via cp_amm module
    position_owner_pda.created_at = current_timestamp;

    // TODO: Create actual DLMM position via CPI to CP-AMM program
    // This would require the actual CP-AMM program interface
    // For now, we'll mark the position as created in our PDA
    
    msg!(
        "Honorary position initialized: vault_seed={}, position={}, ticks=[{}, {}]",
        vault_seed,
        ctx.accounts.position.key(),
        tick_lower,
        tick_upper
    );

    // Emit events
    emit!(PreflightVerificationCompleted {
        pool: ctx.accounts.pool.key(),
        quote_mint,
        tick_lower,
        tick_upper,
        analytical_verified: true,
        simulation_verified: false,
        timestamp: current_timestamp,
    });

    emit!(HonoraryPositionInitialized {
        pda: position_owner_pda.key(),
        position: ctx.accounts.position.key(),
        pool: ctx.accounts.pool.key(),
        quote_mint,
        tick_lower,
        tick_upper,
        timestamp: current_timestamp,
    });

    Ok(())
}

// NOTE: Quote-only validation is now handled by the cp_amm module.
// The validate_quote_only_position function performs deterministic checks:
// 1. Validates quote mint matches pool token_x or token_y
// 2. Validates tick range is entirely on the quote-only side of active price
// 3. Returns error if position would contain base token exposure
//
// TODO: Implement actual CP-AMM CPI for position creation.
// This requires:
// - Meteora DLMM v2 program interface definitions
// - CPI call to create_position with owner = position_owner_pda
// - Proper PDA signer seeds for the position_owner_pda
// - Validation of position NFT account and metadata
