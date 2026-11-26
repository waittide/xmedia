import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import { i18n } from '@/i18n'
import { configureCustomFontsApiBase, ensureCustomFontsLoaded } from '@/theme/customFonts'
import toolEnUS from './locales/en-US'
import toolZhCN from './locales/zh-CN'
import { toolRouter } from './router'
import '@/styles/main.css'

configureCustomFontsApiBase('/api/fonts')
void ensureCustomFontsLoaded()

i18n.global.mergeLocaleMessage('zh-CN', { tool: toolZhCN })
i18n.global.mergeLocaleMessage('en-US', { tool: toolEnUS })

const app = createApp(App)
app.use(createPinia())
app.use(i18n)
app.use(toolRouter)
app.mount('#app')
