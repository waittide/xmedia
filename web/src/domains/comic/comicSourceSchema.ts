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
import type { ComicSourceSummaryDto } from './onlineApi'

export type ComicSourceEditTab = SourceEditFourTab

interface ComicRuleSpec {
  key: string
  placeholder?: string
  multiline?: boolean
  rows?: number
  fullWidth?: boolean
}

const SEARCH_SPECS: readonly ComicRuleSpec[] = [
  { key: 'checkKeyWord' },
  { key: 'bookList', fullWidth: true },
  { key: 'name' },
  { key: 'author' },
  { key: 'bookUrl', fullWidth: true },
  { key: 'coverUrl', fullWidth: true },
  { key: 'kind' },
  { key: 'wordCount' },
  { key: 'lastChapter', fullWidth: true },
  { key: 'intro', multiline: true, rows: 2, fullWidth: true },
]

const EXPLORE_SPECS: readonly ComicRuleSpec[] = SEARCH_SPECS.filter(
  (f) => f.key !== 'checkKeyWord',
)

const DETAIL_SPECS: readonly ComicRuleSpec[] = [
  { key: 'init', multiline: true, rows: 2, fullWidth: true },
  { key: 'name' },
  { key: 'author' },
  { key: 'tocUrl', fullWidth: true },
  { key: 'coverUrl', fullWidth: true },
  { key: 'kind' },
  { key: 'status' },
  { key: 'readingDirection' },
  { key: 'wordCount' },
  { key: 'lastChapter' },
  { key: 'canReName' },
  { key: 'intro', multiline: true, rows: 3, fullWidth: true },
]

const TOC_SPECS: readonly ComicRuleSpec[] = [
  { key: 'chapterList', fullWidth: true },
  { key: 'chapterName' },
  { key: 'chapterUrl' },
  { key: 'nextTocUrl', fullWidth: true },
  { key: 'updateTime' },
  { key: 'isVolume' },
  { key: 'isVip' },
  { key: 'preUpdateJs', multiline: true, rows: 3, fullWidth: true },
]

const PAGES_SPECS: readonly ComicRuleSpec[] = [
  { key: 'content', multiline: true, rows: 4, fullWidth: true },
  { key: 'title' },
  { key: 'imageUrl' },
  { key: 'nextContentUrl' },
  { key: 'imageHeader', multiline: true, rows: 2, fullWidth: true },
  { key: 'replaceRegex', multiline: true, rows: 2, fullWidth: true },
  { key: 'sourceRegex', fullWidth: true },
  { key: 'webJs', multiline: true, rows: 3, fullWidth: true },
]

function toKeyDefs(specs: readonly ComicRuleSpec[]): RuleFieldDef[] {
  return specs.map((s) => ({ key: s.key, label: s.key }))
}

export const COMIC_SEARCH_KEY_DEFS = toKeyDefs(SEARCH_SPECS)
export const COMIC_EXPLORE_KEY_DEFS = toKeyDefs(EXPLORE_SPECS)
export const COMIC_DETAIL_KEY_DEFS = toKeyDefs(DETAIL_SPECS)
export const COMIC_TOC_KEY_DEFS = toKeyDefs(TOC_SPECS)
export const COMIC_PAGES_KEY_DEFS = toKeyDefs(PAGES_SPECS)

function localizeSpecs(
  specs: readonly ComicRuleSpec[],
  translate: (key: string) => string,
): RuleFieldDef[] {
  return specs.map((s) => ({
    key: s.key,
    label: translate(`sourceEditor.fields.${s.key}`),
    placeholder: s.placeholder ?? s.key,
    multiline: s.multiline,
    rows: s.rows,
    fullWidth: s.fullWidth,
  }))
}

export function buildComicRuleFields(translate: (key: string) => string) {
  return {
    search: localizeSpecs(SEARCH_SPECS, translate),
    explore: localizeSpecs(EXPLORE_SPECS, translate),
    detail: localizeSpecs(DETAIL_SPECS, translate),
    toc: localizeSpecs(TOC_SPECS, translate),
    pages: localizeSpecs(PAGES_SPECS, translate),
  }
}

export interface ComicSourceKeyMap {
  nameKey: string
  urlKey: string
  groupKey: string
  commentKey: string
  typeKey: string
  patternKey: string
  detailRuleKey: string
  tocRuleKey: string
  pagesRuleKey: string
}

export interface ComicSourceFormState {
  sourceName: string
  sourceUrl: string
  sourceGroup: string
  sourceComment: string
  sourceType: number
  readingDirection: string
  enabled: boolean
  enabledExplore: boolean
  enabledCookieJar: boolean
  weight: number | string
  concurrentRate: string
  header: string
  imageHeader: string
  comicUrlPattern: string
  loginUrl: string
  loginUi: string
  loginCheckJs: string
  jsLib: string
  searchUrl: string
  exploreUrl: string
  ruleSearch: Record<string, string>
  ruleExplore: Record<string, string>
  ruleDetail: Record<string, string>
  ruleToc: Record<string, string>
  rulePages: Record<string, string>
}

