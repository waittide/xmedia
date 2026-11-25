import type { RuleFieldDef } from '@/components/online-sources/RuleFieldsSection.vue'
import {
  boolVal,
  mergeOptionalField,
  mergeRuleSection,
  numVal,
  pickFirstString,
  pickPresentKey,
  readRuleMap,
  strVal,
} from '@/components/online-sources/useOnlineSourceEdit'
import type { SourceEditFourTab } from '@/domains/shared/source-editor/types'
import type { MusicSourceSummaryDto } from './onlineApi'

export type MusicSourceEditTab = SourceEditFourTab

export const MUSIC_SEARCH_ALIASES: Record<string, readonly string[]> = {
  trackList: ['list', 'songList'],
  trackId: ['id'],
  title: ['name'],
  artist: ['author', 'singer'],
  coverUrl: ['picUrl'],
  coverId: ['picId', 'pic_id'],
  urlId: ['url_id'],
  lyricId: ['lyric_id'],
  playUrl: ['streamUrl'],
  detailUrl: ['trackUrl'],
  checkKeyWord: ['checkKeyword'],
}

export const MUSIC_TRACK_DETAIL_ALIASES: Record<string, readonly string[]> = {
  requestUrl: ['url'],
  trackId: ['id'],
  title: ['name'],
  artist: ['author', 'singer'],
  coverUrl: ['picUrl'],
  coverId: ['picId', 'pic_id'],
  urlId: ['url_id'],
  lyricId: ['lyric_id'],
  playUrl: ['streamUrl'],
}

export const MUSIC_STREAM_ALIASES: Record<string, readonly string[]> = {
  requestUrl: ['url'],
  streamUrl: ['urlRule', 'playUrl'],
}

export const MUSIC_COVER_ALIASES: Record<string, readonly string[]> = {
  requestUrl: ['url'],
  coverUrl: ['urlRule'],
}

export const MUSIC_LYRIC_ALIASES: Record<string, readonly string[]> = {
  requestUrl: ['url'],
  lyric: ['syncedLyrics'],
  translatedLyric: ['tlyric'],
  plainLyric: ['plainLyrics'],
}

interface MusicFieldSpec {
  key: string
  placeholder?: string
  multiline?: boolean
  rows?: number
  fullWidth?: boolean
}

const SEARCH_SPECS: readonly MusicFieldSpec[] = [
  { key: 'checkKeyWord' },
  { key: 'trackList', fullWidth: true },
  { key: 'trackId' },
  { key: 'title' },
  { key: 'artist' },
  { key: 'album' },
  { key: 'duration' },
  { key: 'durationUnit', placeholder: 's | ms' },
  { key: 'coverUrl', fullWidth: true },
  { key: 'coverId' },
  { key: 'urlId' },
  { key: 'lyricId' },
  { key: 'format' },
  { key: 'playUrl', fullWidth: true },
  { key: 'detailUrl', fullWidth: true },
]

const EXPLORE_SPECS: readonly MusicFieldSpec[] = SEARCH_SPECS.filter(
  (f) => f.key !== 'checkKeyWord',
)

const TRACK_DETAIL_SPECS: readonly MusicFieldSpec[] = [
  { key: 'requestUrl', fullWidth: true },
  { key: 'init', multiline: true, rows: 2, fullWidth: true },
  { key: 'trackId' },
  { key: 'title' },
  { key: 'artist' },
  { key: 'album' },
  { key: 'duration' },
  { key: 'durationUnit', placeholder: 's | ms' },
  { key: 'coverUrl', fullWidth: true },
  { key: 'coverId' },
  { key: 'urlId' },
  { key: 'lyricId' },
  { key: 'format' },
  { key: 'playUrl', fullWidth: true },
  { key: 'lyric', multiline: true, rows: 2, fullWidth: true },
]

const STREAM_SPECS: readonly MusicFieldSpec[] = [
  { key: 'requestUrl', fullWidth: true },
  { key: 'streamUrl', multiline: true, rows: 2, fullWidth: true },
  { key: 'format' },
  { key: 'bitrate' },
  { key: 'expiresInSecs' },
  { key: 'headers', multiline: true, rows: 2, fullWidth: true },
]

const COVER_SPECS: readonly MusicFieldSpec[] = [
  { key: 'requestUrl', fullWidth: true },
  { key: 'coverUrl', fullWidth: true },
]

const LYRIC_SPECS: readonly MusicFieldSpec[] = [
  { key: 'requestUrl', fullWidth: true },
  { key: 'lyric', multiline: true, rows: 2, fullWidth: true },
  { key: 'translatedLyric', multiline: true, rows: 2, fullWidth: true },
  { key: 'plainLyric', multiline: true, rows: 2, fullWidth: true },
]

