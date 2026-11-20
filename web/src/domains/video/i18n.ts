import { createDomainI18n } from '@/i18n'

const videoI18n = createDomainI18n({
  'zh-CN': () => import('./locales/zh-CN'),
  'en-US': () => import('./locales/en-US'),
})

export const useVideoI18n = videoI18n.useI18n
export const ensureVideoLocale = videoI18n.ensureLocale
