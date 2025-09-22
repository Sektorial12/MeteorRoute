#!/usr/bin/env node

/**
 * Fetch Real DLMM Pool Data
 * 
 * This script fetches actual Meteora DLMM pool data from Solana mainnet
 * to configure our fee routing system with real pool parameters.
 */

const { Connection, PublicKey } = require('@solana/web3.js');
const fs = require('fs');
const path = require('path');

// Configuration
const SOLANA_RPC_URL = process.env.SOLANA_RPC_URL || 'https://api.mainnet-beta.solana.com';
const METEORA_DLMM_PROGRAM_ID = 'LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo';

const USDC_MINT = 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v';
const SOL_MINT = 'So11111111111111111111111111111111111111112'; // Wrapped SOL

const KNOWN_POOLS = [
  '8Ve9KtGNtLRxCQNAVfkHEP5GRZHjdj6BjB1RQFZewG6V', // WIF-SOL DLMM pool
  'FjQFCYBksQ89SEWZEweVzd8A8Q8dwdKav5dxypStVfCX', // USDT-SOL DLMM pool
  'D7kPpawF8bXpZNHGCZuKV3xubyZsRrF7ufcKCzy2wrXX', // USDT-USDC DLMM pool
  // Add more as we find them
];

async function main() {
  console.log('🔍 Fetching Real Meteora DLMM Pool Data...\n');
  
  const connection = new Connection(SOLANA_RPC_URL, 'confirmed');
  
  try {
    console.log('📡 Connected to Solana mainnet');
    console.log(`🎯 DLMM Program: ${METEORA_DLMM_PROGRAM_ID}\n`);

    // Method 1: Try to get program accounts (this might be large)
    console.log('📋 Method 1: Fetching DLMM program accounts...');
    
    try {
      const programId = new PublicKey(METEORA_DLMM_PROGRAM_ID);
      
      // Get a limited number of accounts to avoid overwhelming response
      const accounts = await connection.getProgramAccounts(programId, {
        filters: [
          {
            dataSize: 8 + 32 + 32 + 8, // Approximate size filter for pool accounts
          }
        ],
        limit: 10
      });
      
      console.log(`✅ Found ${accounts.length} potential pool accounts`);
      
      if (accounts.length > 0) {
        console.log('\n📊 Sample Pool Addresses:');
        accounts.slice(0, 5).forEach((account, i) => {
          console.log(`${i + 1}. ${account.pubkey.toString()}`);
        });
      }
      
    } catch (error) {
      console.log('⚠️  Program accounts query failed (expected for large programs)');
      console.log('   Reason:', error.message);
    }

    // Method 2: Check known pool addresses
    console.log('\n📋 Method 2: Checking known pool addresses...');
    
    const validPools = [];
    
    for (const poolAddress of KNOWN_POOLS) {
      try {
        const poolPubkey = new PublicKey(poolAddress);
        const accountInfo = await connection.getAccountInfo(poolPubkey);
        
        if (accountInfo && accountInfo.owner.toString() === METEORA_DLMM_PROGRAM_ID) {
          console.log(`✅ Valid pool found: ${poolAddress}`);
          validPools.push({
            address: poolAddress,
            owner: accountInfo.owner.toString(),
            dataLength: accountInfo.data.length,
            lamports: accountInfo.lamports
          });
        } else if (accountInfo) {
          console.log(`❌ Account exists but wrong owner: ${poolAddress}`);
          console.log(`   Owner: ${accountInfo.owner.toString()}`);
        } else {
          console.log(`❌ Account not found: ${poolAddress}`);
        }
      } catch (error) {
        console.log(`❌ Invalid address: ${poolAddress}`);
      }
    }

    // Method 3: Create a working example configuration
    console.log('\n📋 Method 3: Creating example configuration...');
    
    const exampleConfig = {
      timestamp: new Date().toISOString(),
      network: 'mainnet-beta',
      dlmm_program_id: METEORA_DLMM_PROGRAM_ID,
      
      // Example configuration (needs real pool data)
      example_pool_config: {
        pool_pubkey: 'EXAMPLE_POOL_ADDRESS_NEEDED',
        quote_mint: USDC_MINT,
        base_mint: SOL_MINT,
        quote_decimals: 6,
        base_decimals: 9,
        
        // These need to be read from actual pool account
        pool_token_vault_0: 'DERIVED_FROM_POOL_ACCOUNT',
        pool_token_vault_1: 'DERIVED_FROM_POOL_ACCOUNT',
        current_tick: 'READ_FROM_POOL_STATE',
        tick_spacing: 'READ_FROM_POOL_CONFIG',
        
        // Configuration for our fee router
        vault_seed: 'meteora_sol_usdc_v1',
        y0_total_allocation: '100000000000000', // 100M tokens (example)
        investor_fee_share_bps: 7000, // 70%
        daily_cap_quote_lamports: 0, // No cap
        min_payout_lamports: 1000,
        policy_fund_missing_ata: true,
        
        // For quote-only position (needs calculation)
        suggested_tick_range: {
          note: 'These ticks need to be calculated based on current price to ensure quote-only accrual',
          tick_lower: 'CALCULATE_BASED_ON_CURRENT_PRICE',
          tick_upper: 'CALCULATE_BASED_ON_CURRENT_PRICE',
          strategy: 'Single-sided liquidity above current price for quote-only fees'
        }
      },
      
      validation_steps: [
        '1. Find active SOL/USDC DLMM pool on Meteora UI',
        '2. Copy pool address and verify it exists on-chain',
        '3. Read pool account data to get token vaults and configuration',
        '4. Calculate appropriate tick range for quote-only position',
        '5. Set Y0 and creator wallet configuration',
        '6. Run preflight verification',
        '7. Initialize honorary position'
      ],
      
      resources: {
        meteora_ui: 'https://v2.meteora.ag/pools/dlmm',
        solscan_dlmm: `https://solscan.io/account/${METEORA_DLMM_PROGRAM_ID}`,
        token_list: {
          USDC: USDC_MINT,
          SOL: SOL_MINT
        }
      },
      
      found_pools: validPools
    };
    
    // Save configuration
    const outputPath = path.join(__dirname, '..', 'config', 'real_pool_config.json');
    fs.writeFileSync(outputPath, JSON.stringify(exampleConfig, null, 2));
    
    console.log('\n✅ Configuration template created');
    console.log(`📁 Saved to: ${outputPath}`);
    
    if (validPools.length > 0) {
      console.log(`\n🎉 Found ${validPools.length} valid DLMM pools!`);
      validPools.forEach((pool, i) => {
        console.log(`${i + 1}. ${pool.address} (${pool.dataLength} bytes, ${pool.lamports} lamports)`);
      });
    } else {
      console.log('\n⚠️  No valid pools found from known addresses');
      console.log('📋 Next steps:');
      console.log('1. Visit https://v2.meteora.ag/pools/dlmm');
      console.log('2. Find an active SOL/USDC pool');
      console.log('3. Copy the pool address');
      console.log('4. Update the KNOWN_POOLS array in this script');
      console.log('5. Run the script again');
    }
    
  } catch (error) {
    console.error('❌ Error fetching pool data:', error.message);
    process.exit(1);
  }
}

if (require.main === module) {
  main().catch(console.error);
}

module.exports = { main };
