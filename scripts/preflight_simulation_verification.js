#!/usr/bin/env node

/**
 * Preflight Simulation Verification
 * 
 * This script simulates the quote-only fee accrual behavior by creating
 * a mock DLMM environment and testing our verified tick range.
 */

const { Connection, PublicKey, Keypair, SystemProgram, Transaction } = require('@solana/web3.js');
const fs = require('fs');
const path = require('path');

// Import our analytical verification results
const { DLMMTickMath } = require('./preflight_analytical_verification.js');

// Configuration from analytical verification
const VERIFIED_CONFIG = {
  pool_address: '8Ve9KtGNtLRxCQNAVfkHEP5GRZHjdj6BjB1RQFZewG6V',
  current_tick: 12000,
  verified_tick_range: {
    tick_lower: 8000,
    tick_upper: 11000
  },
  token_order: {
    token0: 'WIF',
    token1: 'SOL',
    quote: 'token1'
  }
};

/**
 * Mock DLMM Pool Simulator
 * Simulates the behavior of a DLMM pool for testing purposes
 */
class MockDLMMPool {
  constructor(config) {
    this.currentTick = config.current_tick;
    this.tokenOrder = config.token_order;
    this.positions = new Map();
    this.feeAccumulator = {
      token0: 0,
      token1: 0
    };
    this.swapHistory = [];
  }

  /**
   * Create a mock position in the pool
   */
  createPosition(positionId, tickLower, tickUpper, liquidityAmount = 1000000) {
    const position = {
      id: positionId,
      tickLower,
      tickUpper,
      liquidity: liquidityAmount,
      feesAccrued: {
        token0: 0,
        token1: 0
      },
      isActive: this.currentTick >= tickLower && this.currentTick <= tickUpper
    };

    this.positions.set(positionId, position);
    
    console.log(`📍 Created position ${positionId}:`);
    console.log(`   Ticks: [${tickLower}, ${tickUpper}]`);
    console.log(`   Current tick: ${this.currentTick}`);
    console.log(`   Status: ${position.isActive ? 'ACTIVE' : 'INACTIVE'}`);
    console.log(`   Liquidity: ${liquidityAmount}`);
    
    return position;
  }

  /**
   * Simulate a swap that moves the price
   */
  simulateSwap(direction, amount, newTick) {
    const oldTick = this.currentTick;
    this.currentTick = newTick;
    
    const swap = {
      direction, // 'token0_to_token1' or 'token1_to_token0'
      amount,
      oldTick,
      newTick,
      ticksTraversed: Math.abs(newTick - oldTick),
      feesGenerated: amount * 0.003 // 0.3% fee example
    };

    // Determine which token the fees are paid in
    const feeToken = direction === 'token0_to_token1' ? 'token0' : 'token1';
    
    // Distribute fees to active positions
    for (const [positionId, position] of this.positions) {
      const wasActive = position.isActive;
      const isNowActive = this.currentTick >= position.tickLower && this.currentTick <= position.tickUpper;
      
      // CORRECTED LOGIC: In DLMM, positions only earn fees when price is WITHIN their range
      // The key insight: out-of-range positions don't earn fees at all
      const isInRange = this.currentTick >= position.tickLower && this.currentTick <= position.tickUpper;
      
      if (isInRange) {
        // Position earns fees proportional to liquidity, but only in the token being swapped TO
        const feeShare = position.liquidity / 10000000; // Mock total liquidity
        const earnedFees = swap.feesGenerated * feeShare;
        
        // CRITICAL: Fees are earned in the token being received by the pool
        // When someone swaps token0→token1, the pool receives token0, so fees are in token0
        // When someone swaps token1→token0, the pool receives token1, so fees are in token1
        position.feesAccrued[feeToken] += earnedFees;
        
        console.log(`💰 Position ${positionId} earned ${earnedFees.toFixed(6)} ${feeToken} fees (price in range)`);
      } else {
        console.log(`⏸️  Position ${positionId} out of range - no fees earned`);
      }

      position.isActive = isNowActive;
    }

    this.swapHistory.push(swap);
    
    console.log(`🔄 Swap: ${amount} ${direction} | Tick: ${oldTick} → ${newTick} | Fees: ${swap.feesGenerated.toFixed(6)} ${feeToken}`);
    
    return swap;
  }

