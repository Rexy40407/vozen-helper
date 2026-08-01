// eslint.config.mjs — flat config. Regras recommended SEM type-checking (rápido;
// as type-aware ficam para depois). O Prettier trata do estilo. Espelha o projeto
// irmão Vozen-bot para consistência entre os dois bots.
import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import prettier from 'eslint-config-prettier';

export default tseslint.config(
  {
    ignores: [
      'dist/',
      'node_modules/',
      'scratchpad/',
      'panel/dist/',
      'panel/node_modules/',
      'site-publish/',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  prettier,
  {
    // Afinações para o código deste repo (só afrouxar, sem mexer em lógica):
    rules: {
      // O TypeScript já verifica identificadores indefinidos melhor que o ESLint
      // (e conhece os globais Node via @types/node). no-undef só dá falsos
      // positivos em `process`/`console` nos scripts .mjs.
      'no-undef': 'off',
      // Placeholders com prefixo `_` são intencionais (args de callbacks aceites
      // mas não usados, ex.: `_client`, `_old`).
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' },
      ],
      // `void this.playNext()` / atribuições em sequência usadas de propósito.
      '@typescript-eslint/no-unused-expressions': 'off',
      // As classes de emoji em normalize.ts usam code points combinantes de propósito.
      'no-misleading-character-class': 'off',
      // normalize.ts/contentFilter.ts contêm caracteres invisíveis (zero-width) DE
      // PROPÓSITO nos regex anti-evasão. Permite-os em regex/strings; ainda apanha
      // whitespace irregular acidental no código normal.
      'no-irregular-whitespace': [
        'error',
        { skipStrings: true, skipComments: true, skipRegExps: true, skipTemplates: true },
      ],
    },
  },
  {
    // Nos testes, stubs/mocks usam `any` e casts de propósito.
    files: ['tests/**/*.ts'],
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
    },
  },
);
