use anchor_lang::prelude::*;
pub use cp_amm;
pub use cp_amm::const_pda;
pub use cp_amm::constants;
pub use cp_amm::cpi;

/// CP-AMM program ID (local fork for testing)
pub const CP_AMM_PROGRAM_ID: Pubkey = cp_amm::ID;

/// Treasury PDA seeds helper
pub fn treasury_seeds<'a>(vault_seed: &'a str, quote_mint: &'a Pubkey) -> [&'a [u8]; 3] {
    [vault_seed.as_bytes(), b"treasury", quote_mint.as_ref()]
}

/// Derive treasury PDA address
pub fn derive_treasury_pda(vault_seed: &str, quote_mint: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[vault_seed.as_bytes(), b"treasury", quote_mint.as_ref()],
        program_id,
    )
}

// Re-export CP-AMM types for convenience
pub use cp_amm::state::{Pool, Position};

/// Validate quote-only position based on tick range and pool state
/// 
/// For CP-AMM pools, quote-only positions must be positioned to only collect fees in the quote token:
/// - If quote is token_b, position must be above price (tick_lower > 0) - only A->B swaps generate fees
/// - If quote is token_a, position must be below price (tick_upper < 0) - only B->A swaps generate fees
/// 
/// This ensures the position only provides liquidity in one token and only collects fees in the quote token.
pub fn validate_quote_only_position(
    pool: &Pool,
    tick_lower: i32,
    tick_upper: i32,
    quote_mint: &Pubkey,
) -> Result<()> {
    // Verify tick bounds are valid
    require!(
        tick_lower < tick_upper,
        crate::error::FeeRouterError::InvalidTickRange
    );

    // Determine which token is quote based on mint order
    let is_token_a_quote = pool.token_a_mint == *quote_mint;
    let is_token_b_quote = pool.token_b_mint == *quote_mint;

    require!(
        is_token_a_quote || is_token_b_quote,
        crate::error::FeeRouterError::InvalidQuoteMint
    );

    // Quote-only validation for CP-AMM:
    // - If token_b is quote: position must be above current price (tick_lower > 0)
    //   This means we only provide token_a liquidity, and only A->B swaps generate fees in B (quote)
    // - If token_a is quote: position must be below current price (tick_upper < 0)
    //   This means we only provide token_b liquidity, and only B->A swaps generate fees in A (quote)
    if is_token_b_quote {
        require!(
            tick_lower > 0,
            crate::error::FeeRouterError::PositionNotQuoteOnly
        );
    } else {
        require!(
            tick_upper < 0,
            crate::error::FeeRouterError::PositionNotQuoteOnly
        );
    }

    msg!(
        "Quote-only validation passed. Quote mint: {}, Range: [{}, {}]",
        quote_mint,
        tick_lower,
        tick_upper
    );

    Ok(())
}
