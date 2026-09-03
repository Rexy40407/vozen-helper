import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

describe('Rust release configuration', () => {
  it('preserves the private tracker identity from the active shared environment', () => {
    const script = readFileSync('deploy/prepare-rust-release.sh', 'utf8');
    const readExisting = script.indexOf('read_existing_env_value()');
    const loadClient = script.indexOf('read_existing_env_value HELPER_PRIVATE_TRACKER_CLIENT_ID');
    const loadOwner = script.indexOf('read_existing_env_value HELPER_PRIVATE_TRACKER_OWNER_ID');
    const replaceEnvironment = script.indexOf('cp "$node_root/.env" "$root/shared/.env"');

    expect(readExisting).toBeGreaterThan(-1);
    expect(loadClient).toBeGreaterThan(readExisting);
    expect(loadOwner).toBeGreaterThan(readExisting);
    expect(replaceEnvironment).toBeGreaterThan(loadClient);
    expect(replaceEnvironment).toBeGreaterThan(loadOwner);
  });
});
