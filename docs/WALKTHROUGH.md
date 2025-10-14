# Judges Walkthrough: Build, Verify, and Test MeteorRoute

This guide gives judges a deterministic, end-to-end path to build the Anchor program, regenerate the IDL/types, run the test suite, and optionally exercise verification scripts under `scripts/`.

## 1) Prerequisites

- Solana CLI >= 1.18
- Rust toolchain (stable)
- Anchor CLI 0.31.1 (or 0.31.x)
- Node.js 18.x and Yarn Classic (1.x)
- Git, bash (WSL/macOS/Linux)

Quick checks:
```bash
solana --version
rustc --version
anchor --version
node -v && yarn -v
```

## 2) Clone and Install

```bash
# WSL/macOS/Linux shell
git clone <repo_url> MeteorRoute
cd MeteorRoute

# Install JS deps (Yarn 1.x)
yarn install
```

## 3) Build (Regenerate IDL/Types)

We enabled the Anchor IDL build feature in `programs/meteor-route-fee-router/Cargo.toml`:
```toml
[features]
idl-build = ["anchor-lang/idl-build", "anchor-spl/idl-build"]
```

Build the program and regenerate IDL/TS types:
```bash
yarn build
```

Troubleshooting:
- If you see anchor-syn/proc-macro2 span errors (E0599), this means Anchor CLI version mismatch:
  ```bash
  # The project uses Anchor 0.31.1, ensure your CLI matches
  anchor --version  # Should show 0.31.1
  
  # If you have 0.30.x, upgrade:
  cargo install --git https://github.com/coral-xyz/anchor avm --locked --force
  avm install 0.31.1
  avm use 0.31.1
  
  # Then clean and rebuild
  rm -rf target Cargo.lock
  yarn build
  ```
- See `docs/BUILD_ISSUES_AND_SOLUTIONS.md` for more details and Windows-specific issues.

## 4) Run Tests

Two options:

- Anchor-managed local validator (simple):
```bash
anchor test
```

- Manual validator:
```bash
# Terminal A (WSL home path is recommended)
mkdir -p ~/.local/anchor-ledger
solana-test-validator --ledger ~/.local/anchor-ledger --reset --rpc-port 8899 --faucet-port 9900

# Terminal B (in project root)
solana config set --url http://127.0.0.1:8899
anchor test --skip-local-validator
```

The test files exercise:
- Policy initialization
- Progress state initialization
- Honorary position init (account wiring + preflight checks)
- Distribution math vectors (proportional split, dust/min payout, all-unlocked)
 - CP‑AMM integration scaffolding (E2E): two tests are intentionally skipped pending Streamflow mock data

## 5) Using the Scripts (Optional, For Demo Evidence)

All scripts live in `scripts/` and log outputs under `logs/`.

- Gather pool information template:
```bash
node scripts/gather_pool_info.js
# Output: config/pool_info_template.json and logs/pool_snapshots/
```

- Preflight analytical verification (quote-only feasibility):
```bash
SOLANA_RPC_URL=https://api.mainnet-beta.solana.com \
node scripts/preflight_analytical_verification.js
# Output: logs/preflight_analytical_report.{json,txt}
```

- Preflight simulation helper (shell wrapper):
```bash
bash scripts/preflight_simulate.sh <pool_pubkey> <tick_lower> <tick_upper>
```

- Post-init verification snapshot (after position init):
```bash
node scripts/post_init_verification.js
# Confirms token order assumptions, saves snapshot under logs/
```

- Post-crank snapshot (after a distribution crank):
```bash
node scripts/post_crank_snapshot.js
# Captures distribution day state, carry, and remainder evidence
```

- Comprehensive system test harness:
```bash
node scripts/comprehensive_system_test.js
# Or selectively run subsets based on environment flags (see file header)
```

Notes:
- Some scripts assume real mainnet-beta accounts and will only produce mocked analysis if on-chain parsing isnt implemented in this version. They still generate traceable artifacts for review.

## 6) What To Look For

- IDL and TS types regenerate cleanly (`target/idl`, `target/types`)
- Tests pass (at least the initialization and math vectors)
- Events and logs are emitted as described in `README.md` under Events
- Optional: `logs/` contains preflight and snapshot artifacts

## 7) Common Pitfalls & Fixes

- **Missing IDL build feature**: Already added in Cargo.toml; just `yarn build`.
- **Macro span errors (E0599)**: Anchor CLI version mismatch. Ensure you're using Anchor 0.31.1 (see troubleshooting in section 3).
- **Tests failing with account errors**: Anchor 0.31.1 uses auto-PDA resolution. The test file uses `.accounts()` (not `.accountsStrict()`). If tests fail, run `anchor build` to regenerate types, then retry.
- **Windows build issues**: Use WSL for building. See `docs/BUILD_ISSUES_AND_SOLUTIONS.md` for detailed Windows troubleshooting.

## 8) Clean Up

Generated artifacts are ignored by `.gitignore` (e.g., `target/`, `node_modules/`, `logs/`, `artifacts/`). To start fresh:
```bash
rm -rf target node_modules logs artifacts
yarn install
yarn build
```

---
If anything fails, please capture the console output and we will provide a fast patch. 
