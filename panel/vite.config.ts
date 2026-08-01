import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  // GitHub Pages serves this project under /vozen-helper-bot/.
  // Relative asset URLs also keep the Studio usable from a local preview.
  base: './',
  plugins: [react()],
});