function specsToKeyDefs(specs: readonly MusicFieldSpec[]): RuleFieldDef[] {
  return specs.map((s) => ({ key: s.key, label: s.key }))
}

export const MUSIC_SEARCH_KEY_DEFS = specsToKeyDefs(SEARCH_SPECS)
export const MUSIC_EXPLORE_KEY_DEFS = specsToKeyDefs(EXPLORE_SPECS)
export const MUSIC_DETAIL_KEY_DEFS = specsToKeyDefs(TRACK_DETAIL_SPECS)
export const MUSIC_STREAM_KEY_DEFS = specsToKeyDefs(STREAM_SPECS)
export const MUSIC_COVER_KEY_DEFS = specsToKeyDefs(COVER_SPECS)
export const MUSIC_LYRIC_KEY_DEFS = specsToKeyDefs(LYRIC_SPECS)

function localizeSpecs(
  specs: readonly MusicFieldSpec[],
  translate: (key: string) => string,
): RuleFieldDef[] {
  return specs.map((spec) => ({
    key: spec.key,
    label: translate(`sourceEditor.fields.${spec.key}`),
    placeholder: spec.placeholder ?? spec.key,
    multiline: spec.multiline,
    rows: spec.rows,
    fullWidth: spec.fullWidth,
  }))
}

export function buildMusicRuleFields(translate: (key: string) => string) {
  return {
    search: localizeSpecs(SEARCH_SPECS, translate),
    explore: localizeSpecs(EXPLORE_SPECS, translate),
    trackDetail: localizeSpecs(TRACK_DETAIL_SPECS, translate),
    stream: localizeSpecs(STREAM_SPECS, translate),
    cover: localizeSpecs(COVER_SPECS, translate),
    lyric: localizeSpecs(LYRIC_SPECS, translate),
  }
}

export interface MusicSourceKeyMap {
  nameKey: string
  urlKey: string
  headerKey: string
  mediaHeaderKey: string
}

export interface MusicSourceFormState {
  sourceName: string
  sourceUrl: string
  sourceGroup: string
  sourceComment: string
  enabled: boolean
  enabledCookieJar: boolean
  weight: number | string
  concurrentRate: string
  supportedQualities: string
  header: string
  mediaHeader: string
  loginUrl: string
  loginUi: string
  loginCheckJs: string
  jsLib: string
  searchUrl: string
  exploreUrl: string
  ruleSearch: Record<string, string>
  ruleExplore: Record<string, string>
  ruleTrackDetail: Record<string, string>
  ruleStream: Record<string, string>
  ruleCover: Record<string, string>
  ruleLyric: Record<string, string>
}

export interface MusicSourceSnapshot {
  sourceGroup: string
  sourceComment: string
  enabled: boolean
  enabledCookieJar: boolean
  weight: number
  concurrentRate: string
  supportedQualities: string
  header: string
  mediaHeader: string
  loginUrl: string
  loginUi: string
  loginCheckJs: string
  jsLib: string
  searchUrl: string
  exploreUrl: string
  ruleSearch: Record<string, string>
  ruleExplore: Record<string, string>
  ruleTrackDetail: Record<string, string>
  ruleStream: Record<string, string>
  ruleCover: Record<string, string>
  ruleLyric: Record<string, string>
}

export function createDefaultMusicKeyMap(): MusicSourceKeyMap {
  return {
    nameKey: 'sourceName',
    urlKey: 'sourceUrl',
    headerKey: 'header',
    mediaHeaderKey: 'mediaHeader',
  }
}

export function createDefaultMusicFormState(): MusicSourceFormState {
  return {
    sourceName: '',
    sourceUrl: '',
    sourceGroup: '',
    sourceComment: '',
    enabled: true,
    enabledCookieJar: true,
    weight: 0,
    concurrentRate: '',
    supportedQualities: '',
    header: '',
    mediaHeader: '',
    loginUrl: '',
    loginUi: '',
    loginCheckJs: '',
    jsLib: '',
    searchUrl: '',
    exploreUrl: '',
    ruleSearch: readRuleMap(undefined, MUSIC_SEARCH_KEY_DEFS, MUSIC_SEARCH_ALIASES),
    ruleExplore: readRuleMap(undefined, MUSIC_EXPLORE_KEY_DEFS, MUSIC_SEARCH_ALIASES),
    ruleTrackDetail: readRuleMap(undefined, MUSIC_DETAIL_KEY_DEFS, MUSIC_TRACK_DETAIL_ALIASES),
    ruleStream: readRuleMap(undefined, MUSIC_STREAM_KEY_DEFS, MUSIC_STREAM_ALIASES),
    ruleCover: readRuleMap(undefined, MUSIC_COVER_KEY_DEFS, MUSIC_COVER_ALIASES),
    ruleLyric: readRuleMap(undefined, MUSIC_LYRIC_KEY_DEFS, MUSIC_LYRIC_ALIASES),
  }
}

