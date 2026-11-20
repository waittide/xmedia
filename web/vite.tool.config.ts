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
    outDir: 'dist-tool',
    emptyOutDir: true,
    chunkSizeWarningLimit: 500,
    rollupOptions: {
      input: fileURLToPath(new URL('./tool.html', import.meta.url)),
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) return undefined
          if (
            id.includes('/node_modules/vue/') ||
            id.includes('/node_modules/@vue/') ||
            id.includes('/node_modules/pinia/') ||
            id.includes('/node_modules/vue-i18n/') ||
            id.includes('/node_modules/@intlify/')
          ) {
            return 'vue-vendor'
          }
          if (id.includes('/node_modules/vue-icons-plus/')) {
            return 'icons'
          }
          return undefined
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
