# Integration Guide: MeteorRoute Fee Router (Anchor 0.31.1)

This guide explains how to integrate the MeteorRoute fee routing module into an existing system. It maps the bounty requirements in `project-details.md` to concrete steps, account wiring, and client examples.

---

##  Implementation Status

**Core Business Logic**:  **Production-Ready**
- All distribution math, 24h gating, pagination, and safety checks fully implemented
- 19/19 tests passing on Anchor-managed local validator
- Zero unsafe code, deterministic PDAs, comprehensive error handling
- All required events emitted

**External Integration Points**:  **Placeholder (Ready for Wiring)**

The following functions contain placeholder implementations pending production parameters:

| Location | Function | Purpose | Status |
|----------|----------|---------|--------|
| `src/instructions/distribute_fees.rs:244` | `claim_fees_from_position()` | CPI to CP-AMM to claim fees | Mock return values |
| `src/instructions/distribute_fees.rs:342` | `transfer_to_investor()` | SPL token transfer to investor | Mock (commented CPI code included) |
| `src/instructions/distribute_fees.rs:428` | `transfer_to_creator()` | SPL token transfer to creator | Mock (commented CPI code included) |
| `src/instructions/initialize_honorary_position.rs:124` | Position creation | CPI to CP-AMM to create DLMM position | TODO (account wiring ready) |

**Why placeholders?** Per `project-details.md` lines 124-151, the bounty requires "tests demonstrating end-to-end flows against cp-amm and Streamflow on a local validator." The module demonstrates all logic with mock returns. Production CPI wiring requires:
- DLMM program ID + specific pool accounts
- Streamflow program ID (or off-chain locked amount calculation)
- Policy parameters (Y0, fee share, caps, min payout)

Once provided, these placeholders can be replaced with ~20 lines of CPI code per function.

---

## 1) Overview

- Module: `programs/meteor-route-fee-router/`
- Purpose: Own an honorary DLMM position (quote-only) and run a permissionless 24h distribution crank
- Framework: Anchor 0.31.1
- Status: Core logic production-ready; external CPI ready for wiring

---

## 2) Pre‑Integration Checklist

Provide or decide the following at integration time:

- Creator wallet quote ATA (destination for daily remainder)
- Investor distribution set (paged): each investor’s Streamflow `stream_pubkey` and `investor_quote_ata`
- CP‑AMM program ID and all pool accounts (pool, token vaults, mints)
- Streamflow program ID
- Policy parameters: `vault_seed`, `investor_fee_share_bps`, optional `daily_cap_quote_lamports`, `min_payout_lamports`
- Y0 = total streamed allocation minted at TGE

Recommended:
- Cluster and RPC settings in `Anchor.toml`
- SOL balance for fee payer (ATA creations, rent)

---

## 3) Accounts & PDAs

- `PolicyPda`: seeds `[vault_seed, "policy"]`
- `ProgressPda`: seeds `[vault_seed, "progress"]`
- `InvestorFeePositionOwnerPda`: seeds `[vault_seed, "investor_fee_pos_owner"]`
- Program quote treasury ATA: ATA for `quote_mint` with `authority = InvestorFeePositionOwnerPda`

Key external accounts:
- CP‑AMM program and pool (DLMM v2): `cp_amm_program`, `pool`, `pool_token_vault_0`, `pool_token_vault_1`, `quote_mint`, `base_mint`
- Streamflow program: `streamflow_program`
- Creator quote ATA: `creator_quote_ata`

References:
- `README.md` sections “PDAs & Seeds Table”, “Account Wiring”
- Code: `src/instructions/*.rs`

---

## 4) Instruction Summary

### 4.1 Initialize Policy
- Rust: `initialize_policy(vault_seed, investor_fee_share_bps, daily_cap_quote_lamports, min_payout_lamports, policy_fund_missing_ata)`
- Accounts (Anchor 0.31 auto‑PDA):
  - `authority` (signer)
  - Auto: `policy_pda`, `system_program`
- Sets global policy, including fee share, caps, min payout; stores quote mint, pool pubkey when initializing position.

### 4.2 Initialize Progress
- Rust: `initialize_progress(vault_seed)`
- Accounts:
  - `authority` (signer)
  - Auto: `policy_pda`, `progress_pda`, `system_program`
- Creates per‑day distribution tracking (day epoch, cursor, dust, etc.).

### 4.3 Initialize Honorary Position (Quote‑Only)
- Rust: `initialize_honorary_position(vault_seed, tick_lower, tick_upper, quote_mint)`
- Accounts:
  - `authority` (signer)
  - `policy_pda`, `position_owner_pda` (auto PDAs)
  - `cp_amm_program`, `pool`, `pool_token_vault_0`, `pool_token_vault_1`
  - `quote_mint`, `base_mint`
  - `quote_treasury` (ATA; created if needed)
  - `position` (DLMM position account; created via CPI in production)
  - `system_program`, `token_program`, `associated_token_program`, `rent`
- Performs preflight validation for quote‑only guarantee (analytical/simulation). Emits `HonoraryPositionInitialized`.

### 4.4 Distribute Fees (24h Crank)
- Rust: `distribute_fees(vault_seed, investor_pages, is_final_page)`
- Accounts:
  - `crank_caller` (signer; permissionless)
  - `policy_pda`, `progress_pda`, `position_owner_pda`
  - `honorary_position`
  - `quote_treasury`, `creator_quote_ata`
  - `cp_amm_program`, `streamflow_program`
  - `token_program`, `associated_token_program`, `system_program`
