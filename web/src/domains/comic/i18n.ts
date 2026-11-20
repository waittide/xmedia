import { createDomainI18n } from '@/i18n'

const comicI18n = createDomainI18n({
  'zh-CN': () => import('./locales/zh-CN'),
  'en-US': () => import('./locales/en-US'),
})

export const useComicI18n = comicI18n.useI18n
export const ensureComicLocale = comicI18n.ensureLocale
