use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_2022::Token2022;

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

    /// CHECK: CP-AMM pool authority PDA
    #[account(address = cp_amm::const_pda::pool_authority::ID)]
    pub pool_authority: UncheckedAccount<'info>,

    /// CHECK: CP-AMM event authority PDA (for event CPI integrity)
    pub cp_amm_event_authority: UncheckedAccount<'info>,

    /// CP-AMM pool account (mutable for create_position CPI)
    #[account(mut)]
    pub pool: AccountLoader<'info, Pool>,

    /// Pool token vault 0
    pub pool_token_vault_0: Account<'info, TokenAccount>,

    /// Pool token vault 1  
    pub pool_token_vault_1: Account<'info, TokenAccount>,

    /// Quote mint (must match policy)
    pub quote_mint: Account<'info, Mint>,

    /// Base mint (must match policy)
    pub base_mint: Account<'info, Mint>,

    /// Program quote treasury ATA (created if needed)
    #[account(
        init_if_needed,
        payer = authority,
        associated_token::mint = quote_mint,
        associated_token::authority = position_owner_pda
    )]
    pub quote_treasury: Account<'info, TokenAccount>,

    /// Position NFT mint to be created by CP-AMM CPI
    #[account(mut)]
    pub position_mint: Signer<'info>,

    /// CHECK: Position NFT token account to be created by CP-AMM CPI
    #[account(mut)]
    pub position_token_account: UncheckedAccount<'info>,

    /// Position account to be created (will be initialized via CPI)
    /// CHECK: This will be created via CP-AMM CPI
    #[account(mut)]
    pub position: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
    pub token_2022_program: Program<'info, Token2022>,
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

    // Load and validate pool state (scope the borrow to drop it before CPI)
    {
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
    } // Drop pool borrow here before CPI

    // Initialize position owner PDA (short-lived mutable borrow scope)
    {
        let position_owner_pda = &mut ctx.accounts.position_owner_pda;
        position_owner_pda.vault_seed = vault_seed.clone();
        position_owner_pda.position_pubkey = ctx.accounts.position.key();
        position_owner_pda.pool_pubkey = ctx.accounts.pool.key();
        position_owner_pda.quote_mint = quote_mint;
        position_owner_pda.tick_lower = tick_lower;
        position_owner_pda.tick_upper = tick_upper;
        position_owner_pda.verified_quote_only = true; // Validated via cp_amm module
        position_owner_pda.created_at = current_timestamp;
    }

    // Derive and validate expected CP-AMM PDAs for position and its NFT account
    let (expected_position, _) = Pubkey::find_program_address(
        &[
            cp_amm::constants::seeds::POSITION_PREFIX.as_ref(),
            ctx.accounts.position_mint.key().as_ref(),
        ],
        &CP_AMM_PROGRAM_ID,
    );
    require_keys_eq!(
        ctx.accounts.position.key(),
        expected_position,
        FeeRouterError::InvalidCpAmmPda
    );

    let (expected_position_nft_account, _) = Pubkey::find_program_address(
        &[
            cp_amm::constants::seeds::POSITION_NFT_ACCOUNT_PREFIX.as_ref(),
            ctx.accounts.position_mint.key().as_ref(),
        ],
        &CP_AMM_PROGRAM_ID,
    );
    require_keys_eq!(
        ctx.accounts.position_token_account.key(),
        expected_position_nft_account,
        FeeRouterError::InvalidCpAmmPda
    );

    // Create the honorary position via CPI to CP-AMM
    // Validate CP-AMM event authority PDA
    let (expected_event_authority, _) = Pubkey::find_program_address(&[b"__event_authority"], &CP_AMM_PROGRAM_ID);
    require_keys_eq!(
        ctx.accounts.cp_amm_event_authority.key(),
        expected_event_authority,
        FeeRouterError::InvalidCpAmmPda
    );

    let cpi_accounts = cp_amm::cpi::accounts::CreatePositionCtx {
        owner: ctx.accounts.position_owner_pda.to_account_info(),
        position_nft_mint: ctx.accounts.position_mint.to_account_info(),
        position_nft_account: ctx.accounts.position_token_account.to_account_info(),
        pool: ctx.accounts.pool.to_account_info(),
        position: ctx.accounts.position.to_account_info(),
        pool_authority: ctx.accounts.pool_authority.to_account_info(),
        payer: ctx.accounts.authority.to_account_info(),
        token_program: ctx.accounts.token_2022_program.to_account_info(),
        system_program: ctx.accounts.system_program.to_account_info(),
        event_authority: ctx.accounts.cp_amm_event_authority.to_account_info(),
        program: ctx.accounts.cp_amm_program.to_account_info(),
    };

    let cpi_program = ctx.accounts.cp_amm_program.to_account_info();
    // Sign as the position owner PDA (owner of the position NFT token account)
    let owner_bump = ctx.bumps.position_owner_pda;
    let owner_seeds: [&[u8]; 3] = [
        vault_seed.as_bytes(),
        b"investor_fee_pos_owner",
        &[owner_bump],
    ];
    let signer = &[&owner_seeds[..]];

    cp_amm::cpi::create_position(CpiContext::new_with_signer(
        cpi_program,
        cpi_accounts,
        signer,
    ))?;

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
        pda: ctx.accounts.position_owner_pda.key(),
        position: ctx.accounts.position.key(),
        pool: ctx.accounts.pool.key(),
        quote_mint,
        tick_lower,
        tick_upper,
        timestamp: current_timestamp,
    });

    Ok(())
}