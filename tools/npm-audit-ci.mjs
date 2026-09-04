#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

const DEFAULT_ATTEMPTS = 3;
const AUDIT_TIMEOUT_MS = 180_000;

function parseAuditReport(stdout) {
  try {
    return JSON.parse(stdout.trim());
  } catch {
    return null;
  }
}

export function classifyAuditResult(status, stdout, error) {
  if (status === 0 && !error) return { kind: 'success' };

  const report = parseAuditReport(stdout);
  const vulnerabilities = report?.metadata?.vulnerabilities;
  const high = Number(vulnerabilities?.high ?? 0);
  const critical = Number(vulnerabilities?.critical ?? 0);

  if (high > 0 || critical > 0) {
    return { kind: 'vulnerabilities', high, critical };
  }

  return { kind: 'retryable' };
}

function runAuditCommand() {
  const command = process.platform === 'win32' ? 'npm.cmd' : 'npm';
  return spawnSync(
    command,
    [
      'audit',
      '--audit-level=high',
      '--json',
      '--fetch-retries=2',
      '--fetch-retry-mintimeout=2000',
      '--fetch-retry-maxtimeout=10000',
      '--fetch-timeout=120000',
    ],
    {
      cwd: process.cwd(),
      encoding: 'utf8',
      maxBuffer: 10 * 1024 * 1024,
      shell: process.platform === 'win32',
      timeout: AUDIT_TIMEOUT_MS,
    },
  );
}

async function main() {
  for (let attempt = 1; attempt <= DEFAULT_ATTEMPTS; attempt += 1) {
    console.log(`npm audit attempt ${attempt}/${DEFAULT_ATTEMPTS}`);
    const result = runAuditCommand();

    if (result.stdout) process.stdout.write(result.stdout);
    if (result.stderr) process.stderr.write(result.stderr);
    if (result.error) console.error(`npm audit process error: ${result.error.message}`);

    const classification = classifyAuditResult(result.status, result.stdout ?? '', result.error);
    if (classification.kind === 'success') return 0;

    if (classification.kind === 'vulnerabilities') {
      console.error(
        `npm audit found ${classification.high} high and ${classification.critical} critical vulnerabilities; not retrying.`,
      );
      return result.status ?? 1;
    }

    if (attempt === DEFAULT_ATTEMPTS) {
      console.error('npm audit provider/transport failure persisted after all retries.');
      return result.status ?? 1;
    }

    const retryDelayMs = attempt * 10_000;
    console.warn(`npm audit provider/transport failure; retrying in ${retryDelayMs / 1000}s.`);
    await delay(retryDelayMs);
  }

  return 1;
}

const invokedAsScript =
  process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url));

if (invokedAsScript) {
  process.exitCode = await main();
}
