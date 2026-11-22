#!/usr/bin/env node
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { CdpClient } from './lib/cdp.mjs';
import { ApiClient, XMediaServerManager } from './lib/client.mjs';
import { MockOriginServer } from './lib/mock_origin.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const VALID_DOMAINS = new Set(['core', 'comic', 'music', 'video', 'ui', 'nonreg']);
const VALID_TIERS = new Set([1, 2, 3, 4]);

function printUsage() {
  console.log(`xmedia E2E Test Runner (Node.js ESM, zero npm dependencies)

Usage:
  node e2e/run.mjs --self-test
  node e2e/run.mjs [--tier <1|2|3|4>] [--domain <core|comic|music|video|ui|nonreg>]
  node e2e/run.mjs --live [--domain <comic|music|video|ui>]

Options:
  --self-test           Run harness & Mock Origin Server self-tests (ST-01..ST-10 + CDP smoke)
  --live                Run opt-in live public-source verification (requires internet)
  --live-report <dir>   Output directory for --live JSON/Markdown reports
  --tier <1|2|3|4>      Run specific test tier(s); repeatable or comma-separated (default: 1,2,3,4)
  --domain <name>       Scope to domain: core | comic | music | video | ui | nonreg
  --base-url <url>      Target an already-running xmedia server instead of spawning one
  --xmedia-bin <path>   Explicit path to xmedia binary
  --web-dir <path>      Explicit path to built web/dist directory
  --keep-tmp            Preserve temporary data/profile directories after run
  --json                Print JSON summary report to stdout
  -h, --help            Show this help message
`);
}

export function parseArgs(argv) {
  const opts = {
    selfTest: false,
    live: false,
    liveReport: null,
    tiers: [],
    domain: null,
    baseUrl: null,
    xmediaBin: null,
    webDir: null,
    keepTmp: false,
    json: false,
    help: false,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const nextVal = () => {
      if (i + 1 >= argv.length) {
        throw new Error(`Missing value for ${arg}`);
      }
      i += 1;
      return argv[i];
    };

    const eqVal = () => arg.slice(arg.indexOf('=') + 1);

    if (arg === '--self-test') {
      opts.selfTest = true;
    } else if (arg === '--live') {
      opts.live = true;
    } else if (arg === '--live-report' || arg.startsWith('--live-report=')) {
      opts.liveReport = (arg.includes('=') ? eqVal() : nextVal()).trim();
      if (!opts.liveReport) {
        throw new Error('Missing non-empty value for --live-report');
      }
    } else if (arg === '--keep-tmp') {
      opts.keepTmp = true;
    } else if (arg === '--json') {
      opts.json = true;
    } else if (arg === '-h' || arg === '--help') {
      opts.help = true;
    } else if (arg === '--tier' || arg.startsWith('--tier=')) {
      const raw = arg.includes('=') ? eqVal() : nextVal();
      for (const piece of raw.split(',')) {
        const num = Number.parseInt(piece.trim(), 10);
        if (!VALID_TIERS.has(num)) {
          throw new Error(`Invalid --tier value: ${piece}. Must be 1, 2, 3, or 4.`);
        }
        if (!opts.tiers.includes(num)) opts.tiers.push(num);
      }
    } else if (arg === '--domain' || arg.startsWith('--domain=')) {
      const val = (arg.includes('=') ? eqVal() : nextVal()).trim();
      if (!VALID_DOMAINS.has(val)) {
        throw new Error(
          `Invalid --domain value: ${val}. Must be one of: ${[...VALID_DOMAINS].join(', ')}.`,
        );
      }
      opts.domain = val;
    } else if (arg === '--base-url' || arg.startsWith('--base-url=')) {
      opts.baseUrl = (arg.includes('=') ? eqVal() : nextVal()).trim();
    } else if (arg === '--xmedia-bin' || arg.startsWith('--xmedia-bin=')) {
      opts.xmediaBin = (arg.includes('=') ? eqVal() : nextVal()).trim();
    } else if (arg === '--web-dir' || arg.startsWith('--web-dir=')) {
      opts.webDir = (arg.includes('=') ? eqVal() : nextVal()).trim();
    } else {
      throw new Error(`Unknown CLI argument: ${arg}`);
    }
  }

  if (opts.tiers.length === 0) {
    opts.tiers = [1, 2, 3, 4];
  }
  return opts;
}

async function loadSuiteModule(relPath) {
  const fullPath = path.join(__dirname, relPath);
  if (!existsSync(fullPath)) {
    throw new Error(
      `Suite file not found: ${fullPath}. Ensure all E2E suite files have been created.`,
    );
  }
  return import(pathToFileURL(fullPath).href);
}

