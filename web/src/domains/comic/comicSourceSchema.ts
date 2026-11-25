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
import type { ComicSourceSummaryDto } from './onlineApi'

export type ComicSourceEditTab = SourceEditFourTab

export const COMIC_SEARCH_ALIASES: Record<string, readonly string[]> = {
  bookList: ['comicList', 'list'],
  name: ['title'],
  bookUrl: ['comicUrl', 'url'],
  coverUrl: ['cover'],
}

export const COMIC_DETAIL_ALIASES: Record<string, readonly string[]> = {
  name: ['title'],
  coverUrl: ['cover'],
  tocUrl: ['chaptersUrl'],
}

export const COMIC_TOC_ALIASES: Record<string, readonly string[]> = {
  chapterName: ['name', 'title'],
  chapterUrl: ['url'],
  nextTocUrl: ['nextChaptersUrl'],
}

export const COMIC_PAGES_ALIASES: Record<string, readonly string[]> = {
  content: ['pages', 'images'],
  nextContentUrl: ['nextPageUrl'],
}

interface ComicRuleSpec {
  key: string
  placeholder?: string
  multiline?: boolean
  rows?: number
  fullWidth?: boolean
}

const SEARCH_SPECS: readonly ComicRuleSpec[] = [
  { key: 'checkKeyWord' },
  { key: 'bookList', placeholder: 'bookList / comicList', fullWidth: true },
  { key: 'name', placeholder: 'name / title' },
  { key: 'author' },
  { key: 'bookUrl', placeholder: 'bookUrl / comicUrl', fullWidth: true },
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
  { key: 'name', placeholder: 'name / title' },
  { key: 'author' },
  { key: 'tocUrl', placeholder: 'tocUrl / chaptersUrl', fullWidth: true },
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
  { key: 'content', placeholder: 'content / pages', multiline: true, rows: 4, fullWidth: true },
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
    ruleSearch: readRuleMap(undefined, COMIC_SEARCH_KEY_DEFS, COMIC_SEARCH_ALIASES),
    ruleExplore: readRuleMap(undefined, COMIC_EXPLORE_KEY_DEFS, COMIC_SEARCH_ALIASES),
    ruleDetail: readRuleMap(undefined, COMIC_DETAIL_KEY_DEFS, COMIC_DETAIL_ALIASES),
    ruleToc: readRuleMap(undefined, COMIC_TOC_KEY_DEFS, COMIC_TOC_ALIASES),
    rulePages: readRuleMap(undefined, COMIC_PAGES_KEY_DEFS, COMIC_PAGES_ALIASES),
  }
}

export function hydrateComicSourceForm(
  form: ComicSourceFormState,
  keyMap: ComicSourceKeyMap,
  obj: Record<string, unknown>,
  srcSummary?: ComicSourceSummaryDto,
): ComicSourceSnapshot {
  keyMap.nameKey = pickPresentKey(obj, ['sourceName', 'bookSourceName'])
  keyMap.urlKey = pickPresentKey(obj, ['sourceUrl', 'bookSourceUrl'])
  keyMap.groupKey = pickPresentKey(obj, ['sourceGroup', 'bookSourceGroup'])
  keyMap.commentKey = pickPresentKey(obj, ['sourceComment', 'bookSourceComment'])
  keyMap.typeKey = pickPresentKey(obj, ['sourceType', 'bookSourceType'])
  keyMap.patternKey = pickPresentKey(obj, ['comicUrlPattern', 'bookUrlPattern'])
  keyMap.detailRuleKey = pickPresentKey(obj, ['ruleDetail', 'ruleBookInfo'])
  keyMap.tocRuleKey = pickPresentKey(obj, ['ruleToc', 'ruleChapters'])
  keyMap.pagesRuleKey = pickPresentKey(obj, ['rulePages', 'ruleContent'])

  form.sourceName = pickFirstString(obj, ['sourceName', 'bookSourceName']) || srcSummary?.name || ''
  form.sourceUrl =
    pickFirstString(obj, ['sourceUrl', 'bookSourceUrl']) || srcSummary?.source_url || ''
  const rawGroup = obj[keyMap.groupKey]
  form.sourceGroup = rawGroup !== undefined ? strVal(rawGroup) : (srcSummary?.group_name ?? '')
  form.sourceComment = pickFirstString(obj, ['sourceComment', 'bookSourceComment'])
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
  form.comicUrlPattern = pickFirstString(obj, ['comicUrlPattern', 'bookUrlPattern'])
  form.loginUrl = strVal(obj.loginUrl)
  form.loginUi = strVal(obj.loginUi)
  form.loginCheckJs = strVal(obj.loginCheckJs)
  form.jsLib = strVal(obj.jsLib)
  form.searchUrl = strVal(obj.searchUrl)
  form.exploreUrl = strVal(obj.exploreUrl)

  form.ruleSearch = readRuleMap(obj.ruleSearch, COMIC_SEARCH_KEY_DEFS, COMIC_SEARCH_ALIASES)
  form.ruleExplore = readRuleMap(obj.ruleExplore, COMIC_EXPLORE_KEY_DEFS, COMIC_SEARCH_ALIASES)
  form.ruleDetail = readRuleMap(
    obj[keyMap.detailRuleKey],
    COMIC_DETAIL_KEY_DEFS,
    COMIC_DETAIL_ALIASES,
  )
  form.ruleToc = readRuleMap(obj[keyMap.tocRuleKey], COMIC_TOC_KEY_DEFS, COMIC_TOC_ALIASES)
  form.rulePages = readRuleMap(obj[keyMap.pagesRuleKey], COMIC_PAGES_KEY_DEFS, COMIC_PAGES_ALIASES)

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
    [
      string,
      Record<string, string>,
      Record<string, string> | undefined,
      RuleFieldDef[],
      Record<string, readonly string[]>,
    ]
  > = [
    ['ruleSearch', form.ruleSearch, snap?.ruleSearch, COMIC_SEARCH_KEY_DEFS, COMIC_SEARCH_ALIASES],
    [
      'ruleExplore',
      form.ruleExplore,
      snap?.ruleExplore,
      COMIC_EXPLORE_KEY_DEFS,
      COMIC_SEARCH_ALIASES,
    ],
    [
      keyMap.detailRuleKey,
      form.ruleDetail,
      snap?.ruleDetail,
      COMIC_DETAIL_KEY_DEFS,
      COMIC_DETAIL_ALIASES,
    ],
    [keyMap.tocRuleKey, form.ruleToc, snap?.ruleToc, COMIC_TOC_KEY_DEFS, COMIC_TOC_ALIASES],
    [
      keyMap.pagesRuleKey,
      form.rulePages,
      snap?.rulePages,
      COMIC_PAGES_KEY_DEFS,
      COMIC_PAGES_ALIASES,
    ],
  ]
  for (const [key, map, initMap, fields, aliases] of sections) {
    const merged = mergeRuleSection(base[key], map, initMap, fields, aliases)
    if (merged) out[key] = merged
    else delete out[key]
  }
  return out
}
