# MeteorRoute: DAMM v2 Honorary Quote-Only Fee Position + 24h Distribution Crank

## Project Overview

This Anchor-compatible module implements a permissionless fee routing system for CP‑AMM pools. It creates an "honorary" liquidity position biased toward quote‑token fees and enforces quote‑only distribution via runtime base‑fee detection (deterministic failure if any base fees are observed). Investor payouts are computed from Streamflow locked amounts.

**Architecture Flow:** [docs.flow.com](https://docs.flow.com) ← ALWAYS consult for wiring context

## Key Features

- **Quote‑Only Enforcement**: Runtime base‑fee detection; distribution aborts if base fees are observed
- **24h Distribution Crank**: Permissionless, resumable pagination system
- **Pro-Rata Distribution**: Based on Streamflow locked amounts with precise floor math
- **Dust & Cap Handling**: Carries forward small amounts and respects daily limits
- **Production Safety**: Comprehensive error handling and deterministic failure modes
 
 ## Quick Start
 
 For a step-by-step build and test guide, see `docs/WALKTHROUGH.md`.
 
 Short version:
 
 ```bash
 # Install deps
 yarn install
 
 # Build (regenerates IDL/types)
 yarn build
 
 # Run tests (Anchor manages validator)
 anchor test
 
 # Deploy (update Anchor.toml cluster config first)
 anchor deploy
```

## Local Testing & E2E

- The router enables a compile-time feature `local` by default in `programs/meteor-route-fee-router/Cargo.toml`.
  - Under `local`, `streamflow.rs` relaxes parsing and recipient validation to permit mocked Streamflow data during tests.
  - Run tests normally: `anchor test`. For strict behavior, build without defaults: `anchor build -- --no-default-features`.

- Distribution E2E (`tests/distribute-fees.e2e.ts`) specifics:
  - Router-owned position is created via CP‑AMM CPI and has zero liquidity by design; distribution may run on a zero-claim path.
  - Before calling `distribute_fees`, ensure the following PDA-owned token accounts exist (ATAs; authority = `InvestorFeePositionOwnerPda`):
    - `tempA` (for `token_a_mint`)
    - `tempB` (for `token_b_mint`)
    - `quote_treasury` (for `quote_mint`)
  - The crank no longer creates these ATAs on-chain; pre-create them idempotently in the client/tests using the Associated Token Program CreateIdempotent instruction.
  - Each investor must be provided as triples in `remainingAccounts` per page: `[stream, investor_quote_ata (writable), investor_owner (readonly)]` in that exact order.
  - CP‑AMM constants used by the program/tests:
    - `poolAuthority`: `8DKynLAktE6jBWxEqg3to6srgNegwE7EJLd9oJyVSR9B`
    - `cp_amm_event_authority`: PDA = `find_program_address(["__event_authority"])` for the CP‑AMM program ID

## PDAs & Seeds Table

| PDA | Seeds | Usage |
|-----|-------|-------|
| `InvestorFeePositionOwnerPda` | `[VAULT_SEED, vault, "investor_fee_pos_owner"]` | Owns the honorary CP‑AMM position |
| `PolicyPda` | `[VAULT_SEED, vault, "policy"]` | Stores fee share, caps, min payout config |
| `ProgressPda` | `[VAULT_SEED, vault, "progress"]` | Tracks daily distribution state & pagination |
| `QuoteTreasuryPda` | `[vault_seed, "treasury", quote_mint]` | Program-owned ATA for claimed quote fees |

## Account Wiring & Required CP-AMM Accounts

### Initialization Accounts
```rust
// Core accounts
cp_amm_program: Program<'info, CpAmm>,
pool: Account<'info, Pool>,
pool_token_vault_0: Account<'info, TokenAccount>,
pool_token_vault_1: Account<'info, TokenAccount>,
quote_mint: Account<'info, Mint>,
base_mint: Account<'info, Mint>,

// Program PDAs
investor_fee_position_owner: Account<'info, InvestorFeePositionOwnerPda>,
policy_pda: Account<'info, PolicyPda>,
progress_pda: Account<'info, ProgressPda>,
quote_treasury: Account<'info, TokenAccount>,

// System programs
system_program: Program<'info, System>,
token_program: Program<'info, Token>,
```

### Crank Accounts
```rust
// State & owner
policy_pda: Account<'info, PolicyPda>,
progress_pda: Account<'info, ProgressPda>,
position_owner_pda: Account<'info, InvestorFeePositionOwnerPda>,

// CP‑AMM
pool: Account<'info, Pool>,
position: Account<'info, Position>,
position_nft_account: Account<'info, TokenAccount>,
pool_authority: UncheckedAccount<'info>, // const PDA checked in handler
token_a_vault: Account<'info, TokenAccount>,
token_b_vault: Account<'info, TokenAccount>,
token_a_mint: Account<'info, Mint>,
token_b_mint: Account<'info, Mint>,
quote_mint: Account<'info, Mint>,

// Treasury & destination
quote_treasury: Account<'info, TokenAccount>, // ATA authority = position_owner_pda
creator_quote_ata: Account<'info, TokenAccount>,

// Programs
cp_amm_program: Program<'info, CpAmm>,
cp_amm_event_authority: UncheckedAccount<'info>,
streamflow_program: UncheckedAccount<'info>,
token_program: Program<'info, Token>,
associated_token_program: Program<'info, AssociatedToken>,
system_program: Program<'info, System>,
```

## Policy Parameters

| Parameter | Type | Description | Range |
|-----------|------|-------------|-------|
| `investor_fee_share_bps` | u16 | Base investor fee share in basis points | 0-10000 |
| `daily_cap_quote_lamports` | u64 | Optional daily distribution cap (0 = no cap) | 0-u64::MAX |
| `min_payout_lamports` | u64 | Minimum payout threshold (below = carry forward) | 0-u64::MAX |
| `policy_fund_missing_ata` | bool | Whether program funds missing investor ATAs | true/false |

## Error Codes (selected)

| Code (name) | Number | Description |
|-------------|--------|-------------|
| BaseFeeDetected | 6000 | Base token present in claim; distribution aborted |
| DayGateNotPassed | 6003 | 24h gate violated (too early for new distribution) |
| InvalidTickRange | 6010 | Tick inputs invalid for quote‑only validation |
| MissingRequiredInput | 6004 | Missing required on‑chain account or config |
| PdaSeedMismatch | 6006 | Computed PDA doesn’t match expected pubkey |
| Overflow | 6007 | Arithmetic overflow during distribution math |

## Events

### HonoraryPositionInitialized
```rust
pub struct HonoraryPositionInitialized {
    pub pda: Pubkey,
    pub position: Pubkey,
    pub pool: Pubkey,
    pub quote_mint: Pubkey,
    pub tick_lower: i32,
    pub tick_upper: i32,
    pub timestamp: u64,
}
```

### QuoteFeesClaimed
```rust
pub struct QuoteFeesClaimed {
    pub claimed_quote: u128,
    pub claimed_base: u128,
    pub position: Pubkey,
    pub treasury_ata: Pubkey,
    pub timestamp: u64,
}
```

### InvestorPayoutPage
```rust
pub struct InvestorPayoutPage {
    pub page_index: u64,
    pub investors_processed: u32,
    pub successful_transfers: u32,
    pub failed_transfers: u32,
    pub total_distributed: u128,
    pub ata_creation_cost: u64,
    pub timestamp: u64,
}
```

### CreatorPayoutDayClosed
```rust
pub struct CreatorPayoutDayClosed {
    pub day_epoch: u64,
    pub total_claimed: u128,
    pub total_distributed: u128,
    pub creator_payout: u128,
    pub carry: u64,
    pub timestamp: u64,
}
```

## Day/Pagination Semantics

### 24h Distribution Window
- **Day Epoch**: `floor(timestamp / 86400)`
- **Gate Check**: First crank requires `now >= last_distribution_ts + 86400`
- **Finalization**: After final page, day is marked complete and creator gets remainder

### Pagination Flow
1. **Start Day**: Reset cursor=0, cumulative_distributed=0, preserve carry from previous day
2. **Process Pages**: Each page processes N investors, updates cursor and cumulative totals
3. **Resume Safety**: Idempotent operations prevent double-pay on retry
4. **Final Page**: Transfers creator remainder and marks day finalized

### State Tracking (Progress PDA)
```rust
pub struct ProgressPda {
    pub last_distribution_ts: u64,
    pub day_epoch: u64,
    pub cumulative_distributed_today: u128,
    pub carry_over_lamports: u64,
    pub pagination_cursor: u64,
    pub page_in_progress_flag: bool,
    pub day_finalized_flag: bool,
    pub day_total_locked: u128,
    pub day_investor_pool_target: u128,
    pub day_investor_distributed: u128,
    pub day_creator_remainder_target: u128,
}
```

## Distribution Math

### Core Formulas (using floor arithmetic)
```
Y0 = total investor allocation at TGE
locked_total(t) = Σ locked_i(t) across all investors
f_locked(t) = locked_total(t) / Y0  [clamped to [0,1]]
eligible_bps = min(investor_fee_share_bps, floor(f_locked(t) * 10000))
investor_fee_quote = floor(claimed_quote * eligible_bps / 10000)

For each investor i:
weight_i(t) = locked_i(t) / locked_total(t)
raw_payout_i = floor(investor_fee_quote * weight_i(t))
final_payout_i = raw_payout_i >= min_payout_lamports ? raw_payout_i : 0
```

### Daily Cap Application
```
capped_investor_fee = min(investor_fee_quote, 
                         max(0, daily_cap - cumulative_distributed_today))
```

### Creator Remainder
```
creator_remainder = claimed_quote - cumulative_distributed_today - carry_over_lamports
```

## Testing

### Local Validator Setup
```bash
# Start local validator with required programs
solana-test-validator --reset \
  --bpf-program <cp_amm_program_id> <cp_amm_program.so> \
  --bpf-program <streamflow_program_id> <streamflow_program.so>
```

### Test Vectors

#### TV1: Basic Proportional Split
- `claimed_quote = 1_000_000 lamports`
- `Y0 = 10_000_000`
- `locked_total = 6_000_000` → `f_locked = 0.6` → `eligible_bps = 6000`
- `investor_fee_quote = 600_000`
- 3 investors `[3M, 2M, 1M]` → payouts `[300_000, 200_000, 100_000]`

#### TV2: Dust & Min Payout
- `claimed_quote = 1000`, `min_payout = 250`
- Equal 3-way split → `200` each → below threshold → `carry = 600`

#### TV3: All Unlocked
- `locked_total = 0` → `eligible_bps = 0` → 100% to creator

### Running Tests
```bash
# Run all tests (Anchor manages validator)
anchor test

# Or use yarn script
yarn test-local

# Run verification scripts
./scripts/preflight_simulate.sh <pool_pubkey> <tick_lower> <tick_upper>
```

## Security Considerations

- **Safe Math**: All arithmetic uses u128 intermediates with checked operations
- **CPI Validation**: Never trust CP-AMM return values without verification
- **Reentrancy Protection**: Proper account ordering and state updates
- **Quote-Only Enforcement**: Deterministic failure if base fees detected
- **Rent Exemption**: All accounts properly funded for rent exemption

## Known Limitations

- **Manual Tick Range Specification**: Requires external calculation for quote-only guarantee
- **Pagination Coordination**: External coordination needed for multi-page distributions
- **Missing Investor ATAs**: May block distribution (configurable via policy)

## Implementation Status

**Core Logic**: Implemented
- Business logic (math, gating, distribution, events) implemented and tested
- Current status: 21 passing, 2 pending, 1 E2E in progress (local Streamflow mock + ATA setup)
- Deterministic seeds, comprehensive error handling

**External Integration Points**: Wired
- CP‑AMM CPI for position creation and fee claim; SPL transfers implemented
- Streamflow parsing on‑chain (recipient check gated under `local` feature)
- Router requires PDA-owned ATAs to exist before crank (`tempA`, `tempB`, `quote_treasury`)

See `docs/INTEGRATION_GUIDE.md` for exact integration points and wiring instructions.

## Integration Steps

1. **Deploy Program**: Deploy to target cluster with proper program ID
2. **Configure Policy**: Set fee share, caps, and payout thresholds
3. **Initialize Position**: Create honorary position with verified quote-only ticks
4. **Setup Crank**: Configure automated or manual 24h distribution calls
5. **Monitor Events**: Track distributions via emitted events for accounting

## Development Setup

```bash
 # Install dependencies
 yarn install
 
 # Build program
 anchor build
 
 # Run tests
 anchor test
 
 # Deploy (update Anchor.toml cluster config first)
 anchor deploy
```
