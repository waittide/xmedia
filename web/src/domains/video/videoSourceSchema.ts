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
import type { VideoSourceSummaryDto } from './onlineApi'

export type VideoSourceEditTab =
  | 'basic'
  | 'categories'
  | 'search'
  | 'explore'
  | 'detail'
  | 'play'
  | 'json'

export const CATEGORIES_ALIASES: Record<string, readonly string[]> = {
  list: ['bookList'],
  name: ['title'],
}

export const SEARCH_ALIASES: Record<string, readonly string[]> = {
  list: ['bookList'],
  vodId: ['id'],
  name: ['title'],
  coverUrl: ['posterUrl'],
  typeName: ['category'],
  detailUrl: ['bookUrl'],
}

export const DETAIL_ALIASES: Record<string, readonly string[]> = {
  name: ['title'],
  coverUrl: ['posterUrl'],
  intro: ['description'],
  playGroups: ['groups'],
  episodes: ['chapterList'],
  episodeName: ['chapterName'],
  episodeUrl: ['chapterUrl'],
}

export const CATEGORIES_RULE_FIELDS: RuleFieldDef[] = [
  { key: 'list', label: 'list', fullWidth: true },
  { key: 'id', label: 'id' },
  { key: 'name', label: 'name / title' },
]

export const SEARCH_RULE_FIELDS: RuleFieldDef[] = [
  { key: 'list', label: 'list', fullWidth: true },
  { key: 'vodId', label: 'vodId' },
  { key: 'name', label: 'name / title' },
  { key: 'coverUrl', label: 'coverUrl', fullWidth: true },
  { key: 'remarks', label: 'remarks' },
  { key: 'year', label: 'year' },
  { key: 'area', label: 'area' },
  { key: 'typeName', label: 'typeName' },
  { key: 'detailUrl', label: 'detailUrl', fullWidth: true },
]

export const DETAIL_RULE_FIELDS: RuleFieldDef[] = [
  { key: 'requestUrl', label: 'requestUrl', fullWidth: true },
  { key: 'init', label: 'init', multiline: true, rows: 2, fullWidth: true },
  { key: 'name', label: 'name / title' },
  { key: 'coverUrl', label: 'coverUrl', fullWidth: true },
  { key: 'year', label: 'year' },
  { key: 'area', label: 'area' },
  { key: 'actors', label: 'actors' },
  { key: 'director', label: 'director' },
  { key: 'remarks', label: 'remarks' },
  { key: 'playGroups', label: 'playGroups', fullWidth: true },
  { key: 'groupName', label: 'groupName' },
  { key: 'episodes', label: 'episodes', multiline: true, rows: 2, fullWidth: true },
  { key: 'episodeName', label: 'episodeName' },
  { key: 'episodeUrl', label: 'episodeUrl' },
  { key: 'playUrl', label: 'playUrl', fullWidth: true },
  { key: 'intro', label: 'intro', multiline: true, rows: 3, fullWidth: true },
]

export const PLAY_RULE_FIELDS: RuleFieldDef[] = [
  { key: 'requestUrl', label: 'requestUrl', fullWidth: true },
  { key: 'streamUrl', label: 'streamUrl', fullWidth: true },
  { key: 'streamType', label: 'streamType', placeholder: 'hls | mp4 | auto' },
  { key: 'headers', label: 'headers', multiline: true, rows: 2, fullWidth: true },
  { key: 'subtitles', label: 'subtitles', multiline: true, rows: 2, fullWidth: true },
]

export interface VideoSourceKeyMap {
  nameKey: string
  urlKey: string
  groupKey: string
  commentKey: string
  formatKey: string
  apiUrlKey: string
  mediaHeaderKey: string
  checkKeywordKey: string
  categoriesUrlKey: string
  exploreUrlKey: string
}

export interface VideoSourceFormState {
  sourceName: string
  sourceUrl: string
  sourceGroup: string
  sourceComment: string
  sourceFormat: string
  enabled: boolean
  enabledExplore: boolean
  enabledCookieJar: boolean
  searchable: boolean
  weight: number | string
  concurrentRate: string
  checkKeyword: string
  apiUrl: string
  preferredPlayFrom: string
  playUrlFilterRegex: string
  header: string
  mediaHeader: string
  loginUrl: string
  loginUi: string
  loginCheckJs: string
  jsLib: string
  categoriesUrl: string
  searchUrl: string
  exploreUrl: string
  ruleCategories: Record<string, string>
  ruleSearch: Record<string, string>
  ruleExplore: Record<string, string>
  ruleDetail: Record<string, string>
  rulePlay: Record<string, string>
}

