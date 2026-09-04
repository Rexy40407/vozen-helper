import { describe, expect, it } from 'vitest';

import { classifyAuditResult } from '../tools/npm-audit-ci.mjs';

describe('npm audit CI classification', () => {
  it('accepts a successful audit', () => {
    expect(classifyAuditResult(0, '{"metadata":{"vulnerabilities":{"total":0}}}', null)).toEqual({
      kind: 'success',
    });
  });

  it('fails immediately when npm reports a high vulnerability', () => {
    const report = JSON.stringify({
      metadata: {
        vulnerabilities: { info: 0, low: 0, moderate: 0, high: 1, critical: 0, total: 1 },
      },
    });

    expect(classifyAuditResult(1, report, null)).toEqual({
      kind: 'vulnerabilities',
      high: 1,
      critical: 0,
    });
  });

  it('retries provider and transport failures', () => {
    expect(
      classifyAuditResult(
        1,
        JSON.stringify({ error: { code: 'E503', summary: 'Service unavailable' } }),
        null,
      ),
    ).toEqual({ kind: 'retryable' });

    expect(classifyAuditResult(null, '', new Error('ETIMEDOUT'))).toEqual({
      kind: 'retryable',
    });
  });

  it('does not treat low or moderate findings as a high-severity failure', () => {
    const report = JSON.stringify({
      metadata: {
        vulnerabilities: { info: 0, low: 2, moderate: 1, high: 0, critical: 0, total: 3 },
      },
    });

    expect(classifyAuditResult(0, report, null)).toEqual({ kind: 'success' });
  });
});
