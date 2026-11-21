import { ref } from 'vue'
import { createI18n } from 'vue-i18n'
import zhCN from './zh-CN'
import enUS from './en-US'
import type { LocaleCode } from '@/types/api'

const SUPPORTED_LOCALES = new Set<LocaleCode>(['zh-CN', 'en-US'])

function readSavedLocale(): LocaleCode {
  try {
    const raw = localStorage.getItem('xmedia.locale')
    if (raw && SUPPORTED_LOCALES.has(raw as LocaleCode)) {
      return raw as LocaleCode
    }
  } catch {
    // ignore storage errors
  }
  return 'zh-CN'
}

const saved: LocaleCode = readSavedLocale()

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
  const normalized: LocaleCode = SUPPORTED_LOCALES.has(code) ? code : 'zh-CN'
  targetLocale = normalized
  try {
    localStorage.setItem('xmedia.locale', normalized)
  } catch {
    // ignore storage errors
  }
  document.documentElement.lang = normalized

  const pending: Promise<void>[] = []
  for (const handle of domainHandles) {
    if (handle.isActive() && !handle.hasLocale(normalized)) {
      pending.push(handle.loadLocale(normalized))
    }
  }

  if (pending.length === 0) {
    commitLocaleSwitch(normalized)
    return
  }

  void Promise.all(pending).then(() => {
    if (targetLocale !== normalized) return
    commitLocaleSwitch(normalized)
  })
}

export function createDomainI18n(loaders: DomainLocaleLoaders) {
  const inst = createI18n({
    legacy: false,
    locale: targetLocale,
    fallbackLocale: 'en-US',
    missingWarn: false,
    fallbackWarn: false,
    messages: {} as Record<string, Record<string, string>>,
  })

  const loadedLocales = new Set<LocaleCode>()
  const failedLocales = new Set<LocaleCode>()
  const inflight = new Map<LocaleCode, Promise<void>>()
  const revision = ref(0)
  let activated = false

  function loadLocale(loc: LocaleCode): Promise<void> {
    const safeLoc: LocaleCode = SUPPORTED_LOCALES.has(loc) ? loc : 'zh-CN'
    activated = true
    if (loadedLocales.has(safeLoc)) {
      if (safeLoc === targetLocale && inst.global.locale.value !== safeLoc) {
        inst.global.locale.value = safeLoc
      }
      return Promise.resolve()
    }
    const existing = inflight.get(safeLoc)
    if (existing) return existing

    failedLocales.delete(safeLoc)
    const req = loaders[safeLoc]()
      .then((mod) => {
        inst.global.setLocaleMessage(safeLoc, mod.default as never)
        loadedLocales.add(safeLoc)
        if (safeLoc === targetLocale) {
          inst.global.locale.value = safeLoc
        }
        revision.value += 1
      })
      .catch(() => {
        failedLocales.add(safeLoc)
      })
      .finally(() => {
        inflight.delete(safeLoc)
      })

    inflight.set(safeLoc, req)
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
    if (!loadedLocales.has(targetLocale) && !failedLocales.has(targetLocale)) {
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
