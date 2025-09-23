# MeteorRoute Devnet Deployment Guide

This guide walks you through deploying and testing the MeteorRoute system on Solana Devnet.

## 🚀 Quick Start

```bash
# 1. Run pre-deployment checklist
node scripts/pre_deployment_checklist.js

# 2. Deploy and test on devnet
node scripts/deploy_and_test.js
```

## 📋 Prerequisites

### Required Software
- **Solana CLI** v1.16.0+
- **Anchor CLI** v0.30.0+
- **Node.js** v18+
- **Git** (for version control)

### Installation Commands
```bash
# Install Solana CLI
sh -c "$(curl -sSfL https://release.solana.com/v1.16.0/install)"

# Install Anchor CLI
npm install -g @coral-xyz/anchor-cli

# Verify installations
solana --version
anchor --version
```

## 🔧 Environment Setup

### 1. Create Solana Wallet
```bash
# Generate new wallet (if you don't have one)
solana-keygen new

# Or recover existing wallet
solana-keygen recover
```

### 2. Configure for Devnet
```bash
# Set cluster to devnet
solana config set --url devnet

# Verify configuration
solana config get
```

### 3. Fund Wallet
```bash
# Request devnet SOL (5 SOL should be sufficient)
solana airdrop 5

# Check balance
solana balance
```

### 4. Install Dependencies
```bash
# Install Node.js dependencies
npm install

# Build the project
anchor build
```

## 🚀 Deployment Process

### Step 1: Pre-Deployment Verification
```bash
node scripts/pre_deployment_checklist.js
```

This script verifies:
- ✅ All required tools installed
- ✅ Wallet configured and funded
- ✅ Project builds successfully
- ✅ All test files present
- ✅ Documentation complete

### Step 2: Deploy to Devnet
```bash
# Deploy program to devnet
anchor deploy --provider.cluster devnet
```

Expected output:
```
Deploying workspace: https://api.devnet.solana.com
Upgrade authority: <your-wallet-address>
Deploying program "meteor_route_fee_router"...
Program path: /path/to/target/deploy/meteor_route_fee_router.so...
Program Id: <program-id>

Deploy success
```

### Step 3: Run Comprehensive Tests
```bash
node scripts/deploy_and_test.js
```

This will test:
- 🏗️ Program initialization
- 🏛️ Honorary position creation
- 💰 Fee distribution flows
- 🛡️ Error handling
- 📡 Event emission
- 🔄 Pagination support

## 🧪 Test Scenarios

### Core Functionality Tests

#### 1. Basic Pro-Rata Distribution
- **Input**: 5M lamports claimed, 3 investors with different lock amounts
- **Expected**: Proportional distribution based on locked amounts
- **Verification**: Check investor payouts match mathematical formulas

#### 2. Base Fee Detection
- **Input**: Mixed quote + base fees claimed
- **Expected**: Transaction fails with `ERR_BASE_FEE_DETECTED`
- **Verification**: No distribution occurs, funds remain safe

#### 3. 24h Gate Enforcement
- **Input**: Attempt second distribution on same day
- **Expected**: Transaction fails with `ERR_DAY_GATE_NOT_PASSED`
- **Verification**: Gate prevents multiple distributions per day

#### 4. Dust Handling
- **Input**: Small amounts below minimum payout threshold
- **Expected**: Amounts carried forward as dust
- **Verification**: Dust properly accumulated in Progress PDA

#### 5. Daily Cap Enforcement
- **Input**: Distribution amount exceeding daily cap
- **Expected**: Distribution capped at maximum allowed
- **Verification**: Cap enforced, excess carried to next day

### Edge Case Tests

#### 6. All Unlocked Scenario
- **Input**: All investor tokens unlocked (locked_total = 0)
- **Expected**: 100% of fees go to creator
- **Verification**: No investor payouts, all to creator

