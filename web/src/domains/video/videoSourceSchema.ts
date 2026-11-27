import type { RuleFieldDef } from '@/components/online-sources/RuleFieldsSection.vue'
import {
  boolVal,
  mergeOptionalField,
  mergeRuleSection,
  numVal,
  readRuleMap,
  strVal,
} from '@/components/online-sources/useOnlineSourceEdit'
import type { SourceEditFourTab } from '@/domains/shared/source-editor/types'
import type { VideoSourceSummaryDto } from './onlineApi'

export type VideoSourceEditTab = SourceEditFourTab

export const CATEGORIES_RULE_FIELDS: RuleFieldDef[] = [
  { key: 'list', label: 'list', placeholder: 'list', fullWidth: true },
  { key: 'id', label: 'id', placeholder: 'id' },
  { key: 'name', label: 'name', placeholder: 'name' },
]

export const SEARCH_RULE_FIELDS: RuleFieldDef[] = [
  { key: 'list', label: 'list', placeholder: 'list', fullWidth: true },
  { key: 'vodId', label: 'vodId', placeholder: 'vodId' },
  { key: 'name', label: 'name', placeholder: 'name' },
  { key: 'coverUrl', label: 'coverUrl', placeholder: 'coverUrl', fullWidth: true },
  { key: 'remarks', label: 'remarks', placeholder: 'remarks' },
  { key: 'year', label: 'year', placeholder: 'year' },
  { key: 'area', label: 'area', placeholder: 'area' },
  { key: 'typeName', label: 'typeName', placeholder: 'typeName' },
  { key: 'detailUrl', label: 'detailUrl', placeholder: 'detailUrl', fullWidth: true },
]

export const DETAIL_RULE_FIELDS: RuleFieldDef[] = [
  { key: 'requestUrl', label: 'requestUrl', placeholder: 'requestUrl', fullWidth: true },
  { key: 'init', label: 'init', placeholder: 'init', multiline: true, rows: 2, fullWidth: true },
  { key: 'name', label: 'name', placeholder: 'name' },
  { key: 'coverUrl', label: 'coverUrl', placeholder: 'coverUrl', fullWidth: true },
  { key: 'year', label: 'year', placeholder: 'year' },
  { key: 'area', label: 'area', placeholder: 'area' },
  { key: 'actors', label: 'actors', placeholder: 'actors' },
  { key: 'director', label: 'director', placeholder: 'director' },
  { key: 'remarks', label: 'remarks', placeholder: 'remarks' },
  { key: 'playGroups', label: 'playGroups', placeholder: 'playGroups', fullWidth: true },
  { key: 'groupName', label: 'groupName', placeholder: 'groupName' },
  {
    key: 'episodes',
    label: 'episodes',
    placeholder: 'episodes',
    multiline: true,
    rows: 2,
    fullWidth: true,
  },
  { key: 'episodeName', label: 'episodeName', placeholder: 'episodeName' },
  { key: 'episodeUrl', label: 'episodeUrl', placeholder: 'episodeUrl' },
  { key: 'playUrl', label: 'playUrl', placeholder: 'playUrl', fullWidth: true },
  { key: 'intro', label: 'intro', placeholder: 'intro', multiline: true, rows: 3, fullWidth: true },
]

export const PLAY_RULE_FIELDS: RuleFieldDef[] = [
  { key: 'requestUrl', label: 'requestUrl', placeholder: 'requestUrl', fullWidth: true },
  { key: 'streamUrl', label: 'streamUrl', placeholder: 'streamUrl', fullWidth: true },
  { key: 'streamType', label: 'streamType', placeholder: 'hls | mp4 | auto' },
  { key: 'headers', label: 'headers', placeholder: 'headers', multiline: true, rows: 2, fullWidth: true },
  {
    key: 'subtitles',
    label: 'subtitles',
    placeholder: 'subtitles',
    multiline: true,
    rows: 2,
    fullWidth: true,
  },
]

function localizeRuleDefs(
  defs: readonly RuleFieldDef[],
  translate: (key: string) => string,
): RuleFieldDef[] {
  return defs.map((d) => ({
    ...d,
    label: translate(`sourceEditor.fields.${d.key}`),
  }))
}

