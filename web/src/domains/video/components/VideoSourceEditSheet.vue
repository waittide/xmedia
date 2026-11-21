<script setup lang="ts">
import { computed, reactive, toRef } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuCode2,
  LuCompass,
  LuFilm,
  LuFolderTree,
  LuInfo,
  LuPlay,
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
import { videoSourceAdapter, type VideoSourceSummaryDto } from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: VideoSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: [source: VideoSourceSummaryDto]
  savedAndDebug: [source: VideoSourceSummaryDto]
}>()

const { t } = useI18n()

type EditTab = 'basic' | 'categories' | 'search' | 'explore' | 'detail' | 'play' | 'json'

const formatOptions = computed(() => [
  { value: 'applecms_v10', label: t('onlineSources.edit.formatApplecms') },
  { value: 'rule', label: t('onlineSources.edit.formatRule') },
])

const categoriesAliases: Record<string, readonly string[]> = {
  list: ['bookList'],
  name: ['title'],
}

const searchAliases: Record<string, readonly string[]> = {
  list: ['bookList'],
  vodId: ['id'],
  name: ['title'],
  coverUrl: ['posterUrl'],
  typeName: ['category'],
  detailUrl: ['bookUrl'],
}

const detailAliases: Record<string, readonly string[]> = {
  name: ['title'],
  coverUrl: ['posterUrl'],
  intro: ['description'],
  playGroups: ['groups'],
  episodes: ['chapterList'],
  episodeName: ['chapterName'],
  episodeUrl: ['chapterUrl'],
}

const categoriesRuleFields: RuleFieldDef[] = [
  { key: 'list', label: 'list', fullWidth: true },
  { key: 'id', label: 'id' },
  { key: 'name', label: 'name / title' },
]

