import { createDomainI18n } from '@/i18n'

const bookI18n = createDomainI18n({
  'zh-CN': () => import('./locales/zh-CN'),
  'en-US': () => import('./locales/en-US'),
})

export const useBookI18n = bookI18n.useI18n
export const ensureBookLocale = bookI18n.ensureLocale
