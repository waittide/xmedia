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
import type { BookSourceSummaryDto } from './onlineApi'

export type BookSourceEditTab = SourceEditFourTab

interface BookRuleSpec {
  key: string
  placeholderKey?: string
  multiline?: boolean
  rows?: number
  fullWidth?: boolean
}

const SEARCH_SPECS: readonly BookRuleSpec[] = [
  { key: 'checkKeyWord', placeholderKey: 'checkKeyWordPlaceholder' },
  { key: 'bookList', placeholderKey: 'bookListPlaceholder', fullWidth: true },
  { key: 'name' },
  { key: 'author' },
  { key: 'bookUrl', fullWidth: true },
  { key: 'coverUrl', fullWidth: true },
  { key: 'kind' },
  { key: 'wordCount' },
  { key: 'lastChapter', fullWidth: true },
  { key: 'intro', multiline: true, rows: 2, fullWidth: true },
]

const EXPLORE_SPECS: readonly BookRuleSpec[] = SEARCH_SPECS.filter(
  (f) => f.key !== 'checkKeyWord',
)

const BOOK_INFO_SPECS: readonly BookRuleSpec[] = [
  { key: 'init', multiline: true, rows: 2, fullWidth: true },
  { key: 'name' },
  { key: 'author' },
  { key: 'tocUrl', fullWidth: true },
  { key: 'coverUrl', fullWidth: true },
  { key: 'kind' },
  { key: 'wordCount' },
  { key: 'lastChapter' },
  { key: 'canReName' },
  { key: 'intro', multiline: true, rows: 3, fullWidth: true },
]

const TOC_SPECS: readonly BookRuleSpec[] = [
  { key: 'chapterList', fullWidth: true },
  { key: 'chapterName' },
  { key: 'chapterUrl' },
  { key: 'nextTocUrl', fullWidth: true },
  { key: 'updateTime' },
  { key: 'isVolume' },
  { key: 'isVip' },
  { key: 'preUpdateJs', multiline: true, rows: 3, fullWidth: true },
]

const CONTENT_SPECS: readonly BookRuleSpec[] = [
  { key: 'content', multiline: true, rows: 4, fullWidth: true },
  { key: 'title' },
  { key: 'nextContentUrl' },
  { key: 'replaceRegex', multiline: true, rows: 2, fullWidth: true },
  { key: 'sourceRegex', fullWidth: true },
  { key: 'webJs', multiline: true, rows: 3, fullWidth: true },
]

function toKeyDefs(specs: readonly BookRuleSpec[]): RuleFieldDef[] {
  return specs.map((s) => ({ key: s.key, label: s.key }))
}

export const BOOK_SEARCH_KEY_DEFS = toKeyDefs(SEARCH_SPECS)
export const BOOK_EXPLORE_KEY_DEFS = toKeyDefs(EXPLORE_SPECS)
export const BOOK_INFO_KEY_DEFS = toKeyDefs(BOOK_INFO_SPECS)
export const BOOK_TOC_KEY_DEFS = toKeyDefs(TOC_SPECS)
export const BOOK_CONTENT_KEY_DEFS = toKeyDefs(CONTENT_SPECS)

function localizeSpecs(
  specs: readonly BookRuleSpec[],
  translate: (key: string) => string,
): RuleFieldDef[] {
  return specs.map((s) => ({
    key: s.key,
    label: translate(`sourceEditor.fields.${s.key}`),
    placeholder: s.placeholderKey
      ? translate(`sourceEditor.fields.${s.placeholderKey}`)
      : s.key,
    multiline: s.multiline,
    rows: s.rows,
    fullWidth: s.fullWidth,
  }))
}

export function buildBookRuleFields(translate: (key: string) => string) {
  return {
    search: localizeSpecs(SEARCH_SPECS, translate),
    explore: localizeSpecs(EXPLORE_SPECS, translate),
    bookInfo: localizeSpecs(BOOK_INFO_SPECS, translate),
    toc: localizeSpecs(TOC_SPECS, translate),
    content: localizeSpecs(CONTENT_SPECS, translate),
  }
}

