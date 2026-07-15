import { describe, it, expect } from 'vitest';
import { parseDuration, formatDuration, MAX_TIMEOUT_MS } from '../src/moderation/duration.js';

describe('parseDuration', () => {
  it('parseia unidades simples', () => {
    expect(parseDuration('30s')).toBe(30_000);
    expect(parseDuration('10m')).toBe(600_000);
    expect(parseDuration('1h')).toBe(3_600_000);
    expect(parseDuration('2d')).toBe(2 * 86_400_000);
    expect(parseDuration('1w')).toBe(7 * 86_400_000);
  });

  it('soma troços compostos', () => {
    expect(parseDuration('1h30m')).toBe(3_600_000 + 1_800_000);
    expect(parseDuration('1d 12h')).toBe(86_400_000 + 43_200_000);
  });

  it('rejeita lixo e strings vazias', () => {
    expect(parseDuration('')).toBeNull();
    expect(parseDuration('abc')).toBeNull();
    expect(parseDuration('1h xyz 2m')).toBeNull();
    expect(parseDuration('0s')).toBeNull();
  });

  it('MAX_TIMEOUT_MS são 28 dias', () => {
    expect(MAX_TIMEOUT_MS).toBe(28 * 86_400_000);
  });
});

describe('formatDuration', () => {
  it('formata de forma compacta', () => {
    expect(formatDuration(3_600_000)).toBe('1h');
    expect(formatDuration(90_000)).toBe('1m30s');
    expect(formatDuration(0)).toBe('0s');
  });
});
