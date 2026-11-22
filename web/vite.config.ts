import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath, URL } from 'node:url'

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  build: {
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) return undefined
          if (
            id.includes('/node_modules/vue/') ||
            id.includes('/node_modules/@vue/') ||
            id.includes('/node_modules/vue-router/') ||
            id.includes('/node_modules/pinia/') ||
            id.includes('/node_modules/vue-i18n/') ||
            id.includes('/node_modules/@intlify/')
          ) {
            return 'vue-vendor'
          }
          if (id.includes('/node_modules/vue-icons-plus/')) {
            return 'icons'
          }
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
