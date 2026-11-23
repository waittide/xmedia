import { defineConfig } from 'vite'
import { createSharedAlias, createSharedPlugins, matchVueVendorChunk } from './vite.shared.ts'

export default defineConfig({
  plugins: createSharedPlugins(),
  resolve: {
    alias: createSharedAlias(import.meta.url),
  },
  build: {
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      output: {
        manualChunks(id) {
          const vueVendor = matchVueVendorChunk(id)
          if (vueVendor) return vueVendor
          if (!id.includes('node_modules')) return undefined
          if (id.includes('/node_modules/pdfjs-dist/')) {
            return 'pdf-vendor'
          }
          if (id.includes('/node_modules/epubjs/') || id.includes('/node_modules/jszip/')) {
            return 'epub-vendor'
          }
          if (id.includes('/node_modules/hls.js/')) {
            return 'hls-vendor'
          }
          return undefined
        },
      },
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:8090',
        changeOrigin: true,
        ws: true,
      },
      '/opds': {
        target: 'http://127.0.0.1:8090',
        changeOrigin: true,
      },
      '/rest': {
        target: 'http://127.0.0.1:8090',
        changeOrigin: true,
      },
    },
  },
})
