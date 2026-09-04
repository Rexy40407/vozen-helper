export type AuditClassification =
  | { kind: 'success' }
  | { kind: 'vulnerabilities'; high: number; critical: number }
  | { kind: 'retryable' };

export function classifyAuditResult(
  status: number | null,
  stdout: string,
  error: Error | null,
): AuditClassification;