function normalizeQualitiesToCsv(raw: unknown): string {
  if (Array.isArray(raw)) {
    return raw
      .map((v) => String(v ?? '').trim())
      .filter(Boolean)
      .join(', ')
  }
  return strVal(raw)
}

function formatHeaderText(raw: unknown): string {
  if (raw === null || raw === undefined) return ''
  if (typeof raw === 'string') return raw
  if (typeof raw === 'object') {
    try {
      return JSON.stringify(raw, null, 2)
    } catch {
      return ''
    }
  }
  return strVal(raw)
}

function mergeHeaderField(
  base: Record<string, unknown>,
  out: Record<string, unknown>,
  key: string,
  edited: string,
  initial: string | undefined,
): void {
  if (initial !== undefined && edited === initial) return
  const trimmed = edited.trim()
  if (!trimmed) {
    delete out[key]
    return
  }
  if (typeof base[key] === 'object' || trimmed.startsWith('{')) {
    try {
      const parsed = JSON.parse(trimmed)
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        out[key] = parsed
        return
      }
    } catch {
      // fall back to string
    }
  }
  out[key] = edited
}

export function hydrateMusicSourceForm(
  form: MusicSourceFormState,
  keyMap: MusicSourceKeyMap,
  obj: Record<string, unknown>,
  srcSummary?: MusicSourceSummaryDto,
): MusicSourceSnapshot {
  keyMap.nameKey = pickPresentKey(obj, ['sourceName', 'name'])
  keyMap.urlKey = pickPresentKey(obj, ['sourceUrl', 'url'])
  keyMap.headerKey = pickPresentKey(obj, ['header', 'Header'])
  keyMap.mediaHeaderKey = pickPresentKey(obj, ['mediaHeader', 'MediaHeader'])

  form.sourceName = pickFirstString(obj, ['sourceName', 'name']) || srcSummary?.name || ''
  form.sourceUrl = pickFirstString(obj, ['sourceUrl', 'url']) || srcSummary?.source_url || ''
  form.sourceGroup =
    obj.sourceGroup !== undefined ? strVal(obj.sourceGroup) : (srcSummary?.group_name ?? '')
  form.sourceComment = strVal(obj.sourceComment)
  form.enabled = srcSummary ? srcSummary.enabled : boolVal(obj.enabled, true)
  form.enabledCookieJar = boolVal(obj.enabledCookieJar, true)
  form.weight = srcSummary ? srcSummary.weight : numVal(obj.weight, 0)
  form.concurrentRate = strVal(obj.concurrentRate)
  form.supportedQualities = normalizeQualitiesToCsv(obj.supportedQualities)
  form.header = formatHeaderText(obj[keyMap.headerKey])
  form.mediaHeader = formatHeaderText(obj[keyMap.mediaHeaderKey])
  form.loginUrl = strVal(obj.loginUrl)
  form.loginUi = strVal(obj.loginUi)
  form.loginCheckJs = strVal(obj.loginCheckJs)
  form.jsLib = strVal(obj.jsLib)
  form.searchUrl = strVal(obj.searchUrl)
  form.exploreUrl = strVal(obj.exploreUrl)

  form.ruleSearch = readRuleMap(obj.ruleSearch, MUSIC_SEARCH_KEY_DEFS, MUSIC_SEARCH_ALIASES)
  form.ruleExplore = readRuleMap(obj.ruleExplore, MUSIC_EXPLORE_KEY_DEFS, MUSIC_SEARCH_ALIASES)
  form.ruleTrackDetail = readRuleMap(
    obj.ruleTrackDetail,
    MUSIC_DETAIL_KEY_DEFS,
    MUSIC_TRACK_DETAIL_ALIASES,
  )
  form.ruleStream = readRuleMap(obj.ruleStream, MUSIC_STREAM_KEY_DEFS, MUSIC_STREAM_ALIASES)
  form.ruleCover = readRuleMap(obj.ruleCover, MUSIC_COVER_KEY_DEFS, MUSIC_COVER_ALIASES)
  form.ruleLyric = readRuleMap(obj.ruleLyric, MUSIC_LYRIC_KEY_DEFS, MUSIC_LYRIC_ALIASES)

  return {
    sourceGroup: form.sourceGroup,
    sourceComment: form.sourceComment,
    enabled: form.enabled,
    enabledCookieJar: form.enabledCookieJar,
    weight: numVal(form.weight, 0),
    concurrentRate: form.concurrentRate,
    supportedQualities: form.supportedQualities,
    header: form.header,
    mediaHeader: form.mediaHeader,
    loginUrl: form.loginUrl,
    loginUi: form.loginUi,
    loginCheckJs: form.loginCheckJs,
    jsLib: form.jsLib,
    searchUrl: form.searchUrl,
    exploreUrl: form.exploreUrl,
    ruleSearch: { ...form.ruleSearch },
    ruleExplore: { ...form.ruleExplore },
    ruleTrackDetail: { ...form.ruleTrackDetail },
    ruleStream: { ...form.ruleStream },
    ruleCover: { ...form.ruleCover },
    ruleLyric: { ...form.ruleLyric },
  }
}

