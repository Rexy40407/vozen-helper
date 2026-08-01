import { defineConfig } from 'vitest/config';

// The repository contains an embedded static-site checkout whose node:test files are
// intentionally run by that checkout's own CI. Keep the Helper suite deterministic and
// avoid treating those files as Vitest suites when running `npm test` at the repository root.
export default defineConfig({
  test: {
    include: ['tests/**/*.test.ts'],
    exclude: ['site-publish/**', 'node_modules/**'],
    pool: 'forks',
    maxWorkers: 1,
    minWorkers: 1,
  },
});
