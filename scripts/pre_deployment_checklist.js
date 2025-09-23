#!/usr/bin/env node

/**
 * Pre-Deployment Checklist
 * 
 * Verifies all requirements are met before deploying to devnet
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

class PreDeploymentChecker {
  constructor() {
    this.checks = [];
    this.passed = 0;
    this.failed = 0;
  }

  check(name, condition, details = '') {
    const status = condition ? '✅' : '❌';
    const message = `${status} ${name}`;
    
    console.log(details ? `${message}\n   ${details}` : message);
    
    this.checks.push({ name, passed: condition, details });
    
    if (condition) {
      this.passed++;
    } else {
      this.failed++;
    }
    
    return condition;
  }

  async runAllChecks() {
    console.log('🔍 PRE-DEPLOYMENT CHECKLIST\n');
    console.log('Verifying system readiness for devnet deployment...\n');

    // Check 1: Solana CLI installed and configured
    try {
      const solanaVersion = execSync('solana --version', { encoding: 'utf8' }).trim();
      this.check('Solana CLI installed', true, `Version: ${solanaVersion}`);
    } catch (error) {
      this.check('Solana CLI installed', false, 'Run: sh -c "$(curl -sSfL https://release.solana.com/v1.16.0/install)"');
    }

    // Check 2: Anchor CLI installed
    try {
      const anchorVersion = execSync('anchor --version', { encoding: 'utf8' }).trim();
      this.check('Anchor CLI installed', true, `Version: ${anchorVersion}`);
    } catch (error) {
      this.check('Anchor CLI installed', false, 'Run: npm install -g @coral-xyz/anchor-cli');
    }

    // Check 3: Wallet exists
    const walletPath = path.join(process.env.HOME || process.env.USERPROFILE, '.config', 'solana', 'id.json');
    const walletExists = fs.existsSync(walletPath);
    this.check('Solana wallet exists', walletExists, 
      walletExists ? `Path: ${walletPath}` : 'Run: solana-keygen new');

    // Check 4: Solana config points to devnet
    try {
      const config = execSync('solana config get', { encoding: 'utf8' });
      const isDevnet = config.includes('devnet') || config.includes('https://api.devnet.solana.com');
      this.check('Solana config set to devnet', isDevnet, 
        isDevnet ? 'Config: devnet' : 'Run: solana config set --url devnet');
    } catch (error) {
      this.check('Solana config accessible', false, 'Cannot read Solana config');
    }

    // Check 5: Wallet has sufficient balance
    if (walletExists) {
      try {
        const balance = execSync('solana balance', { encoding: 'utf8' }).trim();
        const balanceNum = parseFloat(balance.split(' ')[0]);
        const hasSufficientBalance = balanceNum >= 2.0;
        this.check('Wallet has sufficient SOL (≥2.0)', hasSufficientBalance, 
          `Balance: ${balance}${!hasSufficientBalance ? ' - Run: solana airdrop 5' : ''}`);
      } catch (error) {
        this.check('Wallet balance check', false, 'Cannot check wallet balance');
      }
    }

    // Check 6: Project builds successfully
    try {
      console.log('🔨 Building project...');
      execSync('anchor build', { cwd: process.cwd(), stdio: 'pipe' });
      this.check('Project builds successfully', true, 'Build completed without errors');
    } catch (error) {
      this.check('Project builds successfully', false, 'Build failed - check compilation errors');
    }

    // Check 7: IDL file exists
    const idlPath = './target/idl/meteor_route_fee_router.json';
    const idlExists = fs.existsSync(idlPath);
    this.check('IDL file generated', idlExists, 
      idlExists ? `Path: ${idlPath}` : 'Run anchor build to generate IDL');

    // Check 8: Program binary exists
    const programPath = './target/deploy/meteor_route_fee_router.so';
    const programExists = fs.existsSync(programPath);
    this.check('Program binary exists', programExists, 
      programExists ? `Path: ${programPath}` : 'Run anchor build to generate program binary');

    // Check 9: Test configuration exists
    const devnetConfigPath = './config/devnet_config.json';
    const configExists = fs.existsSync(devnetConfigPath);
    this.check('Devnet config exists', configExists, 
      configExists ? `Path: ${devnetConfigPath}` : 'Devnet configuration file missing');

    // Check 10: Dependencies installed
    const nodeModulesExists = fs.existsSync('./node_modules');
    this.check('Node dependencies installed', nodeModulesExists, 
      nodeModulesExists ? 'node_modules found' : 'Run: npm install');

    // Check 11: TypeScript compiles
    try {
      execSync('npx tsc --noEmit', { cwd: process.cwd(), stdio: 'pipe' });
      this.check('TypeScript compiles', true, 'No TypeScript errors');
    } catch (error) {
      this.check('TypeScript compiles', false, 'TypeScript compilation errors found');
    }

    // Check 12: Test files exist
    const testFiles = [
      './tests/meteor-route-fee-router.ts',
      './scripts/deploy_and_test.js',
      './scripts/comprehensive_system_test.js'
    ];
    
    const allTestsExist = testFiles.every(file => fs.existsSync(file));
    this.check('Test files exist', allTestsExist, 
      allTestsExist ? 'All test files found' : 'Some test files missing');

    // Check 13: Documentation complete
    const docFiles = [
      './README.md',
      './docs/example_transactions.md',
      './docs/quote_only_solution.md'
    ];
    
    const allDocsExist = docFiles.every(file => fs.existsSync(file));
    this.check('Documentation complete', allDocsExist, 
      allDocsExist ? 'All documentation files found' : 'Some documentation missing');

    // Check 14: Git repository clean
    try {
      const gitStatus = execSync('git status --porcelain', { encoding: 'utf8' }).trim();
      const isClean = gitStatus.length === 0;
      this.check('Git repository clean', isClean, 
        isClean ? 'No uncommitted changes' : 'Uncommitted changes found - consider committing');
    } catch (error) {
      this.check('Git repository status', false, 'Not a git repository or git not available');
    }

    // Summary
    console.log('\n' + '='.repeat(60));
    console.log(`CHECKLIST SUMMARY: ${this.passed}/${this.checks.length} checks passed`);
    
    if (this.failed === 0) {
      console.log('🎉 ALL CHECKS PASSED - READY FOR DEPLOYMENT!');
      console.log('\nNext steps:');
      console.log('1. Run: node scripts/deploy_and_test.js');
      console.log('2. Monitor deployment and test results');
      console.log('3. Verify all flows work correctly on devnet');
      return true;
    } else {
      console.log(`❌ ${this.failed} CHECKS FAILED - FIX ISSUES BEFORE DEPLOYMENT`);
      console.log('\nFailed checks:');
      this.checks
        .filter(check => !check.passed)
        .forEach(check => {
          console.log(`  • ${check.name}${check.details ? ': ' + check.details : ''}`);
        });
      return false;
    }
  }
}

async function main() {
  const checker = new PreDeploymentChecker();
  const ready = await checker.runAllChecks();
  
  process.exit(ready ? 0 : 1);
}

if (require.main === module) {
  main().catch(console.error);
}

module.exports = { PreDeploymentChecker };