export interface VideoSourceSnapshot {
  sourceGroup: string
  sourceComment: string
  sourceFormat: string
  enabled: boolean
  enabledExplore: boolean
  enabledCookieJar: boolean
  searchable: boolean
  weight: number
  concurrentRate: string
  checkKeyword: string
  apiUrl: string
  preferredPlayFrom: string
  playUrlFilterRegex: string
  header: string
  mediaHeader: string
  loginUrl: string
  loginUi: string
  loginCheckJs: string
  jsLib: string
  categoriesUrl: string
  searchUrl: string
  exploreUrl: string
  ruleCategories: Record<string, string>
  ruleSearch: Record<string, string>
  ruleExplore: Record<string, string>
  ruleDetail: Record<string, string>
  rulePlay: Record<string, string>
}

export function createDefaultKeyMap(): VideoSourceKeyMap {
  return {
    nameKey: 'sourceName',
    urlKey: 'sourceUrl',
    groupKey: 'sourceGroup',
    commentKey: 'sourceComment',
    formatKey: 'sourceFormat',
    apiUrlKey: 'apiUrl',
    mediaHeaderKey: 'mediaHeader',
    checkKeywordKey: 'checkKeyword',
    categoriesUrlKey: 'categoriesUrl',
    exploreUrlKey: 'exploreUrl',
  }
}

export function createDefaultFormState(): VideoSourceFormState {
  return {
    sourceName: '',
    sourceUrl: '',
    sourceGroup: '',
    sourceComment: '',
    sourceFormat: 'applecms_v10',
    enabled: true,
    enabledExplore: true,
    enabledCookieJar: true,
    searchable: true,
    weight: 0,
    concurrentRate: '',
    checkKeyword: '',
    apiUrl: '',
    preferredPlayFrom: '',
    playUrlFilterRegex: '',
    header: '',
    mediaHeader: '',
    loginUrl: '',
    loginUi: '',
    loginCheckJs: '',
    jsLib: '',
    categoriesUrl: '',
    searchUrl: '',
    exploreUrl: '',
    ruleCategories: readRuleMap(undefined, CATEGORIES_RULE_FIELDS, CATEGORIES_ALIASES),
    ruleSearch: readRuleMap(undefined, SEARCH_RULE_FIELDS, SEARCH_ALIASES),
    ruleExplore: readRuleMap(undefined, SEARCH_RULE_FIELDS, SEARCH_ALIASES),
    ruleDetail: readRuleMap(undefined, DETAIL_RULE_FIELDS, DETAIL_ALIASES),
    rulePlay: readRuleMap(undefined, PLAY_RULE_FIELDS),
  }
}

function normalizeListToCsv(raw: unknown): string {
  if (Array.isArray(raw)) {
    return raw.map((v) => String(v ?? '').trim()).filter(Boolean).join(', ')
  }
  return strVal(raw)
}

export function resolveVideoFormat(
  obj: Record<string, unknown>,
  srcSummary?: VideoSourceSummaryDto,
): string {
  const raw = pickFirstString(obj, ['sourceFormat', 'mode']).trim().toLowerCase()
  if (raw === 'applecms_v10' || raw === 'applecms' || raw === 'applecmsv10') {
    return 'applecms_v10'
  }
  if (raw === 'rule') return 'rule'
  if (pickFirstString(obj, ['apiUrl', 'api_url']).trim() && !obj.ruleSearch) {
    return 'applecms_v10'
  }
  if (srcSummary && srcSummary.source_type === 0) return 'applecms_v10'
  return 'rule'
}

export function captureVideoSourceSnapshot(form: VideoSourceFormState): VideoSourceSnapshot {
  return {
    sourceGroup: form.sourceGroup,
    sourceComment: form.sourceComment,
    sourceFormat: form.sourceFormat,
    enabled: form.enabled,
    enabledExplore: form.enabledExplore,
    enabledCookieJar: form.enabledCookieJar,
    searchable: form.searchable,
    weight: numVal(form.weight, 0),
    concurrentRate: form.concurrentRate,
    checkKeyword: form.checkKeyword,
    apiUrl: form.apiUrl,
    preferredPlayFrom: form.preferredPlayFrom,
    playUrlFilterRegex: form.playUrlFilterRegex,
    header: form.header,
    mediaHeader: form.mediaHeader,
    loginUrl: form.loginUrl,
    loginUi: form.loginUi,
    loginCheckJs: form.loginCheckJs,
    jsLib: form.jsLib,
    categoriesUrl: form.categoriesUrl,
    searchUrl: form.searchUrl,
    exploreUrl: form.exploreUrl,
    ruleCategories: { ...form.ruleCategories },
    ruleSearch: { ...form.ruleSearch },
    ruleExplore: { ...form.ruleExplore },
    ruleDetail: { ...form.ruleDetail },
    rulePlay: { ...form.rulePlay },
  }
}

