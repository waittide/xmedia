import type { UserPreferences } from '@/types/api'

export const PREFS_STORAGE_KEY = 'xmedia.preferences'

export type VisualPreferencesSnapshot = Pick<
  UserPreferences,
  'theme_mode' | 'seed_color' | 'app_icon' | 'ui_font' | 'locale' | 'active_domain'
>

export function readStoredPreferencesRaw(): Partial<UserPreferences> {
  try {
    const raw = localStorage.getItem(PREFS_STORAGE_KEY)
    if (!raw) return {}
    const parsed: unknown = JSON.parse(raw)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {}
    return parsed as Partial<UserPreferences>
  } catch {
    return {}
  }
}

export function writeStoredVisualPreferences(visual: VisualPreferencesSnapshot): void {
  try {
    const current = readStoredPreferencesRaw()
    localStorage.setItem(
      PREFS_STORAGE_KEY,
      JSON.stringify({
        ...current,
        ...visual,
      }),
    )
  } catch {
    // ignore storage quota/privacy errors
  }
}
