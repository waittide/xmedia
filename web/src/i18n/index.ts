import { createI18n } from 'vue-i18n'
import zhCN from './zh-CN'
import enUS from './en-US'
import type { LocaleCode } from '@/types/api'

const saved = (localStorage.getItem('xmedia.locale') as LocaleCode | null) || 'zh-CN'

export const i18n = createI18n({
  legacy: false,
  locale: saved,
  fallbackLocale: 'en-US',
  messages: {
    'zh-CN': zhCN,
    'en-US': enUS,
  },
})

const localeListeners: Array<(loc: LocaleCode) => void> = []

export function registerDomainLocaleListener(fn: (loc: LocaleCode) => void): void {
  localeListeners.push(fn)
}

export function setAppLocale(code: LocaleCode): void {
  i18n.global.locale.value = code
  localStorage.setItem('xmedia.locale', code)
  document.documentElement.lang = code
  for (const fn of localeListeners) {
    fn(code)
  }
}

export function createDomainI18n<T extends Record<string, unknown>>(zh: T, en: T) {
  const inst = createI18n({
    legacy: false,
    locale: i18n.global.locale.value,
    fallbackLocale: 'en-US',
    messages: {
      'zh-CN': zh as unknown as Record<string, string>,
      'en-US': en as unknown as Record<string, string>,
    },
  })
  registerDomainLocaleListener((loc) => {
    inst.global.locale.value = loc
  })
  const translate = (key: string, named?: Record<string, unknown>): string =>
    named ? inst.global.t(key, named) : inst.global.t(key)
  return () => ({
    t: translate,
    locale: inst.global.locale,
  })
}