export function hydrateVideoSourceForm(
  form: VideoSourceFormState,
  keyMap: VideoSourceKeyMap,
  obj: Record<string, unknown>,
  srcSummary?: VideoSourceSummaryDto,
): VideoSourceSnapshot {
  keyMap.nameKey = pickPresentKey(obj, ['sourceName', 'name', 'bookSourceName'])
  keyMap.urlKey = pickPresentKey(obj, ['sourceUrl', 'url', 'bookSourceUrl'])
  keyMap.groupKey = pickPresentKey(obj, ['sourceGroup', 'group', 'bookSourceGroup'])
  keyMap.commentKey = pickPresentKey(obj, ['sourceComment', 'comment', 'bookSourceComment'])
  keyMap.formatKey = pickPresentKey(obj, ['sourceFormat', 'mode'])
  keyMap.apiUrlKey = pickPresentKey(obj, ['apiUrl', 'api_url'])
  keyMap.mediaHeaderKey = pickPresentKey(obj, ['mediaHeader', 'streamHeader'])
  keyMap.checkKeywordKey = pickPresentKey(obj, ['checkKeyword', 'checkKeyWord'])
  keyMap.categoriesUrlKey = pickPresentKey(obj, ['categoriesUrl', 'categoryUrl'])
  keyMap.exploreUrlKey = pickPresentKey(obj, ['exploreUrl', 'browseUrl'])

  form.sourceName =
    pickFirstString(obj, ['sourceName', 'name', 'bookSourceName']) || srcSummary?.name || ''
  form.sourceUrl =
    pickFirstString(obj, ['sourceUrl', 'url', 'bookSourceUrl']) || srcSummary?.source_url || ''
  const rawGroup = obj[keyMap.groupKey]
  form.sourceGroup =
    rawGroup !== undefined ? strVal(rawGroup) : (srcSummary?.group_name ?? '')
  form.sourceComment = pickFirstString(obj, ['sourceComment', 'comment', 'bookSourceComment'])
  form.sourceFormat = resolveVideoFormat(obj, srcSummary)
  form.enabled = srcSummary ? srcSummary.enabled : boolVal(obj.enabled, true)
  form.enabledExplore = boolVal(obj.enabledExplore, true)
  form.enabledCookieJar = boolVal(obj.enabledCookieJar, true)
  form.searchable = boolVal(obj.searchable, true)
  form.weight = srcSummary ? srcSummary.weight : numVal(obj.weight, 0)
  form.concurrentRate = strVal(obj.concurrentRate)
  form.checkKeyword = pickFirstString(obj, ['checkKeyword', 'checkKeyWord'])
  form.apiUrl = pickFirstString(obj, ['apiUrl', 'api_url'])
  form.preferredPlayFrom = normalizeListToCsv(obj.preferredPlayFrom)
  form.playUrlFilterRegex = strVal(obj.playUrlFilterRegex)
  form.header = strVal(obj.header)
  form.mediaHeader = pickFirstString(obj, ['mediaHeader', 'streamHeader'])
  form.loginUrl = strVal(obj.loginUrl)
  form.loginUi = strVal(obj.loginUi)
  form.loginCheckJs = strVal(obj.loginCheckJs)
  form.jsLib = strVal(obj.jsLib)
  form.categoriesUrl = pickFirstString(obj, ['categoriesUrl', 'categoryUrl'])
  form.searchUrl = strVal(obj.searchUrl)
  form.exploreUrl = pickFirstString(obj, ['exploreUrl', 'browseUrl'])

  form.ruleCategories = readRuleMap(obj.ruleCategories, CATEGORIES_RULE_FIELDS, CATEGORIES_ALIASES)
  form.ruleSearch = readRuleMap(obj.ruleSearch, SEARCH_RULE_FIELDS, SEARCH_ALIASES)
  form.ruleExplore = readRuleMap(obj.ruleExplore, SEARCH_RULE_FIELDS, SEARCH_ALIASES)
  form.ruleDetail = readRuleMap(obj.ruleDetail, DETAIL_RULE_FIELDS, DETAIL_ALIASES)
  form.rulePlay = readRuleMap(obj.rulePlay, PLAY_RULE_FIELDS)

  return captureVideoSourceSnapshot(form)
}

