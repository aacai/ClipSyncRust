import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1440,
    strictPort: true,
  },
  build: {
    rollupOptions: {
      input: {
        main: 'index.html',
        timer: 'timer.html',
        picker: 'picker.html',
        preview: 'preview.html',
      },
    },
  },
});