export interface ComicSourceSnapshot {
  sourceGroup: string
  sourceComment: string
  readingDirection: string
  enabled: boolean
  enabledExplore: boolean
  enabledCookieJar: boolean
  weight: number
  concurrentRate: string
  header: string
  imageHeader: string
  comicUrlPattern: string
  loginUrl: string
  loginUi: string
  loginCheckJs: string
  jsLib: string
  searchUrl: string
  exploreUrl: string
  ruleSearch: Record<string, string>
  ruleExplore: Record<string, string>
  ruleDetail: Record<string, string>
  ruleToc: Record<string, string>
  rulePages: Record<string, string>
}

export function createDefaultComicKeyMap(): ComicSourceKeyMap {
  return {
    nameKey: 'sourceName',
    urlKey: 'sourceUrl',
    groupKey: 'sourceGroup',
    commentKey: 'sourceComment',
    typeKey: 'sourceType',
    patternKey: 'comicUrlPattern',
    detailRuleKey: 'ruleDetail',
    tocRuleKey: 'ruleToc',
    pagesRuleKey: 'rulePages',
  }
}

export function createDefaultComicFormState(): ComicSourceFormState {
  return {
    sourceName: '',
    sourceUrl: '',
    sourceGroup: '',
    sourceComment: '',
    sourceType: 2,
    readingDirection: 'rtl',
    enabled: true,
    enabledExplore: true,
    enabledCookieJar: true,
    weight: 0,
    concurrentRate: '',
    header: '',
    imageHeader: '',
    comicUrlPattern: '',
    loginUrl: '',
    loginUi: '',
    loginCheckJs: '',
    jsLib: '',
    searchUrl: '',
    exploreUrl: '',
    ruleSearch: readRuleMap(undefined, COMIC_SEARCH_KEY_DEFS),
    ruleExplore: readRuleMap(undefined, COMIC_EXPLORE_KEY_DEFS),
    ruleDetail: readRuleMap(undefined, COMIC_DETAIL_KEY_DEFS),
    ruleToc: readRuleMap(undefined, COMIC_TOC_KEY_DEFS),
    rulePages: readRuleMap(undefined, COMIC_PAGES_KEY_DEFS),
  }
}

export function hydrateComicSourceForm(
  form: ComicSourceFormState,
  keyMap: ComicSourceKeyMap,
  obj: Record<string, unknown>,
  srcSummary?: ComicSourceSummaryDto,
): ComicSourceSnapshot {
  keyMap.nameKey = 'sourceName'
  keyMap.urlKey = 'sourceUrl'
  keyMap.groupKey = 'sourceGroup'
  keyMap.commentKey = 'sourceComment'
  keyMap.typeKey = 'sourceType'
  keyMap.patternKey = 'comicUrlPattern'
  keyMap.detailRuleKey = 'ruleDetail'
  keyMap.tocRuleKey = 'ruleToc'
  keyMap.pagesRuleKey = 'rulePages'

  form.sourceName = strVal(obj.sourceName) || srcSummary?.name || ''
  form.sourceUrl = strVal(obj.sourceUrl) || srcSummary?.source_url || ''
  const rawGroup = obj[keyMap.groupKey]
  form.sourceGroup = rawGroup !== undefined ? strVal(rawGroup) : (srcSummary?.group_name ?? '')
  form.sourceComment = strVal(obj.sourceComment)
  form.sourceType = numVal(obj[keyMap.typeKey], srcSummary?.source_type ?? 2)
  const dir = strVal(obj.readingDirection).trim().toLowerCase()
  form.readingDirection =
    dir === 'ltr' || dir === 'webtoon'
      ? dir
      : dir === 'vertical' || dir === 'scroll'
        ? 'webtoon'
        : 'rtl'
  form.enabled = srcSummary ? srcSummary.enabled : boolVal(obj.enabled, true)
  form.enabledExplore = boolVal(obj.enabledExplore, true)
  form.enabledCookieJar = boolVal(obj.enabledCookieJar, true)
  form.weight = srcSummary ? srcSummary.weight : numVal(obj.weight, 0)
  form.concurrentRate = strVal(obj.concurrentRate)
  form.header = strVal(obj.header)
  form.imageHeader = strVal(obj.imageHeader)
  form.comicUrlPattern = strVal(obj.comicUrlPattern)
  form.loginUrl = strVal(obj.loginUrl)
  form.loginUi = strVal(obj.loginUi)
  form.loginCheckJs = strVal(obj.loginCheckJs)
  form.jsLib = strVal(obj.jsLib)
  form.searchUrl = strVal(obj.searchUrl)
  form.exploreUrl = strVal(obj.exploreUrl)

  form.ruleSearch = readRuleMap(obj.ruleSearch, COMIC_SEARCH_KEY_DEFS)
  form.ruleExplore = readRuleMap(obj.ruleExplore, COMIC_EXPLORE_KEY_DEFS)
  form.ruleDetail = readRuleMap(obj[keyMap.detailRuleKey], COMIC_DETAIL_KEY_DEFS)
  form.ruleToc = readRuleMap(obj[keyMap.tocRuleKey], COMIC_TOC_KEY_DEFS)
  form.rulePages = readRuleMap(obj[keyMap.pagesRuleKey], COMIC_PAGES_KEY_DEFS)

  return {
    sourceGroup: form.sourceGroup,
    sourceComment: form.sourceComment,
    readingDirection: form.readingDirection,
    enabled: form.enabled,
    enabledExplore: form.enabledExplore,
    enabledCookieJar: form.enabledCookieJar,
    weight: numVal(form.weight, 0),
    concurrentRate: form.concurrentRate,
    header: form.header,
    imageHeader: form.imageHeader,
    comicUrlPattern: form.comicUrlPattern,
    loginUrl: form.loginUrl,
    loginUi: form.loginUi,
    loginCheckJs: form.loginCheckJs,
    jsLib: form.jsLib,
    searchUrl: form.searchUrl,
    exploreUrl: form.exploreUrl,
    ruleSearch: { ...form.ruleSearch },
    ruleExplore: { ...form.ruleExplore },
    ruleDetail: { ...form.ruleDetail },
    ruleToc: { ...form.ruleToc },
    rulePages: { ...form.rulePages },
  }
}