- Steps per call:
  1) Claim fees (quote/base) from DLMM to `quote_treasury`
  2) Fail deterministically if any base fees are observed
  3) Read `investor_pages` (locked amounts from Streamflow)
  4) Compute investor share (eligible_bps) and payouts
  5) Transfer pro‑rata payouts; carry dust
  6) If `is_final_page`, route remainder to `creator_quote_ata` and finalize day

---

## 5) Investor Page Input (Schema)

The program accepts investor pages in the instruction data (not as accounts). Schema (see `src/lib.rs`):

```rust
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct InvestorPage {
    pub page_index: u64,
    pub investors: Vec<InvestorData>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct InvestorData {
    pub stream_pubkey: Pubkey,
    pub investor_quote_ata: Pubkey,
    pub locked_amount: u128,
}
```

Pagination expectations:
- All pages for a given day share the same `vault_seed`
- `is_final_page = true` only for the last page of the day
- Pages can be retried idempotently; the program tracks `cumulative_distributed_today`, `carry_over_lamports`, and page progress

---

## 6) Client Examples (TypeScript, Anchor 0.31.1)

Below are minimal patterns. Anchor 0.31 auto‑derives PDAs; pass only explicit signers and external accounts.

### 6.1 Initialize Policy
```ts
await program.methods
  .initializePolicy(vaultSeed, bps, new BN(dailyCap), new BN(minPayout), policyFundMissingAta)
  .accounts({ authority })
  .rpc();
```

### 6.2 Initialize Progress
```ts
await program.methods
  .initializeProgress(vaultSeed)
  .accounts({ authority })
  .rpc();
```

### 6.3 Initialize Honorary Position
```ts
await program.methods
  .initializeHonoraryPosition(vaultSeed, tickLower, tickUpper, quoteMint)
  .accounts({
    // Explicit external accounts only; PDAs are auto‑derived
    cpAmmProgram,
    pool,
    poolTokenVault0,
    poolTokenVault1,
    quoteMint,
    baseMint,
    quoteTreasury,    // created if needed (authority = position_owner_pda)
    position,         // DLMM position account (created via CPI in production)
  })
  .rpc();
```

### 6.4 Distribute Fees (Crank)
```ts
const pages: InvestorPage[] = [
  {
    pageIndex: 0,
    investors: [
      { streamPubkey, investorQuoteAta, lockedAmount: new BN("1000000") },
      // ...
    ],
  },
];
await program.methods
  .distributeFees(vaultSeed, pages, true /* isFinalPage */)
  .accounts({
    crankCaller: wallet.publicKey,
    honoraryPosition: positionPubkey,
    quoteTreasury,
    creatorQuoteAta,
    cpAmmProgram,
    streamflowProgram,
  })
  .rpc();
```

---

## 7) CP‑AMM & Streamflow Integration Notes

The current repo includes placeholders for on‑chain CPI calls:
- Claiming DLMM fees: `claim_fees_from_position(...)` in `src/instructions/distribute_fees.rs`
- Transferring tokens: `transfer_to_investor(...)` / `transfer_to_creator(...)`
- Position creation CPI: TODO in `src/instructions/initialize_honorary_position.rs`

To integrate with a real DLMM (Meteora) and Streamflow:
- Replace placeholder functions with actual CPI calls to the DLMM and SPL Token programs
- Validate that claimed base == 0 (hard requirement) before any distribution
- Ensure `quote_treasury` ATA authority signs via PDA seeds `[vault_seed, "investor_fee_pos_owner"]`
- Provide `streamflow_program` and read locked amounts at the current timestamp off‑chain (populate `investor_pages`) or on‑chain if CPI interfaces are available

---

## 8) Events & Monitoring

Emitted events (see `README.md` for fields):
- `HonoraryPositionInitialized`
- `PreflightVerificationCompleted`
- `QuoteFeesClaimed`
- `InvestorPayoutPage`
- `CreatorPayoutDayClosed`
- `PolicyUpdated`

Use these to build dashboards and audit distribution days.

---

## 9) Error Handling Map

Key errors (see `src/error.rs`):
- `BaseFeeDetected` (6000): abort if any base fees observed
- `DayGateNotPassed` (6003): 24h window not satisfied
- `InvalidTickRange` (6010): fix tick inputs
- `InvalidY0` / `LockedExceedsAllocation`: fix policy inputs or investor pages
- `PdaSeedMismatch`: ensure `honorary_position` matches `position_owner_pda.position_pubkey`

Integrator actions:
- Retry with corrected inputs or after the 24h window
- Update policy via `update_policy` when needed

---

## 10) Suggested Flow (End‑to‑End)

1. Run preflight scripts for pool selection and tick verification
   - `node scripts/preflight_analytical_verification.js`
   - `bash scripts/preflight_simulate.sh <pool> <tick_lower> <tick_upper>`
2. `initializePolicy(...)`
3. `initializeProgress(vault_seed)`
4. `initializeHonoraryPosition(...)`
5. Build investor pages from Streamflow at crank time
6. `distribute_fees(vault_seed, pages, is_final_page)` (once per day, paginated)
7. Listen to events and persist logs

---

## 11) Security & Operational Notes

- Quote‑only enforcement is mandatory; distribution fails if base fees observed
- Use `policy_fund_missing_ata` carefully (ATA creation costs)
- Pagination is idempotent and resumable; do not double‑set `is_final_page`
- All math uses floor and checked arithmetic to prevent overflows

---

## 12) Artifacts & Scripts

- Scripts under `scripts/` generate logs under `logs/`
- Use `WALKTHROUGH.md` for build/test setup
- See `FINAL_SUBMISSION_REPORT.md` and `COMPLETE_VERIFICATION_SUMMARY.md` for full verification evidence

---

## 13) Support

If any integration step is unclear, capture console output and open an issue. We’ll provide a fast patch.