function summarizeResults(results, startTimeMs) {
  const total = results.reduce((acc, r) => acc + (r.total || 0), 0);
  const passed = results.reduce((acc, r) => acc + (r.passed || 0), 0);
  const failed = results.reduce((acc, r) => acc + (r.failed || 0), 0);
  const warned = results.reduce((acc, r) => acc + (r.warned || 0), 0);
  const durationMs = Math.round(performance.now() - startTimeMs);
  return { total, passed, failed, warned, durationMs, suites: results };
}

async function main() {
  let opts;
  try {
    opts = parseArgs(process.argv.slice(2));
  } catch (err) {
    console.error(`Argument error: ${err.message}`);
    process.exitCode = 2;
    return;
  }

  if (opts.help) {
    printUsage();
    return;
  }

  const startTimeMs = performance.now();

  if (opts.selfTest) {
    if (!opts.json) {
      console.log('Running xmedia E2E harness self-tests...');
    }
    const mod = await loadSuiteModule('tests/self_test.mjs');
    const res = await mod.runSelfTests({ silent: opts.json });
    const summary = summarizeResults([res], startTimeMs);
    if (opts.json) {
      console.log(JSON.stringify(summary, null, 2));
    } else {
      console.log(
        `\nSelf-test complete: ${summary.passed}/${summary.total} passed, ${summary.failed} failed (${summary.durationMs}ms)`,
      );
    }
    process.exitCode = summary.failed > 0 ? 1 : 0;
    return;
  }

  const context = {
    baseUrl: opts.baseUrl,
    xmediaBin: opts.xmediaBin,
    webDir: opts.webDir,
    liveReport: opts.liveReport,
    keepTmp: opts.keepTmp,
    domain: opts.domain,
    silent: opts.json,
    MockOriginServer,
    XMediaServerManager,
    ApiClient,
    CdpClient,
  };

  if (opts.live) {
    if (!opts.json) {
      console.log('Running xmedia opt-in --live external source verification...');
    }
    let res;
    try {
      const mod = await loadSuiteModule('tests/live.mjs');
      const runner = mod.runLiveTests || mod.runSuite || mod.default;
      if (typeof runner !== 'function') {
        throw new Error('e2e/tests/live.mjs must export runLiveTests(context)');
      }
      res = await runner(context);
    } catch (err) {
      res = {
        name: 'live',
        suite: 'live',
        total: 1,
        passed: 0,
        failed: 1,
        warned: 0,
        cases: [
          {
            id: 'LIVE-RUNNER',
            domain: 'core',
            title: 'Live runner execution',
            status: 'fail',
            durationMs: Math.round(performance.now() - startTimeMs),
            error: err.stack || err.message,
          },
        ],
      };
      if (!opts.json) {
        console.error(`  [FAIL] LIVE-RUNNER: ${err.message}`);
      }
    }
    const summary = summarizeResults([res], startTimeMs);
    if (opts.json) {
      console.log(JSON.stringify(summary, null, 2));
    } else {
      console.log(
        `\nLive verification complete: ${summary.passed}/${summary.total} passed, ${summary.failed} failed, ${summary.warned} warned (${summary.durationMs}ms)`,
      );
    }
    process.exitCode = summary.failed > 0 || summary.total === 0 ? 1 : 0;
    return;
  }

  const results = [];
  for (const tier of opts.tiers) {
    if (!opts.json) {
      console.log(`\nRunning Tier ${tier}${opts.domain ? ` (domain=${opts.domain})` : ''}...`);
    }
    const tierStartMs = performance.now();
    try {
      const mod = await loadSuiteModule(`tests/tier${tier}.mjs`);
      const runner =
        mod[`runTier${tier}`] || mod.runSuite || mod.runTests || mod.default;
      if (typeof runner !== 'function') {
        throw new Error(`e2e/tests/tier${tier}.mjs must export runTier${tier}(context) or runSuite(context)`);
      }
      const res = await runner(context);
      results.push(res);
    } catch (err) {
      const durationMs = Math.round(performance.now() - tierStartMs);
      if (!opts.json) {
        console.error(`  [FAIL] T${tier}-RUNNER: ${err.message}`);
      }
      results.push({
        name: `tier${tier}`,
        suite: `tier${tier}`,
        total: 1,
        passed: 0,
        failed: 1,
        cases: [
          {
            id: `T${tier}-RUNNER`,
            domain: opts.domain || 'core',
            title: `Tier ${tier} runner execution`,
            status: 'fail',
            durationMs,
            error: err.stack || err.message,
          },
        ],
      });
    }
  }

  const summary = summarizeResults(results, startTimeMs);
  if (opts.json) {
    console.log(JSON.stringify(summary, null, 2));
  } else {
    console.log(
      `\nE2E Summary: ${summary.passed}/${summary.total} passed, ${summary.failed} failed (${summary.durationMs}ms)`,
    );
  }
  process.exitCode = summary.failed > 0 || summary.total === 0 ? 1 : 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((err) => {
    console.error(err.stack || err.message);
    process.exitCode = 1;
  });
}