export function buildMergedComicSourceObject(
  base: Record<string, unknown>,
  form: ComicSourceFormState,
  keyMap: ComicSourceKeyMap,
  snap: ComicSourceSnapshot | null,
): Record<string, unknown> {
  const out: Record<string, unknown> = { ...base }
  out[keyMap.nameKey] = form.sourceName.trim()
  out[keyMap.urlKey] = form.sourceUrl.trim()
  mergeOptionalField(base, out, keyMap.groupKey, form.sourceGroup, snap?.sourceGroup, true)
  mergeOptionalField(base, out, keyMap.commentKey, form.sourceComment, snap?.sourceComment)

  if (!(keyMap.typeKey in base)) {
    out[keyMap.typeKey] = form.sourceType
  }
  const baseDir = strVal(base.readingDirection).trim().toLowerCase()
  const preserveReadingDirectionAlias =
    form.readingDirection === 'webtoon' &&
    snap?.readingDirection === 'webtoon' &&
    (baseDir === 'vertical' || baseDir === 'scroll')
  if (
    !preserveReadingDirectionAlias &&
    ('readingDirection' in base || !snap || form.readingDirection !== snap.readingDirection)
  ) {
    out.readingDirection = form.readingDirection
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
  const normalizedWeight = numVal(form.weight, 0)
  if ('weight' in base || !snap || normalizedWeight !== snap.weight) {
    out.weight = normalizedWeight
  }

  mergeOptionalField(base, out, 'concurrentRate', form.concurrentRate, snap?.concurrentRate, true)
  mergeOptionalField(base, out, 'header', form.header, snap?.header)
  mergeOptionalField(base, out, 'imageHeader', form.imageHeader, snap?.imageHeader)
  mergeOptionalField(base, out, keyMap.patternKey, form.comicUrlPattern, snap?.comicUrlPattern, true)
  mergeOptionalField(base, out, 'loginUrl', form.loginUrl, snap?.loginUrl)
  mergeOptionalField(base, out, 'loginUi', form.loginUi, snap?.loginUi)
  mergeOptionalField(base, out, 'loginCheckJs', form.loginCheckJs, snap?.loginCheckJs)
  mergeOptionalField(base, out, 'jsLib', form.jsLib, snap?.jsLib)
  mergeOptionalField(base, out, 'searchUrl', form.searchUrl, snap?.searchUrl)
  mergeOptionalField(base, out, 'exploreUrl', form.exploreUrl, snap?.exploreUrl)

  const sections: Array<
    [string, Record<string, string>, Record<string, string> | undefined, RuleFieldDef[]]
  > = [
    ['ruleSearch', form.ruleSearch, snap?.ruleSearch, COMIC_SEARCH_KEY_DEFS],
    ['ruleExplore', form.ruleExplore, snap?.ruleExplore, COMIC_EXPLORE_KEY_DEFS],
    [keyMap.detailRuleKey, form.ruleDetail, snap?.ruleDetail, COMIC_DETAIL_KEY_DEFS],
    [keyMap.tocRuleKey, form.ruleToc, snap?.ruleToc, COMIC_TOC_KEY_DEFS],
    [keyMap.pagesRuleKey, form.rulePages, snap?.rulePages, COMIC_PAGES_KEY_DEFS],
  ]
  for (const [key, map, initMap, fields] of sections) {
    const merged = mergeRuleSection(base[key], map, initMap, fields)
    if (merged) out[key] = merged
    else delete out[key]
  }
  return out
}