const searchRuleFields: RuleFieldDef[] = [
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

const detailRuleFields: RuleFieldDef[] = [
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

const playRuleFields: RuleFieldDef[] = [
  { key: 'requestUrl', label: 'requestUrl', fullWidth: true },
  { key: 'streamUrl', label: 'streamUrl', fullWidth: true },
  { key: 'streamType', label: 'streamType', placeholder: 'hls | mp4 | auto' },
  { key: 'headers', label: 'headers', multiline: true, rows: 2, fullWidth: true },
  { key: 'subtitles', label: 'subtitles', multiline: true, rows: 2, fullWidth: true },
]

interface KeyMap {
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

const keyMap = reactive<KeyMap>({
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
})

interface VideoSnapshot {
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

const form = reactive({
  sourceName: '',
  sourceUrl: '',
  sourceGroup: '',
  sourceComment: '',
  sourceFormat: 'applecms_v10',
  enabled: true,
  enabledExplore: true,
  enabledCookieJar: true,
  searchable: true,
  weight: 0 as number | string,
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
  ruleCategories: {} as Record<string, string>,
  ruleSearch: {} as Record<string, string>,
  ruleExplore: {} as Record<string, string>,
  ruleDetail: {} as Record<string, string>,
  rulePlay: {} as Record<string, string>,
})

let initialSnapshot: VideoSnapshot | null = null

function normalizeListToCsv(raw: unknown): string {
  if (Array.isArray(raw)) {
    return raw.map((v) => String(v ?? '').trim()).filter(Boolean).join(', ')
  }
  return strVal(raw)
}

function clearFormState(): void {
  initialSnapshot = null
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

  form.sourceName = ''
  form.sourceUrl = ''
  form.sourceGroup = ''
  form.sourceComment = ''
  form.sourceFormat = 'applecms_v10'
  form.enabled = true
  form.enabledExplore = true
  form.enabledCookieJar = true
  form.searchable = true
  form.weight = 0
  form.concurrentRate = ''
  form.checkKeyword = ''
  form.apiUrl = ''
  form.preferredPlayFrom = ''
  form.playUrlFilterRegex = ''
  form.header = ''
  form.mediaHeader = ''
  form.loginUrl = ''
  form.loginUi = ''
  form.loginCheckJs = ''
  form.jsLib = ''
  form.categoriesUrl = ''
  form.searchUrl = ''
  form.exploreUrl = ''
  form.ruleCategories = readRuleMap(undefined, categoriesRuleFields, categoriesAliases)
  form.ruleSearch = readRuleMap(undefined, searchRuleFields, searchAliases)
  form.ruleExplore = readRuleMap(undefined, searchRuleFields, searchAliases)
  form.ruleDetail = readRuleMap(undefined, detailRuleFields, detailAliases)
  form.rulePlay = readRuleMap(undefined, playRuleFields)
}

function captureSnapshot(): VideoSnapshot {
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

function resolveVideoFormat(obj: Record<string, unknown>, srcSummary?: VideoSourceSummaryDto): string {
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

function hydrateFormFromObject(
  obj: Record<string, unknown>,
  srcSummary?: VideoSourceSummaryDto,
): void {
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

  form.ruleCategories = readRuleMap(obj.ruleCategories, categoriesRuleFields, categoriesAliases)
  form.ruleSearch = readRuleMap(obj.ruleSearch, searchRuleFields, searchAliases)
  form.ruleExplore = readRuleMap(obj.ruleExplore, searchRuleFields, searchAliases)
  form.ruleDetail = readRuleMap(obj.ruleDetail, detailRuleFields, detailAliases)
  form.rulePlay = readRuleMap(obj.rulePlay, playRuleFields)
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
} = useOnlineSourceEdit<VideoSourceSummaryDto, EditTab>({
  open: toRef(props, 'open'),
  source: toRef(props, 'source'),
  adapter: videoSourceAdapter,
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
        categoriesRuleFields,
        categoriesAliases,
      ],
      ['ruleSearch', form.ruleSearch, snap?.ruleSearch, searchRuleFields, searchAliases],
      ['ruleExplore', form.ruleExplore, snap?.ruleExplore, searchRuleFields, searchAliases],
      ['ruleDetail', form.ruleDetail, snap?.ruleDetail, detailRuleFields, detailAliases],
      ['rulePlay', form.rulePlay, snap?.rulePlay, playRuleFields, undefined],
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
    const name = pickFirstString(obj, ['sourceName', 'name', 'bookSourceName']).trim()
    const url = pickFirstString(obj, ['sourceUrl', 'url', 'bookSourceUrl']).trim()
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

const domainLabel = computed(() => t('onlineSources.domainLabel.video'))

const tabItems = computed<EditTabItem<EditTab>[]>(() => [
  { id: 'basic', label: t('onlineSources.edit.tabBasic'), icon: LuInfo, activeDot: false },
  {
    id: 'categories',
    label: t('onlineSources.edit.tabCategories'),
    icon: LuFolderTree,
    activeDot: Boolean(form.categoriesUrl.trim() || hasAnyRule(form.ruleCategories)),
  },
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
    icon: LuFilm,
    activeDot: hasAnyRule(form.ruleDetail),
  },
  {
    id: 'play',
    label: t('onlineSources.edit.tabPlay'),
    icon: LuPlay,
    activeDot: hasAnyRule(form.rulePlay),
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
            <input v-model="form.sourceName" type="text" class="md-input w-full py-2 text-sm" />
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
              {{ t('onlineSources.edit.sourceFormatLabel') }}
              <span class="font-mono opacity-65">{{ keyMap.formatKey }}</span>
            </span>
            <AppSelect v-model="form.sourceFormat" :options="formatOptions" />
          </div>

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.checkKeywordLabel') }}
              <span class="font-mono opacity-65">{{ keyMap.checkKeywordKey }}</span>
            </span>
            <input
              v-model="form.checkKeyword"
              type="text"
              :placeholder="t('onlineSources.edit.checkKeywordPlaceholder')"
              class="md-input w-full py-2 text-xs"
            />
          </label>

          <div class="grid grid-cols-2 gap-2.5 sm:col-span-2">
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
            <AppSwitch v-model="form.searchable" />
            <span>{{ t('onlineSources.edit.searchableSwitch') }}</span>
          </label>
          <label class="inline-flex items-center gap-2 text-xs text-[var(--md-sys-color-on-surface)]">
            <AppSwitch v-model="form.enabledCookieJar" />
            <span>{{ t('onlineSources.edit.enabledCookieJarSwitch') }}</span>
          </label>
        </div>
      </section>

      <!-- AppleCMS v10 Config -->
      <section
        v-if="form.sourceFormat === 'applecms_v10'"
        class="flex flex-col gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4"
      >
        <h4 class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('onlineSources.edit.applecmsSectionTitle') }}
        </h4>
        <div class="grid grid-cols-1 gap-3">
          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.apiUrlLabel') }}
              <span class="font-mono opacity-65">{{ keyMap.apiUrlKey }}</span>
            </span>
            <input
              v-model="form.apiUrl"
              type="text"
              :placeholder="t('onlineSources.edit.apiUrlPlaceholder')"
              class="md-input w-full py-2 font-mono text-xs"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.preferredPlayFromLabel') }}
              <span class="font-mono opacity-65">preferredPlayFrom</span>
            </span>
            <input
              v-model="form.preferredPlayFrom"
              type="text"
              :placeholder="t('onlineSources.edit.preferredPlayFromPlaceholder')"
              class="md-input w-full py-2 font-mono text-xs"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.playUrlFilterRegexLabel') }}
              <span class="font-mono opacity-65">playUrlFilterRegex</span>
            </span>
            <input
              v-model="form.playUrlFilterRegex"
              type="text"
              spellcheck="false"
              class="md-input w-full py-2 font-mono text-xs"
            />
          </label>
        </div>
      </section>

      <!-- Headers & Auth -->
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
              {{ t('onlineSources.edit.mediaHeaderLabel') }}
              <span class="font-mono opacity-65">{{ keyMap.mediaHeaderKey }}</span>
            </span>
            <textarea
              v-model="form.mediaHeader"
              rows="2"
              spellcheck="false"
              :placeholder="t('onlineSources.edit.headerPlaceholder')"
              class="md-input w-full py-2 font-mono text-xs leading-5"
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

    <template v-else-if="activeTab === 'categories'">
      <section class="flex flex-col gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
        <h4 class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('onlineSources.edit.categoriesUrlLabel') }}
        </h4>
        <textarea
          v-model="form.categoriesUrl"
          rows="3"
          spellcheck="false"
          class="md-input w-full py-2 font-mono text-xs leading-5"
        />
      </section>

      <RuleFieldsSection
        v-model="form.ruleCategories"
        title="ruleCategories"
        :fields="categoriesRuleFields"
      />
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
          rows="3"
          spellcheck="false"
          class="md-input w-full py-2 font-mono text-xs leading-5"
        />
      </section>

      <RuleFieldsSection
        v-model="form.ruleExplore"
        title="ruleExplore"
        :fields="searchRuleFields"
      />
    </template>

    <template v-else-if="activeTab === 'detail'">
      <RuleFieldsSection
        v-model="form.ruleDetail"
        title="ruleDetail"
        :fields="detailRuleFields"
      />
    </template>

    <template v-else-if="activeTab === 'play'">
      <RuleFieldsSection
        v-model="form.rulePlay"
        title="rulePlay"
        :fields="playRuleFields"
      />
    </template>
  </OnlineSourceEditShell>
</template>