export interface BookSourceFormState {
  bookSourceName: string
  bookSourceUrl: string
  bookSourceGroup: string
  bookSourceComment: string
  bookSourceType: string
  enabled: boolean
  enabledExplore: boolean
  enabledCookieJar: boolean
  weight: number | string
  concurrentRate: string
  header: string
  bookUrlPattern: string
  loginUrl: string
  loginUi: string
  loginCheckJs: string
  jsLib: string
  searchUrl: string
  exploreUrl: string
  ruleSearch: Record<string, string>
  ruleExplore: Record<string, string>
  ruleBookInfo: Record<string, string>
  ruleToc: Record<string, string>
  ruleContent: Record<string, string>
}

export interface BookSourceSnapshot {
  bookSourceGroup: string
  bookSourceComment: string
  bookSourceType: string
  enabled: boolean
  enabledExplore: boolean
  enabledCookieJar: boolean
  weight: number
  concurrentRate: string
  header: string
  bookUrlPattern: string
  loginUrl: string
  loginUi: string
  loginCheckJs: string
  jsLib: string
  searchUrl: string
  exploreUrl: string
  ruleSearch: Record<string, string>
  ruleExplore: Record<string, string>
  ruleBookInfo: Record<string, string>
  ruleToc: Record<string, string>
  ruleContent: Record<string, string>
}

export function createDefaultBookFormState(): BookSourceFormState {
  return {
    bookSourceName: '',
    bookSourceUrl: '',
    bookSourceGroup: '',
    bookSourceComment: '',
    bookSourceType: '0',
    enabled: true,
    enabledExplore: true,
    enabledCookieJar: true,
    weight: 0,
    concurrentRate: '',
    header: '',
    bookUrlPattern: '',
    loginUrl: '',
    loginUi: '',
    loginCheckJs: '',
    jsLib: '',
    searchUrl: '',
    exploreUrl: '',
    ruleSearch: readRuleMap(undefined, BOOK_SEARCH_KEY_DEFS),
    ruleExplore: readRuleMap(undefined, BOOK_EXPLORE_KEY_DEFS),
    ruleBookInfo: readRuleMap(undefined, BOOK_INFO_KEY_DEFS),
    ruleToc: readRuleMap(undefined, BOOK_TOC_KEY_DEFS),
    ruleContent: readRuleMap(undefined, BOOK_CONTENT_KEY_DEFS),
  }
}

export function hydrateBookSourceForm(
  form: BookSourceFormState,
  obj: Record<string, unknown>,
  srcSummary?: BookSourceSummaryDto,
): BookSourceSnapshot {
  form.bookSourceName = strVal(obj.bookSourceName) || srcSummary?.name || ''
  form.bookSourceUrl = strVal(obj.bookSourceUrl) || srcSummary?.source_url || ''
  form.bookSourceGroup =
    obj.bookSourceGroup !== undefined
      ? strVal(obj.bookSourceGroup)
      : (srcSummary?.group_name ?? '')
  form.bookSourceComment = strVal(obj.bookSourceComment)
  form.bookSourceType = String(numVal(obj.bookSourceType, srcSummary?.source_type ?? 0))
  form.enabled = srcSummary ? srcSummary.enabled : boolVal(obj.enabled, true)
  form.enabledExplore = boolVal(obj.enabledExplore, true)
  form.enabledCookieJar = boolVal(obj.enabledCookieJar, true)
  form.weight = srcSummary ? srcSummary.weight : numVal(obj.weight, 0)
  form.concurrentRate = strVal(obj.concurrentRate)
  form.header = strVal(obj.header)
  form.bookUrlPattern = strVal(obj.bookUrlPattern)
  form.loginUrl = strVal(obj.loginUrl)
  form.loginUi = strVal(obj.loginUi)
  form.loginCheckJs = strVal(obj.loginCheckJs)
  form.jsLib = strVal(obj.jsLib)
  form.searchUrl = strVal(obj.searchUrl)
  form.exploreUrl = strVal(obj.exploreUrl)
  form.ruleSearch = readRuleMap(obj.ruleSearch, BOOK_SEARCH_KEY_DEFS)
  form.ruleExplore = readRuleMap(obj.ruleExplore, BOOK_EXPLORE_KEY_DEFS)
  form.ruleBookInfo = readRuleMap(obj.ruleBookInfo, BOOK_INFO_KEY_DEFS)
  form.ruleToc = readRuleMap(obj.ruleToc, BOOK_TOC_KEY_DEFS)
  form.ruleContent = readRuleMap(obj.ruleContent, BOOK_CONTENT_KEY_DEFS)

  return {
    bookSourceGroup: form.bookSourceGroup,
    bookSourceComment: form.bookSourceComment,
    bookSourceType: form.bookSourceType,
    enabled: form.enabled,
    enabledExplore: form.enabledExplore,
    enabledCookieJar: form.enabledCookieJar,
    weight: numVal(form.weight, 0),
    concurrentRate: form.concurrentRate,
    header: form.header,
    bookUrlPattern: form.bookUrlPattern,
    loginUrl: form.loginUrl,
    loginUi: form.loginUi,
    loginCheckJs: form.loginCheckJs,
    jsLib: form.jsLib,
    searchUrl: form.searchUrl,
    exploreUrl: form.exploreUrl,
    ruleSearch: { ...form.ruleSearch },
    ruleExplore: { ...form.ruleExplore },
    ruleBookInfo: { ...form.ruleBookInfo },
    ruleToc: { ...form.ruleToc },
    ruleContent: { ...form.ruleContent },
  }
}

