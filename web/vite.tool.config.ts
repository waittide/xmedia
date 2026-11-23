import { fileURLToPath, URL } from 'node:url'
import { defineConfig } from 'vite'
import { createSharedAlias, createSharedPlugins, matchVueVendorChunk } from './vite.shared.ts'

export default defineConfig({
  plugins: createSharedPlugins(),
  resolve: {
    alias: createSharedAlias(import.meta.url),
  },
  build: {
    outDir: 'dist-tool',
    emptyOutDir: true,
    chunkSizeWarningLimit: 500,
    rollupOptions: {
      input: fileURLToPath(new URL('./tool.html', import.meta.url)),
      output: {
        manualChunks(id) {
          return matchVueVendorChunk(id)
        },
      },
    },
  },
  server: {
    port: 5174,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8091',
        changeOrigin: true,
      },
    },
  },
})
