import { fileURLToPath, URL } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'
import type { PluginOption } from 'vite'

export function createSharedPlugins(): PluginOption[] {
  return [vue(), tailwindcss()]
}

export function createSharedAlias(metaUrl: string): Record<string, string> {
  return {
    '@': fileURLToPath(new URL('./src', metaUrl)),
  }
}

export function matchVueVendorChunk(id: string): string | undefined {
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
  return undefined
}
