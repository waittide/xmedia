import { createI18n } from 'vue-i18n'
import zhCN from './zh-CN'
import enUS from './en-US'

const saved = (localStorage.getItem('xmedia.locale') as 'zh-CN' | 'en-US' | null) || 'zh-CN'

export const i18n = createI18n({
  legacy: false,
  locale: saved,
  fallbackLocale: 'en-US',
  messages: {
    'zh-CN': zhCN,
    'en-US': enUS,
  },
})