export function buildMergedMusicSourceObject(
  base: Record<string, unknown>,
  form: MusicSourceFormState,
  keyMap: MusicSourceKeyMap,
  snap: MusicSourceSnapshot | null,
): Record<string, unknown> {
  const out: Record<string, unknown> = { ...base }
  out[keyMap.nameKey] = form.sourceName.trim()
  out[keyMap.urlKey] = form.sourceUrl.trim()
  mergeOptionalField(base, out, 'sourceGroup', form.sourceGroup, snap?.sourceGroup, true)
  mergeOptionalField(base, out, 'sourceComment', form.sourceComment, snap?.sourceComment)

  if ('enabled' in base || !snap || form.enabled !== snap.enabled) {
    out.enabled = form.enabled
  }
  if ('enabledCookieJar' in base || !snap || form.enabledCookieJar !== snap.enabledCookieJar) {
    out.enabledCookieJar = form.enabledCookieJar
  }
  const normalizedWeight = numVal(form.weight, 0)
  if ('weight' in base || !snap || normalizedWeight !== snap.weight) {
    out.weight = normalizedWeight
  }

  if (!snap || form.supportedQualities !== snap.supportedQualities) {
    const items = form.supportedQualities
      .split(/[,;|]/)
      .map((s) => s.trim())
      .filter(Boolean)
    if (items.length > 0) out.supportedQualities = items
    else delete out.supportedQualities
  }

  mergeOptionalField(base, out, 'concurrentRate', form.concurrentRate, snap?.concurrentRate, true)
  mergeHeaderField(base, out, keyMap.headerKey, form.header, snap?.header)
  mergeHeaderField(base, out, keyMap.mediaHeaderKey, form.mediaHeader, snap?.mediaHeader)
  mergeOptionalField(base, out, 'loginUrl', form.loginUrl, snap?.loginUrl)
  mergeOptionalField(base, out, 'loginUi', form.loginUi, snap?.loginUi)
  mergeOptionalField(base, out, 'loginCheckJs', form.loginCheckJs, snap?.loginCheckJs)
  mergeOptionalField(base, out, 'jsLib', form.jsLib, snap?.jsLib)
  mergeOptionalField(base, out, 'searchUrl', form.searchUrl, snap?.searchUrl)
  mergeOptionalField(base, out, 'exploreUrl', form.exploreUrl, snap?.exploreUrl)

  const sections: Array<
    [
      string,
      Record<string, string>,
      Record<string, string> | undefined,
      RuleFieldDef[],
      Record<string, readonly string[]>,
    ]
  > = [
    ['ruleSearch', form.ruleSearch, snap?.ruleSearch, MUSIC_SEARCH_KEY_DEFS, MUSIC_SEARCH_ALIASES],
    [
      'ruleExplore',
      form.ruleExplore,
      snap?.ruleExplore,
      MUSIC_EXPLORE_KEY_DEFS,
      MUSIC_SEARCH_ALIASES,
    ],
    [
      'ruleTrackDetail',
      form.ruleTrackDetail,
      snap?.ruleTrackDetail,
      MUSIC_DETAIL_KEY_DEFS,
      MUSIC_TRACK_DETAIL_ALIASES,
    ],
    ['ruleStream', form.ruleStream, snap?.ruleStream, MUSIC_STREAM_KEY_DEFS, MUSIC_STREAM_ALIASES],
    ['ruleCover', form.ruleCover, snap?.ruleCover, MUSIC_COVER_KEY_DEFS, MUSIC_COVER_ALIASES],
    ['ruleLyric', form.ruleLyric, snap?.ruleLyric, MUSIC_LYRIC_KEY_DEFS, MUSIC_LYRIC_ALIASES],
  ]
  for (const [key, map, initMap, fields, aliases] of sections) {
    const merged = mergeRuleSection(base[key], map, initMap, fields, aliases)
    if (merged) out[key] = merged
    else delete out[key]
  }
  return out
}