export function buildMergedBookSourceObject(
  base: Record<string, unknown>,
  form: BookSourceFormState,
  snap: BookSourceSnapshot | null,
): Record<string, unknown> {
  const out: Record<string, unknown> = { ...base }
  out.bookSourceName = form.bookSourceName.trim()
  out.bookSourceUrl = form.bookSourceUrl.trim()
  mergeOptionalField(base, out, 'bookSourceGroup', form.bookSourceGroup, snap?.bookSourceGroup, true)
  mergeOptionalField(base, out, 'bookSourceComment', form.bookSourceComment, snap?.bookSourceComment)

  if ('bookSourceType' in base || !snap || form.bookSourceType !== snap.bookSourceType) {
    out.bookSourceType = numVal(form.bookSourceType, 0)
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
  mergeOptionalField(base, out, 'bookUrlPattern', form.bookUrlPattern, snap?.bookUrlPattern, true)
  mergeOptionalField(base, out, 'loginUrl', form.loginUrl, snap?.loginUrl)
  mergeOptionalField(base, out, 'loginUi', form.loginUi, snap?.loginUi)
  mergeOptionalField(base, out, 'loginCheckJs', form.loginCheckJs, snap?.loginCheckJs)
  mergeOptionalField(base, out, 'jsLib', form.jsLib, snap?.jsLib)
  mergeOptionalField(base, out, 'searchUrl', form.searchUrl, snap?.searchUrl)
  mergeOptionalField(base, out, 'exploreUrl', form.exploreUrl, snap?.exploreUrl)

  const sections: Array<
    [string, Record<string, string>, Record<string, string> | undefined, RuleFieldDef[]]
  > = [
    ['ruleSearch', form.ruleSearch, snap?.ruleSearch, BOOK_SEARCH_KEY_DEFS],
    ['ruleExplore', form.ruleExplore, snap?.ruleExplore, BOOK_EXPLORE_KEY_DEFS],
    ['ruleBookInfo', form.ruleBookInfo, snap?.ruleBookInfo, BOOK_INFO_KEY_DEFS],
    ['ruleToc', form.ruleToc, snap?.ruleToc, BOOK_TOC_KEY_DEFS],
    ['ruleContent', form.ruleContent, snap?.ruleContent, BOOK_CONTENT_KEY_DEFS],
  ]
  for (const [key, map, initMap, fields] of sections) {
    const merged = mergeRuleSection(base[key], map, initMap, fields)
    if (merged) out[key] = merged
    else delete out[key]
  }
  return out
}
