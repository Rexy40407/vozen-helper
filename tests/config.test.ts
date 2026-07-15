import { describe, it, expect } from 'vitest';
import { loadEnv, ConfigError } from '../src/config.js';

const VALID = {
  DISCORD_TOKEN: 'abc.def.ghi',
  CLIENT_ID: '123456789012345678',
  GUILD_ID: '876543210987654321',
};

describe('loadEnv', () => {
  it('aceita um ambiente válido e aplica defaults', () => {
    const cfg = loadEnv({ ...VALID });
    expect(cfg.token).toBe('abc.def.ghi');
    expect(cfg.clientId).toBe('123456789012345678');
    expect(cfg.guildId).toBe('876543210987654321');
    expect(cfg.dbPath).toBe('./vozen-helper.db');
    expect(cfg.logLevel).toBe('info');
  });

  it('lista TODAS as variáveis obrigatórias em falta numa só mensagem', () => {
    try {
      loadEnv({});
      throw new Error('devia ter lançado');
    } catch (err) {
      expect(err).toBeInstanceOf(ConfigError);
      const msg = (err as Error).message;
      expect(msg).toContain('DISCORD_TOKEN');
      expect(msg).toContain('CLIENT_ID');
      expect(msg).toContain('GUILD_ID');
    }
  });

  it('rejeita CLIENT_ID que não é um snowflake', () => {
    expect(() => loadEnv({ ...VALID, CLIENT_ID: 'not-an-id' })).toThrow(ConfigError);
  });

  it('rejeita GUILD_ID que não é um snowflake', () => {
    expect(() => loadEnv({ ...VALID, GUILD_ID: '123' })).toThrow(ConfigError);
  });

  it('respeita DB_PATH e LOG_LEVEL quando dados', () => {
    const cfg = loadEnv({ ...VALID, DB_PATH: '/data/x.db', LOG_LEVEL: 'debug' });
    expect(cfg.dbPath).toBe('/data/x.db');
    expect(cfg.logLevel).toBe('debug');
  });

  it('cai em info quando LOG_LEVEL é inválido', () => {
    const cfg = loadEnv({ ...VALID, LOG_LEVEL: 'verbose' });
    expect(cfg.logLevel).toBe('info');
  });

  it('trata espaços em branco e trata token só-espaços como em falta', () => {
    expect(() => loadEnv({ ...VALID, DISCORD_TOKEN: '   ' })).toThrow(ConfigError);
  });
});
