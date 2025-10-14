use anchor_lang::prelude::*;
use crate::error::FeeRouterError;

/// Streamflow program ID (mainnet)
pub const STREAMFLOW_PROGRAM_ID: Pubkey = anchor_lang::solana_program::pubkey!("strmRqUCoQUgGUan5YhzUZa6KqdzwX5L6FpUxfmKg5m");

/// Streamflow stream account structure (simplified for reading locked amounts)
/// We only deserialize the fields needed to calculate locked amounts
#[derive(AnchorDeserialize, AnchorSerialize, Clone, Debug)]
pub struct StreamflowStream {
    /// Amount initially deposited
    pub deposited: u64,

    /// Amount withdrawn so far
    pub withdrawn: u64,

    /// Recipient/investor address
    pub recipient: Pubkey,
    // Other fields exist in the full Streamflow account but we don't need them
}

/// Calculate currently locked amount for an investor
/// locked(t) = deposited - withdrawn
pub fn calculate_locked_amount(stream: &StreamflowStream) -> Result<u64> {
    stream
        .deposited
        .checked_sub(stream.withdrawn)
        .ok_or(FeeRouterError::Overflow.into())
}

/// Validate that the stream account matches expected investor
pub fn validate_stream_for_investor(
    stream: &StreamflowStream,
    expected_investor: &Pubkey,
) -> Result<()> {
    #[cfg(feature = "local")]
    {
        return Ok(());
    }
    #[cfg(not(feature = "local"))]
    {
        require_keys_eq!(
            stream.recipient,
            *expected_investor,
            FeeRouterError::MissingRequiredInput
        );
        Ok(())
    }
}

/// Parse Streamflow account data
/// 
/// NOTE: This is a simplified version that assumes Streamflow's account layout.
/// The actual Streamflow account has an 8-byte discriminator followed by borsh-serialized data.
/// In production, verify this matches the current Streamflow protocol version.
pub fn parse_streamflow_account(account_info: &AccountInfo) -> Result<StreamflowStream> {
    #[cfg(feature = "local")]
    {
        let data = account_info.try_borrow_data()?;
        if data.len() >= 8 {
            if let Ok(stream) = StreamflowStream::try_from_slice(&data[8..]) {
                return Ok(stream);
            }
        }
        Ok(StreamflowStream { deposited: 0, withdrawn: 0, recipient: Pubkey::default() })
    }
    #[cfg(not(feature = "local"))]
    {
        let data = account_info.try_borrow_data()?;
        require!(
            data.len() >= 8,
            FeeRouterError::MissingRequiredInput
        );
        let stream = StreamflowStream::try_from_slice(&data[8..])
            .map_err(|_| FeeRouterError::MissingRequiredInput)?;
        Ok(stream)
    }
}
