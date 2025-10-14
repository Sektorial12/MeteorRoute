# MeteorRoute Remediation TODO Plan

Purpose: Raise MeteorRoute to parity (or above) with `star-honorary-fee-module` by implementing missing Work A/Work B requirements, strengthening determinism, tests, and docs.

## Priority P0 (Critical)

- **[DONE] Implement CP‑AMM CPI position creation (Work A)**
  - Files: `programs/meteor-route-fee-router/src/instructions/initialize_honorary_position.rs`, `programs/meteor-route-fee-router/src/cp_amm.rs`, `programs/meteor-route-fee-router/src/state.rs`.
  - Actions:
    - Implemented CPI to CP‑AMM `create_position` with `owner = position_owner_pda`.
    - Validates CP‑AMM PDAs pre‑CPI: `pool_authority`, `__event_authority`, expected `position` PDA, and expected `position_nft_account`.
    - Ensures `quote_treasury` ATA is owned by `position_owner_pda` and persists position on PDA.
    - New required accounts on `InitializeHonoraryPosition`: `pool_authority`, `cp_amm_event_authority`, `position_mint` (Signer), `position_token_account`, `token_2022_program`.
    - Deterministic seeds unchanged for `InvestorFeePositionOwnerPda`.

- **Set and enforce Y0 total allocation (inputs + invariants)**
  - Files: `programs/meteor-route-fee-router/src/instructions/initialize_policy.rs`, `programs/meteor-route-fee-router/src/state.rs`.
  - Actions:
    - Add `y0_total_allocation` as an argument to `initialize_policy` (or set in `initialize_honorary_position`) and persist it on `PolicyPda` (non‑zero).
    - In `DistributionMath::calculate_eligible_bps()`, either require Y0 > 0 (preferred, parity with star) or clearly document fallback behavior if Y0==0.
    - Update tests to cover Y0 invariants.

- **[DONE] Robust claim flow with base‑fee rejection (Work B precondition)**
  - Files: `programs/meteor-route-fee-router/src/instructions/distribute_fees.rs` (claim path), `programs/meteor-route-fee-router/src/events.rs`, `programs/meteor-route-fee-router/src/error.rs`.
  - Actions:
    - CPI to CP‑AMM `claim_position_fee` already implemented: claims into temp ATAs and transfers quote to treasury.
    - Enforces `base_amount == 0` (deterministic failure otherwise) and emits `QuoteFeesClaimed`.

- **Pagination, idempotency, and 24h gating hardening**
  - Files: `programs/meteor-route-fee-router/src/state.rs` (`ProgressPda`), `programs/meteor-route-fee-router/src/instructions/distribute_fees.rs`.
  - Actions:
    - First page: validate 24h gate; reset per‑day fields; compute day targets; carry unused amount forward; set cursor to 0.
    - Subsequent pages: enforce `page_start == pagination_cursor`; require `total_investors/total_locked` match day’s snapshot; never overpay remaining budgets.
    - Final page: distribute creator remainder, carry investor dust, emit `CreatorPayoutDayClosed`, and reset day state.

- **[DONE] Missing investor ATA handling that does not block day close**
  - Files: `programs/meteor-route-fee-router/src/instructions/distribute_fees.rs`, `programs/meteor-route-fee-router/src/instructions/initialize_policy.rs`, `programs/meteor-route-fee-router/src/events.rs`.
  - Actions:
    - Skip path: if investor ATA is missing/invalid, add payout to dust and continue (no abort).
    - Fund path: if `policy_fund_missing_ata` is true, create missing investor ATAs via Associated Token Program using crank as payer.
    - Tracks per-page `successful_transfers`/`failed_transfers` and `ata_creation_cost` in `InvestorPayoutPage`.
    - remaining_accounts layout updated to `[stream, investor_quote_ata, investor_owner]` per investor.

## Priority P1 (High)

- **Align distribution math and state updates**
  - Files: `programs/meteor-route-fee-router/src/state.rs` (DistributionMath), `.../distribute_fees.rs`.
  - Actions:
    - Ensure exact floor arithmetic: `eligible_bps = min(policy_bps, floor(total_locked/Y0*10000))`.
    - Compute `investor_fee_quote = floor(claimed_quote * eligible_bps / 10000)`.
    - Apply daily cap before investor/creator split; track `day_investor_pool_target`, `day_creator_remainder_target`, and cumulative fields.

- **Strengthen Streamflow integration (read‑only)**
  - Files: `programs/meteor-route-fee-router/src/streamflow.rs`, `.../distribute_fees.rs`.
  - Actions:
    - Use `remaining_accounts` for per‑investor Streamflow PDAs and investor ATAs.
    - Validate `owner == STREAMFLOW_PROGRAM_ID` and recipient matches investor.
    - Compute locked = deposited ‑ withdrawn; require positive unless creator‑only day.

- **Events completeness and consistency**
  - Files: `programs/meteor-route-fee-router/src/events.rs`, emit sites in instructions.
  - Actions:
    - Emit `HonoraryPositionInitialized` after successful CP‑AMM CPI.
    - Ensure `InvestorPayoutPage` includes page indices, processed counts, totals, and ATA creation cost if funded.
    - Ensure `CreatorPayoutDayClosed` includes day totals, carry, and timestamp.

- **Documentation updates**
  - Files: `README.md`, `docs/quote_only_solution.md`, `docs/WALKTHROUGH.md`.
  - Actions:
    - Document final PDA seeds (policy/progress/position_owner/treasury), account maps, and CP‑AMM wiring.
    - Provide example commands for validator, build, and tests.

## Priority P2 (Medium)

- **Test suite expansion (unit + integration + bankrun)**
  - Files: `tests/`.
  - Coverage goals:
    - Initialization + quote‑only validation.
    - Claim flow: base‑fee present → deterministic failure.
    - Distribution across multiple pages with stable day snapshot; re‑run of same page doesn’t double pay.
    - Daily cap and min payout threshold behavior; dust carry forward.
    - Missing investor ATA: both funded and skipped paths.
    - Day close remainder to creator and proper resets.
    - 24h gate: second run in same day fails with expected error.

- **Developer ergonomics**
  - Add `package.json` scripts mirroring `star-honorary-fee-module` (e.g., `yarn test:full:bankrun`).
  - Optional: Introduce Bankrun harness for faster, deterministic testing.

## Priority P3 (Nice‑to‑have)

- **CI & quality gates**
  - Lint/format checks; unit test coverage thresholds; anchor build in CI.

- **Observability improvements**
  - Structured logs for critical steps; clearer msg! output for page progress and caps.

## Acceptance Checklist (must pass before sign‑off)

- **Work A:** Honorary CP‑AMM position is created by CPI, owned by program PDA, deterministic seeds documented.
- **Quote‑only guarantee:** Validated at init; claim flow fails if any base fees are present.
- **Work B:** 24h gating, pagination, idempotency, Streamflow‑weighted pro‑rata, daily cap, min payouts, dust carry, creator remainder at close.
- **Resilience:** Missing investor ATAs do not block day close; policy toggle to fund or skip.
- **Events & errors:** Required events emitted; clear, deterministic error codes.
- **Tests:** Full E2E coverage with concrete numeric assertions; re‑run safety validated.
- **Docs:** PDAs, wiring, examples, and runbooks updated.
