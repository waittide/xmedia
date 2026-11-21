import { createDomainI18n } from '@/i18n'

const musicI18n = createDomainI18n({
  'zh-CN': () => import('./locales/zh-CN'),
  'en-US': () => import('./locales/en-US'),
})

export const useMusicI18n = musicI18n.useI18n
export const ensureMusicLocale = musicI18n.ensureLocale