#### 7. Missing ATA Handling
- **Input**: Investors with missing Associated Token Accounts
- **Expected**: ATAs created if policy allows, or skipped if not
- **Verification**: ATA creation cost tracked, failures logged

#### 8. Multi-Page Distribution
- **Input**: Large number of investors requiring pagination
- **Expected**: Multiple pages processed correctly
- **Verification**: Pagination cursor updates, no double-pays

## 📊 Expected Test Results

### Success Criteria
```
📊 COMPREHENSIVE TEST REPORT
==================================================
Program Deployment          ✅ PASSED
System Initialization       ✅ PASSED
Policy Setup                ✅ PASSED
Position Creation           ✅ PASSED
Fee Distribution            ✅ PASSED
Pagination Support          ✅ PASSED
Error Handling              ✅ PASSED
Event Emission              ✅ PASSED
==================================================
OVERALL RESULT: 8/8 tests passed
🎉 ALL TESTS PASSED - SYSTEM READY FOR PRODUCTION!
```

### Performance Benchmarks
- **Compute Units**: < 200,000 per transaction
- **Execution Time**: < 2 seconds per page
- **Memory Usage**: < 512 KB
- **Max Investors per Page**: 50 (recommended)

## 🔍 Monitoring & Verification

### Key Metrics to Monitor
1. **Transaction Success Rate**: Should be 100% for valid operations
2. **Mathematical Accuracy**: Payouts match expected formulas
3. **Safety Mechanisms**: Base fee detection working correctly
4. **Event Emission**: All required events emitted properly
5. **PDA State**: Account states update correctly

### Verification Commands
```bash
# Check program deployment
solana program show <program-id> --url devnet

# Monitor account changes
solana account <pda-address> --url devnet

# View transaction logs
solana transaction <tx-signature> --url devnet
```

## 🚨 Troubleshooting

### Common Issues

#### Deployment Fails
```bash
# Check wallet balance
solana balance

# Verify network connection
solana cluster-version

# Rebuild if needed
anchor clean && anchor build
```

#### Tests Fail
```bash
# Check program ID in tests matches deployed program
# Verify devnet RPC endpoint is responsive
# Ensure wallet has sufficient SOL for test transactions
```

#### Transaction Errors
```bash
# Check compute budget limits
# Verify account permissions
# Review transaction logs for specific error codes
```

### Error Code Reference
- `6001`: `ERR_BASE_FEE_DETECTED` - Base fees present, distribution aborted
- `6002`: `ERR_DAY_GATE_NOT_PASSED` - 24h gate not satisfied
- `6003`: `ERR_ALREADY_DISTRIBUTED` - Day already finalized
- `6004`: `ERR_OVERFLOW` - Arithmetic overflow in calculations
- `6005`: `ERR_PDA_SEED_MISMATCH` - PDA derivation mismatch

## 🎯 Next Steps After Successful Devnet Testing

1. **Mainnet Preparation**
   - Update configuration for mainnet
   - Set up production monitoring
   - Prepare real pool integration

2. **Integration with Real Systems**
   - Replace mock CP-AMM calls with real Meteora DLMM CPIs
   - Replace mock Streamflow calls with real Streamflow CPIs
   - Set up real token accounts and ATAs

3. **Production Deployment**
   - Deploy to mainnet-beta
   - Initialize with production parameters
   - Begin live fee routing operations

## 📞 Support

If you encounter issues during deployment or testing:

1. Check the troubleshooting section above
2. Review transaction logs for specific error messages
3. Verify all prerequisites are met
4. Ensure sufficient devnet SOL in wallet

## ✅ Deployment Checklist

- [ ] Pre-deployment checklist passes
- [ ] Program deploys successfully to devnet
- [ ] All 8 test scenarios pass
- [ ] Performance benchmarks met
- [ ] Event emission verified
- [ ] Error handling confirmed
- [ ] Documentation updated with program ID
- [ ] Ready for mainnet deployment

---

**🎉 Once all tests pass, your MeteorRoute system is verified and ready for production use!**