export function buildVideoRuleFields(translate: (key: string) => string) {
  return {
    categories: localizeRuleDefs(CATEGORIES_RULE_FIELDS, translate),
    search: localizeRuleDefs(SEARCH_RULE_FIELDS, translate),
    detail: localizeRuleDefs(DETAIL_RULE_FIELDS, translate),
    play: localizeRuleDefs(PLAY_RULE_FIELDS, translate),
  }
}

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
    ruleCategories: readRuleMap(undefined, CATEGORIES_RULE_FIELDS),
    ruleSearch: readRuleMap(undefined, SEARCH_RULE_FIELDS),
    ruleExplore: readRuleMap(undefined, SEARCH_RULE_FIELDS),
    ruleDetail: readRuleMap(undefined, DETAIL_RULE_FIELDS),
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
  const raw = strVal(obj.sourceFormat).trim().toLowerCase()
  if (raw === 'applecms_v10') return 'applecms_v10'
  if (raw === 'rule') return 'rule'
  if (strVal(obj.apiUrl).trim() && !obj.ruleSearch) {
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
  keyMap.nameKey = 'sourceName'
  keyMap.urlKey = 'sourceUrl'
  keyMap.groupKey = 'sourceGroup'
  keyMap.commentKey = 'sourceComment'
  keyMap.formatKey = 'sourceFormat'
  keyMap.apiUrlKey = 'apiUrl'
  keyMap.mediaHeaderKey = 'mediaHeader'
  keyMap.checkKeywordKey = 'checkKeyword'
  keyMap.categoriesUrlKey = 'categoriesUrl'
  keyMap.exploreUrlKey = 'exploreUrl'

  form.sourceName = strVal(obj.sourceName) || srcSummary?.name || ''
  form.sourceUrl = strVal(obj.sourceUrl) || srcSummary?.source_url || ''
  const rawGroup = obj[keyMap.groupKey]
  form.sourceGroup =
    rawGroup !== undefined ? strVal(rawGroup) : (srcSummary?.group_name ?? '')
  form.sourceComment = strVal(obj.sourceComment)
  form.sourceFormat = resolveVideoFormat(obj, srcSummary)
  form.enabled = srcSummary ? srcSummary.enabled : boolVal(obj.enabled, true)
  form.enabledExplore = boolVal(obj.enabledExplore, true)
  form.enabledCookieJar = boolVal(obj.enabledCookieJar, true)
  form.searchable = boolVal(obj.searchable, true)
  form.weight = srcSummary ? srcSummary.weight : numVal(obj.weight, 0)
  form.concurrentRate = strVal(obj.concurrentRate)
  form.checkKeyword = strVal(obj.checkKeyword)
  form.apiUrl = strVal(obj.apiUrl)
  form.preferredPlayFrom = normalizeListToCsv(obj.preferredPlayFrom)
  form.playUrlFilterRegex = strVal(obj.playUrlFilterRegex)
  form.header = strVal(obj.header)
  form.mediaHeader = strVal(obj.mediaHeader)
  form.loginUrl = strVal(obj.loginUrl)
  form.loginUi = strVal(obj.loginUi)
  form.loginCheckJs = strVal(obj.loginCheckJs)
  form.jsLib = strVal(obj.jsLib)
  form.categoriesUrl = strVal(obj.categoriesUrl)
  form.searchUrl = strVal(obj.searchUrl)
  form.exploreUrl = strVal(obj.exploreUrl)

  form.ruleCategories = readRuleMap(obj.ruleCategories, CATEGORIES_RULE_FIELDS)
  form.ruleSearch = readRuleMap(obj.ruleSearch, SEARCH_RULE_FIELDS)
  form.ruleExplore = readRuleMap(obj.ruleExplore, SEARCH_RULE_FIELDS)
  form.ruleDetail = readRuleMap(obj.ruleDetail, DETAIL_RULE_FIELDS)
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
    [string, Record<string, string>, Record<string, string> | undefined, RuleFieldDef[]]
  > = [
    ['ruleCategories', form.ruleCategories, snap?.ruleCategories, CATEGORIES_RULE_FIELDS],
    ['ruleSearch', form.ruleSearch, snap?.ruleSearch, SEARCH_RULE_FIELDS],
    ['ruleExplore', form.ruleExplore, snap?.ruleExplore, SEARCH_RULE_FIELDS],
    ['ruleDetail', form.ruleDetail, snap?.ruleDetail, DETAIL_RULE_FIELDS],
    ['rulePlay', form.rulePlay, snap?.rulePlay, PLAY_RULE_FIELDS],
  ]

  for (const [key, map, initMap, fields] of sections) {
    const merged = mergeRuleSection(base[key], map, initMap, fields)
    if (merged) out[key] = merged
    else delete out[key]
  }
  return out
}
