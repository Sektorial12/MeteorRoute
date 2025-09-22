use anchor_lang::prelude::*;

#[error_code]
pub enum FeeRouterError {
    #[msg("Base token claimed during cp-amm claim; distribution aborted.")]
    BaseFeeDetected = 6000,
    
    #[msg("Pool token order vs declared quote mint mismatch.")]
    InvalidPoolOrder = 6001,
    
    #[msg("Preflight (analytical/simulation) failed or couldn't be performed.")]
    PreflightFailed = 6002,
    
    #[msg("First crank in day called before 24h since last_distribution_ts.")]
    DayGateNotPassed = 6003,
    
    #[msg("Distribution would exceed daily cap or already finalized.")]
    AlreadyDistributed = 6004,
    
    #[msg("Missing required on-chain account or config.")]
    MissingRequiredInput = 6005,
    
    #[msg("Per investor payout below min_payout_lamports; added to carry.")]
    MinPayoutNotReached = 6006,
    
    #[msg("Computed PDA does not match expected pubkey.")]
    PdaSeedMismatch = 6007,
    
    #[msg("Arithmetic overflow during distribution math.")]
    Overflow = 6008,
    
    #[msg("Insufficient lamports to create required ATAs/accounts.")]
    InsufficientRent = 6009,
    
    #[msg("Invalid tick range for quote-only fee accrual.")]
    InvalidTickRange = 6010,
    
    #[msg("Pool configuration does not support quote-only guarantee.")]
    QuoteOnlyNotGuaranteed = 6011,
    
    #[msg("Y0 (total investor allocation) cannot be zero.")]
    InvalidY0 = 6012,
    
    #[msg("Investor fee share basis points must be <= 10000.")]
    InvalidFeeShareBps = 6013,
    
    #[msg("Day already finalized, cannot process more pages.")]
    DayAlreadyFinalized = 6014,
    
    #[msg("Pagination cursor out of bounds.")]
    PaginationOutOfBounds = 6015,
    
    #[msg("SPL token transfer failed.")]
    TransferFailed = 6016,
    
    #[msg("Total locked amount exceeds Y0 allocation.")]
    LockedExceedsAllocation = 6017,
}
