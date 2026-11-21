import { ref } from 'vue'
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

export type DomainLocaleLoaders = Record<
  LocaleCode,
  () => Promise<{ default: Record<string, unknown> }>
>

interface DomainI18nHandle {
  isActive: () => boolean
  hasLocale: (loc: LocaleCode) => boolean
  loadLocale: (loc: LocaleCode) => Promise<void>
  applyLocale: (loc: LocaleCode) => void
}

let targetLocale: LocaleCode = saved
const domainHandles = new Set<DomainI18nHandle>()

export function getTargetLocale(): LocaleCode {
  return targetLocale
}

function commitLocaleSwitch(code: LocaleCode): void {
  i18n.global.locale.value = code
  for (const handle of domainHandles) {
    if (handle.hasLocale(code)) {
      handle.applyLocale(code)
    }
  }
}

export function setAppLocale(code: LocaleCode): void {
  targetLocale = code
  localStorage.setItem('xmedia.locale', code)
  document.documentElement.lang = code

  const pending: Promise<void>[] = []
  for (const handle of domainHandles) {
    if (handle.isActive() && !handle.hasLocale(code)) {
      pending.push(handle.loadLocale(code))
    }
  }

  if (pending.length === 0) {
    commitLocaleSwitch(code)
    return
  }

  void Promise.all(pending).then(() => {
    if (targetLocale !== code) return
    commitLocaleSwitch(code)
  })
}

export function createDomainI18n(loaders: DomainLocaleLoaders) {
  const inst = createI18n({
    legacy: false,
    locale: targetLocale,
    fallbackLocale: targetLocale,
    missingWarn: false,
    fallbackWarn: false,
    messages: {} as Record<string, Record<string, string>>,
  })

  const loadedLocales = new Set<LocaleCode>()
  const inflight = new Map<LocaleCode, Promise<void>>()
  const revision = ref(0)
  let activated = false

  function loadLocale(loc: LocaleCode): Promise<void> {
    activated = true
    if (loadedLocales.has(loc)) {
      if (loc === targetLocale && inst.global.locale.value !== loc) {
        inst.global.locale.value = loc
      }
      return Promise.resolve()
    }
    const existing = inflight.get(loc)
    if (existing) return existing

    const req = loaders[loc]()
      .then((mod) => {
        inst.global.setLocaleMessage(loc, mod.default as never)
        loadedLocales.add(loc)
        if (loc === targetLocale) {
          inst.global.locale.value = loc
        }
        revision.value += 1
      })
      .finally(() => {
        inflight.delete(loc)
      })

    inflight.set(loc, req)
    return req
  }

  function ensureLocale(loc: LocaleCode = targetLocale): Promise<void> {
    return loadLocale(loc)
  }

  const handle: DomainI18nHandle = {
    isActive: () => activated,
    hasLocale: (loc) => loadedLocales.has(loc),
    loadLocale,
    applyLocale: (loc) => {
      inst.global.locale.value = loc
    },
  }
  domainHandles.add(handle)

  const translate = (key: string, named?: Record<string, unknown>): string => {
    // Track revision so any late-loaded locale re-renders reactively without stale keys.
    void revision.value
    if (!loadedLocales.has(targetLocale)) {
      void loadLocale(targetLocale)
    }
    return named ? inst.global.t(key, named) : inst.global.t(key)
  }

  const useI18n = () => ({
    t: translate,
    locale: inst.global.locale,
    ensureLocale,
  })

  return {
    useI18n,
    ensureLocale,
  }
}
