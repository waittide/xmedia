<script setup lang="ts">
import { computed, reactive, toRef } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuCode2,
  LuCompass,
  LuImage,
  LuInfo,
  LuList,
  LuSearch,
} from 'vue-icons-plus/lu'
import OnlineSourceEditShell from '@/components/online-sources/OnlineSourceEditShell.vue'
import RuleFieldsSection, { type RuleFieldDef } from '@/components/online-sources/RuleFieldsSection.vue'
import {
  boolVal,
  hasAnyRule,
  mergeOptionalField,
  mergeRuleSection,
  numVal,
  pickFirstString,
  pickPresentKey,
  readRuleMap,
  strVal,
  useOnlineSourceEdit,
  type EditTabItem,
} from '@/components/online-sources/useOnlineSourceEdit'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import { comicSourceAdapter, type ComicSourceSummaryDto } from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: ComicSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: [source: ComicSourceSummaryDto]
  savedAndDebug: [source: ComicSourceSummaryDto]
}>()

const { t } = useI18n()

type EditTab = 'basic' | 'search' | 'explore' | 'detail' | 'toc' | 'pages' | 'json'

const readingDirectionOptions = computed(() => [
  { value: 'rtl', label: t('onlineSources.edit.directionRtl') },
  { value: 'ltr', label: t('onlineSources.edit.directionLtr') },
  { value: 'webtoon', label: t('onlineSources.edit.directionWebtoon') },
])

const searchAliases: Record<string, readonly string[]> = {
  bookList: ['comicList', 'list'],
  name: ['title'],
  bookUrl: ['comicUrl', 'url'],
  coverUrl: ['cover'],
}

const detailAliases: Record<string, readonly string[]> = {
  name: ['title'],
  coverUrl: ['cover'],
  tocUrl: ['chaptersUrl'],
}

const tocAliases: Record<string, readonly string[]> = {
  chapterName: ['name', 'title'],
  chapterUrl: ['url'],
  nextTocUrl: ['nextChaptersUrl'],
}

const pagesAliases: Record<string, readonly string[]> = {
  content: ['pages', 'images'],
  nextContentUrl: ['nextPageUrl'],
}

const searchRuleFields: RuleFieldDef[] = [
  { key: 'checkKeyWord', label: 'checkKeyWord' },
  { key: 'bookList', label: 'bookList / comicList', fullWidth: true },
  { key: 'name', label: 'name / title' },
  { key: 'author', label: 'author' },
  { key: 'bookUrl', label: 'bookUrl / comicUrl', fullWidth: true },
  { key: 'coverUrl', label: 'coverUrl', fullWidth: true },
  { key: 'kind', label: 'kind' },
  { key: 'wordCount', label: 'wordCount' },
  { key: 'lastChapter', label: 'lastChapter', fullWidth: true },
  { key: 'intro', label: 'intro', multiline: true, rows: 2, fullWidth: true },
]

const exploreRuleFields: RuleFieldDef[] = searchRuleFields.filter((f) => f.key !== 'checkKeyWord')

const detailRuleFields: RuleFieldDef[] = [
  { key: 'init', label: 'init', multiline: true, rows: 2, fullWidth: true },
  { key: 'name', label: 'name / title' },
  { key: 'author', label: 'author' },
  { key: 'tocUrl', label: 'tocUrl / chaptersUrl', fullWidth: true },
  { key: 'coverUrl', label: 'coverUrl', fullWidth: true },
  { key: 'kind', label: 'kind' },
  { key: 'status', label: 'status' },
  { key: 'readingDirection', label: 'readingDirection' },
  { key: 'wordCount', label: 'wordCount' },
  { key: 'lastChapter', label: 'lastChapter' },
  { key: 'canReName', label: 'canReName' },
  { key: 'intro', label: 'intro', multiline: true, rows: 3, fullWidth: true },
]

const tocRuleFields: RuleFieldDef[] = [
  { key: 'chapterList', label: 'chapterList', fullWidth: true },
  { key: 'chapterName', label: 'chapterName' },
  { key: 'chapterUrl', label: 'chapterUrl' },
  { key: 'nextTocUrl', label: 'nextTocUrl', fullWidth: true },
  { key: 'updateTime', label: 'updateTime' },
  { key: 'isVolume', label: 'isVolume' },
  { key: 'isVip', label: 'isVip' },
  { key: 'preUpdateJs', label: 'preUpdateJs', multiline: true, rows: 3, fullWidth: true },
]