  /**
   * Claim fees from a position (mock CP-AMM claim)
   */
  claimFees(positionId) {
    const position = this.positions.get(positionId);
    if (!position) {
      throw new Error(`Position ${positionId} not found`);
    }

    const claimedFees = {
      token0: position.feesAccrued.token0,
      token1: position.feesAccrued.token1
    };

    // Reset position fees
    position.feesAccrued.token0 = 0;
    position.feesAccrued.token1 = 0;

    console.log(`💸 Claimed fees from position ${positionId}:`);
    console.log(`   ${this.tokenOrder.token0}: ${claimedFees.token0.toFixed(6)}`);
    console.log(`   ${this.tokenOrder.token1}: ${claimedFees.token1.toFixed(6)}`);

    return claimedFees;
  }

  getPositionStatus(positionId) {
    return this.positions.get(positionId);
  }
}

async function main() {
  console.log('🧪 Preflight Simulation Verification\n');
  console.log('Pool:', VERIFIED_CONFIG.pool_address);
  console.log('Verified Tick Range:', VERIFIED_CONFIG.verified_tick_range);
  console.log('Timestamp:', new Date().toISOString());
  console.log('=' .repeat(60) + '\n');

  try {
    // Step 1: Initialize mock DLMM pool
    console.log('📊 Step 1: Initializing mock DLMM pool...');
    const mockPool = new MockDLMMPool(VERIFIED_CONFIG);
    
    // Step 2: Create our honorary position with verified tick range
    console.log('\n📊 Step 2: Creating honorary position...');
    const honoraryPosition = mockPool.createPosition(
      'honorary_position',
      VERIFIED_CONFIG.verified_tick_range.tick_lower,
      VERIFIED_CONFIG.verified_tick_range.tick_upper,
      5000000 // 5M liquidity units
    );

    // Step 3: Create control positions for comparison
    console.log('\n📊 Step 3: Creating control positions...');
    
    // Active position (should earn both tokens)
    const activePosition = mockPool.createPosition(
      'active_control',
      11000, // Spans current tick
      13000,
      1000000
    );

    // Below range position (should earn WIF fees)
    const belowPosition = mockPool.createPosition(
      'below_control',
      5000,
      7000,
      1000000
    );

    // Step 4: Simulate various swaps to generate fees
    console.log('\n📊 Step 4: Simulating swaps to generate fees...');
    
    const swapScenarios = [
      // Our range is [8000, 11000], current tick is 12000 (above our range)
      // For quote-only fees, we need price to move INTO our range and only do quote→base swaps
      
      // Scenario 1: Price drops into our range via SOL→WIF swap (should generate SOL fees)
      { description: 'SOL→WIF swap brings price into our range', direction: 'token1_to_token0', amount: 1000, newTick: 10000 },
      
      // Scenario 2: More SOL→WIF trading within our range (should generate more SOL fees)
      { description: 'More SOL→WIF trading in range', direction: 'token1_to_token0', amount: 500, newTick: 9500 },
      
      // Scenario 3: WIF→SOL swap within our range (should generate WIF fees - NOT what we want)
      { description: 'WIF→SOL swap in range (undesired)', direction: 'token0_to_token1', amount: 300, newTick: 9800 },
      
      // Scenario 4: Price moves out of our range (no more fees)
      { description: 'Price moves above our range', direction: 'token0_to_token1', amount: 1200, newTick: 12500 },
      
      // Scenario 5: Trading outside our range (no fees for us)
      { description: 'Trading above our range', direction: 'token1_to_token0', amount: 800, newTick: 12200 }
    ];

    for (const scenario of swapScenarios) {
      console.log(`\n🎯 ${scenario.description}:`);
      mockPool.simulateSwap(scenario.direction, scenario.amount, scenario.newTick);
    }

    // Step 5: Claim fees and analyze results
    console.log('\n📊 Step 5: Claiming fees and analyzing results...');
    
    console.log('\n💰 HONORARY POSITION (Quote-Only Target):');
    const honoraryFees = mockPool.claimFees('honorary_position');
    
    console.log('\n💰 ACTIVE CONTROL POSITION:');
    const activeFees = mockPool.claimFees('active_control');
    
    console.log('\n💰 BELOW RANGE CONTROL POSITION:');
    const belowFees = mockPool.claimFees('below_control');

    // Step 6: Verify quote-only guarantee
    console.log('\n📊 Step 6: Verifying quote-only guarantee...');
    
    const verification = {
      honorary_position: {
        base_fees: honoraryFees.token0,
        quote_fees: honoraryFees.token1,
        quote_only: honoraryFees.token0 === 0 && honoraryFees.token1 > 0
      },
      active_control: {
        base_fees: activeFees.token0,
        quote_fees: activeFees.token1,
        quote_only: activeFees.token0 === 0 && activeFees.token1 > 0
      },
      below_control: {
        base_fees: belowFees.token0,
        quote_fees: belowFees.token1,
        quote_only: belowFees.token0 === 0 && belowFees.token1 > 0
      }
    };

    console.log('\n🔍 VERIFICATION RESULTS:');
    console.log(`Honorary Position Quote-Only: ${verification.honorary_position.quote_only ? '✅ PASS' : '❌ FAIL'}`);
    console.log(`  - ${VERIFIED_CONFIG.token_order.token0} fees: ${verification.honorary_position.base_fees.toFixed(6)}`);
    console.log(`  - ${VERIFIED_CONFIG.token_order.token1} fees: ${verification.honorary_position.quote_fees.toFixed(6)}`);
    
    console.log(`Active Control Quote-Only: ${verification.active_control.quote_only ? '✅ PASS' : '❌ FAIL'} (Expected: FAIL)`);
    console.log(`  - ${VERIFIED_CONFIG.token_order.token0} fees: ${verification.active_control.base_fees.toFixed(6)}`);
    console.log(`  - ${VERIFIED_CONFIG.token_order.token1} fees: ${verification.active_control.quote_fees.toFixed(6)}`);

    // Step 7: Generate simulation report
    const simulationReport = {
      timestamp: new Date().toISOString(),
      pool_address: VERIFIED_CONFIG.pool_address,
      verification_type: 'simulation',
      test_configuration: VERIFIED_CONFIG,
      
      positions_tested: {
        honorary_position: {
          tick_range: [VERIFIED_CONFIG.verified_tick_range.tick_lower, VERIFIED_CONFIG.verified_tick_range.tick_upper],
          strategy: 'quote_only_target',
          result: verification.honorary_position
        },
        active_control: {
          tick_range: [11000, 13000],
          strategy: 'active_control',
          result: verification.active_control
        },
        below_control: {
          tick_range: [5000, 7000],
          strategy: 'below_range_control',
          result: verification.below_control
        }
      },
      
      swap_scenarios: swapScenarios,
      swap_history: mockPool.swapHistory,
      
      verification_result: {
        quote_only_achieved: verification.honorary_position.quote_only,
        confidence_level: verification.honorary_position.quote_only ? 'HIGH' : 'LOW',
        status: verification.honorary_position.quote_only ? 'PASSED' : 'FAILED'
      },
      
      conclusion: verification.honorary_position.quote_only 
        ? 'Simulation confirms quote-only fee accrual for the verified tick range'
        : 'Simulation failed to achieve quote-only fee accrual'
    };

    // Save simulation report
    const outputPath = path.join(__dirname, '..', 'logs', 'preflight_simulation_log.json');
    fs.mkdirSync(path.dirname(outputPath), { recursive: true });
    fs.writeFileSync(outputPath, JSON.stringify(simulationReport, null, 2));

    console.log(`\n📄 Simulation report saved: ${outputPath}`);
    console.log(`🎯 Simulation Result: ${simulationReport.verification_result.status}`);

    return simulationReport.verification_result.quote_only_achieved;

  } catch (error) {
    console.error('❌ Error during simulation verification:', error.message);
    
    const errorReport = {
      timestamp: new Date().toISOString(),
      pool_address: VERIFIED_CONFIG.pool_address,
      verification_type: 'simulation',
      status: 'FAILED',
      error: error.message,
      conclusion: 'Simulation verification could not be completed'
    };
    
    const outputPath = path.join(__dirname, '..', 'logs', 'preflight_simulation_log.json');
    fs.mkdirSync(path.dirname(outputPath), { recursive: true });
    fs.writeFileSync(outputPath, JSON.stringify(errorReport, null, 2));
    
    return false;
  }
}

if (require.main === module) {
  main().then(success => {
    process.exit(success ? 0 : 1);
  }).catch(console.error);
}

module.exports = { main, MockDLMMPool };