export function buildMergedVideoSourceObject(
  base: Record<string, unknown>,
  form: VideoSourceFormState,
  keyMap: VideoSourceKeyMap,
  snap: VideoSourceSnapshot | null,
): Record<string, unknown> {
  const out: Record<string, unknown> = { ...base }
  out[keyMap.nameKey] = form.sourceName.trim()
  out[keyMap.urlKey] = form.sourceUrl.trim()
  mergeOptionalField(base, out, keyMap.groupKey, form.sourceGroup, snap?.sourceGroup, true)
  mergeOptionalField(base, out, keyMap.commentKey, form.sourceComment, snap?.sourceComment)

  if (keyMap.formatKey in base || !snap || form.sourceFormat !== snap.sourceFormat) {
    out[keyMap.formatKey] = form.sourceFormat
  }
  if ('enabled' in base || !snap || form.enabled !== snap.enabled) {
    out.enabled = form.enabled
  }
  if ('enabledExplore' in base || !snap || form.enabledExplore !== snap.enabledExplore) {
    out.enabledExplore = form.enabledExplore
  }
  if ('enabledCookieJar' in base || !snap || form.enabledCookieJar !== snap.enabledCookieJar) {
    out.enabledCookieJar = form.enabledCookieJar
  }
  if ('searchable' in base || !snap || form.searchable !== snap.searchable) {
    out.searchable = form.searchable
  }
  const normalizedWeight = numVal(form.weight, 0)
  if ('weight' in base || !snap || normalizedWeight !== snap.weight) {
    out.weight = normalizedWeight
  }

  mergeOptionalField(base, out, 'concurrentRate', form.concurrentRate, snap?.concurrentRate, true)
  mergeOptionalField(base, out, keyMap.checkKeywordKey, form.checkKeyword, snap?.checkKeyword, true)
  mergeOptionalField(base, out, keyMap.apiUrlKey, form.apiUrl, snap?.apiUrl, true)

  if (!snap || form.preferredPlayFrom !== snap.preferredPlayFrom) {
    const items = form.preferredPlayFrom
      .split(/[,，|]/)
      .map((s) => s.trim())
      .filter(Boolean)
    if (items.length > 0) out.preferredPlayFrom = items
    else delete out.preferredPlayFrom
  }

  mergeOptionalField(
    base,
    out,
    'playUrlFilterRegex',
    form.playUrlFilterRegex,
    snap?.playUrlFilterRegex,
  )
  mergeOptionalField(base, out, 'header', form.header, snap?.header)
  mergeOptionalField(base, out, keyMap.mediaHeaderKey, form.mediaHeader, snap?.mediaHeader)
  mergeOptionalField(base, out, 'loginUrl', form.loginUrl, snap?.loginUrl)
  mergeOptionalField(base, out, 'loginUi', form.loginUi, snap?.loginUi)
  mergeOptionalField(base, out, 'loginCheckJs', form.loginCheckJs, snap?.loginCheckJs)
  mergeOptionalField(base, out, 'jsLib', form.jsLib, snap?.jsLib)
  mergeOptionalField(base, out, keyMap.categoriesUrlKey, form.categoriesUrl, snap?.categoriesUrl)
  mergeOptionalField(base, out, 'searchUrl', form.searchUrl, snap?.searchUrl)
  mergeOptionalField(base, out, keyMap.exploreUrlKey, form.exploreUrl, snap?.exploreUrl)

  const sections: Array<
    [
      string,
      Record<string, string>,
      Record<string, string> | undefined,
      RuleFieldDef[],
      Record<string, readonly string[]> | undefined,
    ]
  > = [
    [
      'ruleCategories',
      form.ruleCategories,
      snap?.ruleCategories,
      CATEGORIES_RULE_FIELDS,
      CATEGORIES_ALIASES,
    ],
    ['ruleSearch', form.ruleSearch, snap?.ruleSearch, SEARCH_RULE_FIELDS, SEARCH_ALIASES],
    ['ruleExplore', form.ruleExplore, snap?.ruleExplore, SEARCH_RULE_FIELDS, SEARCH_ALIASES],
    ['ruleDetail', form.ruleDetail, snap?.ruleDetail, DETAIL_RULE_FIELDS, DETAIL_ALIASES],
    ['rulePlay', form.rulePlay, snap?.rulePlay, PLAY_RULE_FIELDS, undefined],
  ]

  for (const [key, map, initMap, fields, aliases] of sections) {
    const merged = mergeRuleSection(base[key], map, initMap, fields, aliases)
    if (merged) out[key] = merged
    else delete out[key]
  }
  return out
}