const pagesRuleFields: RuleFieldDef[] = [
  { key: 'content', label: 'content / pages', multiline: true, rows: 4, fullWidth: true },
  { key: 'title', label: 'title' },
  { key: 'imageUrl', label: 'imageUrl' },
  { key: 'nextContentUrl', label: 'nextContentUrl' },
  { key: 'imageHeader', label: 'imageHeader', multiline: true, rows: 2, fullWidth: true },
  { key: 'replaceRegex', label: 'replaceRegex', multiline: true, rows: 2, fullWidth: true },
  { key: 'sourceRegex', label: 'sourceRegex', fullWidth: true },
  { key: 'webJs', label: 'webJs', multiline: true, rows: 3, fullWidth: true },
]

interface KeyMap {
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

const keyMap = reactive<KeyMap>({
  nameKey: 'sourceName',
  urlKey: 'sourceUrl',
  groupKey: 'sourceGroup',
  commentKey: 'sourceComment',
  typeKey: 'sourceType',
  patternKey: 'comicUrlPattern',
  detailRuleKey: 'ruleDetail',
  tocRuleKey: 'ruleToc',
  pagesRuleKey: 'rulePages',
})

interface ComicSnapshot {
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

const form = reactive({
  sourceName: '',
  sourceUrl: '',
  sourceGroup: '',
  sourceComment: '',
  sourceType: 2,
  readingDirection: 'rtl',
  enabled: true,
  enabledExplore: true,
  enabledCookieJar: true,
  weight: 0 as number | string,
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
  ruleSearch: {} as Record<string, string>,
  ruleExplore: {} as Record<string, string>,
  ruleDetail: {} as Record<string, string>,
  ruleToc: {} as Record<string, string>,
  rulePages: {} as Record<string, string>,
})

let initialSnapshot: ComicSnapshot | null = null

function clearFormState(): void {
  initialSnapshot = null
  keyMap.nameKey = 'sourceName'
  keyMap.urlKey = 'sourceUrl'
  keyMap.groupKey = 'sourceGroup'
  keyMap.commentKey = 'sourceComment'
  keyMap.typeKey = 'sourceType'
  keyMap.patternKey = 'comicUrlPattern'
  keyMap.detailRuleKey = 'ruleDetail'
  keyMap.tocRuleKey = 'ruleToc'
  keyMap.pagesRuleKey = 'rulePages'

  form.sourceName = ''
  form.sourceUrl = ''
  form.sourceGroup = ''
  form.sourceComment = ''
  form.sourceType = 2
  form.readingDirection = 'rtl'
  form.enabled = true
  form.enabledExplore = true
  form.enabledCookieJar = true
  form.weight = 0
  form.concurrentRate = ''
  form.header = ''
  form.imageHeader = ''
  form.comicUrlPattern = ''
  form.loginUrl = ''
  form.loginUi = ''
  form.loginCheckJs = ''
  form.jsLib = ''
  form.searchUrl = ''
  form.exploreUrl = ''
  form.ruleSearch = readRuleMap(undefined, searchRuleFields, searchAliases)
  form.ruleExplore = readRuleMap(undefined, exploreRuleFields, searchAliases)
  form.ruleDetail = readRuleMap(undefined, detailRuleFields, detailAliases)
  form.ruleToc = readRuleMap(undefined, tocRuleFields, tocAliases)
  form.rulePages = readRuleMap(undefined, pagesRuleFields, pagesAliases)
}

function captureSnapshot(): ComicSnapshot {
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

function hydrateFormFromObject(
  obj: Record<string, unknown>,
  srcSummary?: ComicSourceSummaryDto,
): void {
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
  form.sourceGroup =
    rawGroup !== undefined ? strVal(rawGroup) : (srcSummary?.group_name ?? '')
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

  form.ruleSearch = readRuleMap(obj.ruleSearch, searchRuleFields, searchAliases)
  form.ruleExplore = readRuleMap(obj.ruleExplore, exploreRuleFields, searchAliases)
  form.ruleDetail = readRuleMap(obj[keyMap.detailRuleKey], detailRuleFields, detailAliases)
  form.ruleToc = readRuleMap(obj[keyMap.tocRuleKey], tocRuleFields, tocAliases)
  form.rulePages = readRuleMap(obj[keyMap.pagesRuleKey], pagesRuleFields, pagesAliases)
  initialSnapshot = captureSnapshot()
}

const {
  activeTab,
  loading,
  saving,
  loadError,
  rawBaseObj,
  jsonText,
  jsonDirty,
  variableText,
  jsonValidation,
  switchTab,
  formatJson,
  copyJson,
  saveSource,
} = useOnlineSourceEdit<ComicSourceSummaryDto, EditTab>({
  open: toRef(props, 'open'),
  source: toRef(props, 'source'),
  adapter: comicSourceAdapter,
  initialTab: 'basic',
  jsonTabId: 'json',
  clearFormState,
  hydrateFormFromObject,
  buildMergedObject: () => {
    const base = rawBaseObj.value
    const snap = initialSnapshot
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
    mergeOptionalField(
      base,
      out,
      keyMap.patternKey,
      form.comicUrlPattern,
      snap?.comicUrlPattern,
      true,
    )
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
      ['ruleSearch', form.ruleSearch, snap?.ruleSearch, searchRuleFields, searchAliases],
      ['ruleExplore', form.ruleExplore, snap?.ruleExplore, exploreRuleFields, searchAliases],
      [keyMap.detailRuleKey, form.ruleDetail, snap?.ruleDetail, detailRuleFields, detailAliases],
      [keyMap.tocRuleKey, form.ruleToc, snap?.ruleToc, tocRuleFields, tocAliases],
      [keyMap.pagesRuleKey, form.rulePages, snap?.rulePages, pagesRuleFields, pagesAliases],
    ]
    for (const [key, map, initMap, fields, aliases] of sections) {
      const merged = mergeRuleSection(base[key], map, initMap, fields, aliases)
      if (merged) out[key] = merged
      else delete out[key]
    }
    return out
  },
  validateForm: () => {
    if (!form.sourceName.trim() && !form.sourceUrl.trim()) {
      return t('onlineSources.edit.nameAndUrlRequired')
    }
    return null
  },
  validateJsonObject: (obj) => {
    const name = pickFirstString(obj, ['sourceName', 'bookSourceName']).trim()
    const url = pickFirstString(obj, ['sourceUrl', 'bookSourceUrl']).trim()
    if (!name && !url) return t('onlineSources.edit.nameAndUrlRequired')
    return null
  },
  getGroupName: () => form.sourceGroup,
  getEnabled: () => form.enabled,
  getWeight: () => numVal(form.weight, 0),
  onSaved: (updated) => emit('saved', updated),
  onSavedAndDebug: (updated) => emit('savedAndDebug', updated),
  onClose: () => emit('close'),
})

const domainLabel = computed(() => t('onlineSources.domainLabel.comic'))

const tabItems = computed<EditTabItem<EditTab>[]>(() => [
  { id: 'basic', label: t('onlineSources.edit.tabBasic'), icon: LuInfo, activeDot: false },
  {
    id: 'search',
    label: t('onlineSources.edit.tabSearch'),
    icon: LuSearch,
    activeDot: Boolean(form.searchUrl.trim() || hasAnyRule(form.ruleSearch)),
  },
  {
    id: 'explore',
    label: t('onlineSources.edit.tabExplore'),
    icon: LuCompass,
    activeDot: Boolean(form.exploreUrl.trim() || hasAnyRule(form.ruleExplore)),
  },
  {
    id: 'detail',
    label: t('onlineSources.edit.tabDetail'),
    icon: LuBookOpen,
    activeDot: hasAnyRule(form.ruleDetail),
  },
  {
    id: 'toc',
    label: t('onlineSources.edit.tabToc'),
    icon: LuList,
    activeDot: hasAnyRule(form.ruleToc),
  },
  {
    id: 'pages',
    label: t('onlineSources.edit.tabPages'),
    icon: LuImage,
    activeDot: hasAnyRule(form.rulePages),
  },
  { id: 'json', label: t('onlineSources.edit.tabJson'), icon: LuCode2, activeDot: false },
])
</script>

<template>
  <OnlineSourceEditShell
    :open="open"
    :title="t('onlineSources.edit.title', { domain: domainLabel, name: form.sourceName || source?.name || '' })"
    :subtitle="form.sourceUrl || source?.source_url"
    :domain-label="domainLabel"
    :tabs="tabItems"
    :active-tab="activeTab"
    json-tab-id="json"
    :loading="loading"
    :saving="saving"
    :load-error="loadError"
    :json-text="jsonText"
    :json-validation="jsonValidation"
    @close="emit('close')"
    @switch-tab="switchTab"
    @update:json-text="jsonText = $event"
    @mark-json-dirty="jsonDirty = true"
    @format-json="formatJson"
    @copy-json="copyJson"
    @save="saveSource"
  >
    <template v-if="activeTab === 'basic'">
      <section class="flex flex-col gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
        <h4 class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('onlineSources.edit.basicSectionTitle') }}
        </h4>
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.nameLabel') }}
              <span class="font-mono opacity-65">{{ keyMap.nameKey }}</span>
            </span>
            <input
              v-model="form.sourceName"
              type="text"
              class="md-input w-full py-2 text-sm"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.groupLabel') }}
              <span class="font-mono opacity-65">{{ keyMap.groupKey }}</span>
            </span>
            <input
              v-model="form.sourceGroup"
              name="group_name"
              type="text"
              :aria-label="t('onlineSources.edit.groupLabel')"
              :placeholder="t('onlineSources.edit.groupPlaceholder')"
              class="md-input w-full py-2 text-sm"
            />
          </label>

          <label class="block space-y-1 sm:col-span-2">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.urlLabel') }}
              <span class="font-mono opacity-65">{{ keyMap.urlKey }}</span>
            </span>
            <input
              v-model="form.sourceUrl"
              type="text"
              :placeholder="t('onlineSources.edit.urlPlaceholder')"
              class="md-input w-full py-2 font-mono text-xs"
            />
          </label>

          <div class="space-y-1">
            <span class="block text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.readingDirectionLabel') }}
              <span class="font-mono opacity-65">readingDirection</span>
            </span>
            <AppSelect
              v-model="form.readingDirection"
              :options="readingDirectionOptions"
            />
          </div>

          <div class="grid grid-cols-2 gap-2.5">
            <label class="block space-y-1">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                {{ t('onlineSources.edit.concurrentRateLabel') }}
              </span>
              <input
                v-model="form.concurrentRate"
                type="text"
                :placeholder="t('onlineSources.edit.concurrentRatePlaceholder')"
                class="md-input w-full py-2 font-mono text-xs"
              />
            </label>

            <label class="block space-y-1">
              <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                {{ t('onlineSources.edit.weightLabel') }}
              </span>
              <input
                v-model.number="form.weight"
                type="number"
                class="md-input w-full py-2 font-mono text-xs"
              />
            </label>
          </div>

          <label class="block space-y-1 sm:col-span-2">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.commentLabel') }}
              <span class="font-mono opacity-65">{{ keyMap.commentKey }}</span>
            </span>
            <textarea
              v-model="form.sourceComment"
              rows="2"
              :placeholder="t('onlineSources.edit.commentPlaceholder')"
              class="md-input w-full py-2 text-xs leading-5"
            />
          </label>
        </div>

        <div class="flex flex-wrap items-center gap-x-6 gap-y-2.5 border-t border-[var(--md-sys-color-outline-variant)]/40 pt-3">
          <label class="inline-flex items-center gap-2 text-xs text-[var(--md-sys-color-on-surface)]">
            <AppSwitch v-model="form.enabled" />
            <span>{{ t('onlineSources.edit.enabledSwitch') }}</span>
          </label>
          <label class="inline-flex items-center gap-2 text-xs text-[var(--md-sys-color-on-surface)]">
            <AppSwitch v-model="form.enabledExplore" />
            <span>{{ t('onlineSources.edit.enabledExploreSwitch') }}</span>
          </label>
          <label class="inline-flex items-center gap-2 text-xs text-[var(--md-sys-color-on-surface)]">
            <AppSwitch v-model="form.enabledCookieJar" />
            <span>{{ t('onlineSources.edit.enabledCookieJarSwitch') }}</span>
          </label>
        </div>
      </section>

      <section class="flex flex-col gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
        <h4 class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('onlineSources.edit.networkSectionTitle') }}
        </h4>
        <div class="grid grid-cols-1 gap-3">
          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.headerLabel') }} <span class="font-mono opacity-65">header</span>
            </span>
            <textarea
              v-model="form.header"
              rows="2"
              spellcheck="false"
              :placeholder="t('onlineSources.edit.headerPlaceholder')"
              class="md-input w-full py-2 font-mono text-xs leading-5"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.imageHeaderLabel') }} <span class="font-mono opacity-65">imageHeader</span>
            </span>
            <textarea
              v-model="form.imageHeader"
              rows="2"
              spellcheck="false"
              :placeholder="t('onlineSources.edit.headerPlaceholder')"
              class="md-input w-full py-2 font-mono text-xs leading-5"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.comicUrlPatternLabel') }}
              <span class="font-mono opacity-65">{{ keyMap.patternKey }}</span>
            </span>
            <input
              v-model="form.comicUrlPattern"
              type="text"
              spellcheck="false"
              class="md-input w-full py-2 font-mono text-xs"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.loginUrlLabel') }} <span class="font-mono opacity-65">loginUrl</span>
            </span>
            <textarea
              v-model="form.loginUrl"
              rows="2"
              spellcheck="false"
              class="md-input w-full py-2 font-mono text-xs leading-5"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.loginUiLabel') }} <span class="font-mono opacity-65">loginUi</span>
            </span>
            <textarea
              v-model="form.loginUi"
              rows="3"
              spellcheck="false"
              class="md-input w-full py-2 font-mono text-xs leading-5"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.loginCheckJsLabel') }} <span class="font-mono opacity-65">loginCheckJs</span>
            </span>
            <textarea
              v-model="form.loginCheckJs"
              rows="2"
              spellcheck="false"
              class="md-input w-full py-2 font-mono text-xs leading-5"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.jsLibLabel') }} <span class="font-mono opacity-65">jsLib</span>
            </span>
            <textarea
              v-model="form.jsLib"
              rows="3"
              spellcheck="false"
              class="md-input w-full py-2 font-mono text-xs leading-5"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.variableLabel') }} <span class="font-mono opacity-65">source.variable</span>
            </span>
            <input
              v-model="variableText"
              type="text"
              spellcheck="false"
              :placeholder="t('onlineSources.edit.variablePlaceholder')"
              class="md-input w-full py-2 font-mono text-xs"
            />
          </label>
        </div>
      </section>
    </template>

    <template v-else-if="activeTab === 'search'">
      <section class="flex flex-col gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
        <h4 class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('onlineSources.edit.searchUrlLabel') }}
        </h4>
        <textarea
          v-model="form.searchUrl"
          rows="3"
          spellcheck="false"
          class="md-input w-full py-2 font-mono text-xs leading-5"
        />
      </section>

      <RuleFieldsSection
        v-model="form.ruleSearch"
        title="ruleSearch"
        :fields="searchRuleFields"
      />
    </template>

    <template v-else-if="activeTab === 'explore'">
      <section class="flex flex-col gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
        <h4 class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('onlineSources.edit.exploreUrlLabel') }}
        </h4>
        <textarea
          v-model="form.exploreUrl"
          rows="4"
          spellcheck="false"
          class="md-input w-full py-2 font-mono text-xs leading-5"
        />
      </section>

      <RuleFieldsSection
        v-model="form.ruleExplore"
        title="ruleExplore"
        :fields="exploreRuleFields"
      />
    </template>

    <template v-else-if="activeTab === 'detail'">
      <RuleFieldsSection
        v-model="form.ruleDetail"
        :title="keyMap.detailRuleKey"
        :fields="detailRuleFields"
      />
    </template>

    <template v-else-if="activeTab === 'toc'">
      <RuleFieldsSection
        v-model="form.ruleToc"
        :title="keyMap.tocRuleKey"
        :fields="tocRuleFields"
      />
    </template>

    <template v-else-if="activeTab === 'pages'">
      <RuleFieldsSection
        v-model="form.rulePages"
        :title="keyMap.pagesRuleKey"
        :fields="pagesRuleFields"
      />
    </template>
  </OnlineSourceEditShell>
</template>
