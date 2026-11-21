import type { MediaKind } from '@/types/api'
import type { AutoScrapeMatchMode, AutoScrapeOptionsDto } from './types'

const PREFS_STORAGE_KEY = 'xmedia_tool.auto_scrape_prefs.v1'
const LAST_OPTIONS_STORAGE_KEY = 'xmedia_tool.auto_scrape_last_options.v1'

export interface AutoScrapeStoredPrefs {
  match_mode: AutoScrapeMatchMode
  only_fill_missing: boolean
  apply_cover: boolean
  write_sidecar_lrc: boolean
  organize_by_template: boolean
  field_basic: boolean
  field_summary: boolean
  field_tags: boolean
  field_lyrics: boolean
}

export const DEFAULT_AUTO_SCRAPE_PREFS: AutoScrapeStoredPrefs = {
  match_mode: 'standard',
  only_fill_missing: true,
  apply_cover: true,
  write_sidecar_lrc: true,
  organize_by_template: false,
  field_basic: true,
  field_summary: true,
  field_tags: true,
  field_lyrics: true,
}

export function loadAutoScrapePrefs(): AutoScrapeStoredPrefs {
  try {
    const raw = localStorage.getItem(PREFS_STORAGE_KEY)
    if (!raw) return { ...DEFAULT_AUTO_SCRAPE_PREFS }
    const parsed = JSON.parse(raw) as Partial<AutoScrapeStoredPrefs>
    return {
      match_mode:
        parsed.match_mode === 'strict' ||
        parsed.match_mode === 'standard' ||
        parsed.match_mode === 'loose'
          ? parsed.match_mode
          : DEFAULT_AUTO_SCRAPE_PREFS.match_mode,
      only_fill_missing:
        typeof parsed.only_fill_missing === 'boolean'
          ? parsed.only_fill_missing
          : DEFAULT_AUTO_SCRAPE_PREFS.only_fill_missing,
      apply_cover:
        typeof parsed.apply_cover === 'boolean'
          ? parsed.apply_cover
          : DEFAULT_AUTO_SCRAPE_PREFS.apply_cover,
      write_sidecar_lrc:
        typeof parsed.write_sidecar_lrc === 'boolean'
          ? parsed.write_sidecar_lrc
          : DEFAULT_AUTO_SCRAPE_PREFS.write_sidecar_lrc,
      organize_by_template:
        typeof parsed.organize_by_template === 'boolean'
          ? parsed.organize_by_template
          : DEFAULT_AUTO_SCRAPE_PREFS.organize_by_template,
      field_basic:
        typeof parsed.field_basic === 'boolean'
          ? parsed.field_basic
          : DEFAULT_AUTO_SCRAPE_PREFS.field_basic,
      field_summary:
        typeof parsed.field_summary === 'boolean'
          ? parsed.field_summary
          : DEFAULT_AUTO_SCRAPE_PREFS.field_summary,
      field_tags:
        typeof parsed.field_tags === 'boolean'
          ? parsed.field_tags
          : DEFAULT_AUTO_SCRAPE_PREFS.field_tags,
      field_lyrics:
        typeof parsed.field_lyrics === 'boolean'
          ? parsed.field_lyrics
          : DEFAULT_AUTO_SCRAPE_PREFS.field_lyrics,
    }
  } catch {
    return { ...DEFAULT_AUTO_SCRAPE_PREFS }
  }
}

export function saveAutoScrapePrefs(prefs: AutoScrapeStoredPrefs): void {
  try {
    localStorage.setItem(PREFS_STORAGE_KEY, JSON.stringify(prefs))
  } catch {
    // ignore storage quota
  }
}

export function buildAutoScrapeOptionsFromPrefs(
  prefs: AutoScrapeStoredPrefs,
  domain: MediaKind,
  customProviders: string[] = [],
): AutoScrapeOptionsDto {
  const groups: string[] = []
  if (prefs.field_basic) groups.push('basic')
  if (prefs.field_summary) groups.push('summary')
  if (prefs.field_tags) groups.push('tags')
  if (domain === 'music' && prefs.field_lyrics) groups.push('lyrics')
  const fullGroupCount = domain === 'music' ? 4 : 3
  return {
    match_mode: prefs.match_mode,
    only_fill_missing: prefs.only_fill_missing,
    apply_cover: prefs.apply_cover,
    write_sidecar_lrc: domain === 'music' ? prefs.write_sidecar_lrc : false,
    organize_by_template: prefs.organize_by_template,
    providers: [...customProviders],
    field_groups: groups.length === fullGroupCount ? [] : groups,
  }
}

export function saveLastAutoScrapeOptions(domain: MediaKind, options: AutoScrapeOptionsDto): void {
  try {
    const raw = localStorage.getItem(LAST_OPTIONS_STORAGE_KEY)
    const map: Partial<Record<MediaKind, AutoScrapeOptionsDto>> = raw
      ? (JSON.parse(raw) as Partial<Record<MediaKind, AutoScrapeOptionsDto>>)
      : {}
    map[domain] = options
    localStorage.setItem(LAST_OPTIONS_STORAGE_KEY, JSON.stringify(map))
  } catch {
    // ignore storage quota
  }
}

export function loadLastAutoScrapeOptions(domain: MediaKind): AutoScrapeOptionsDto {
  try {
    const raw = localStorage.getItem(LAST_OPTIONS_STORAGE_KEY)
    if (raw) {
      const map = JSON.parse(raw) as Partial<Record<MediaKind, AutoScrapeOptionsDto>>
      const saved = map[domain]
      if (saved && typeof saved === 'object') {
        return { ...saved }
      }
    }
  } catch {
    // fallback to prefs
  }
  return buildAutoScrapeOptionsFromPrefs(loadAutoScrapePrefs(), domain)
}

export function formatAutoScrapeSummary(options: AutoScrapeOptionsDto): string {
  const modeLabel: Record<AutoScrapeMatchMode, string> = {
    strict: '严格',
    standard: '标准',
    loose: '宽松',
  }
  const parts: string[] = [modeLabel[options.match_mode ?? 'standard'] ?? '标准']
  parts.push(options.only_fill_missing === false ? '强制覆盖' : '仅补空')
  if (options.organize_by_template) parts.push('自动归档')
  return parts.join('·')
}
