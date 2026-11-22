<script setup lang="ts">
import { computed, reactive, ref, toRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuCheckCircle2,
  LuChevronDown,
  LuCode2,
  LuCompass,
  LuCopy,
  LuDisc3,
  LuFileText,
  LuFlaskConical,
  LuImage,
  LuInfo,
  LuLoader2,
  LuMusic,
  LuSave,
  LuSearch,
  LuSlidersHorizontal,
  LuXCircle,
} from 'vue-icons-plus/lu'
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
  useOnlineSourceEdit,
} from '@/components/online-sources/useOnlineSourceEdit'
import { MdButton, MdDialog, MdSwitch, MdTabs, type MdTabItem } from '@/components/ui'
import { ensureMusicLocale, useMusicI18n } from '../i18n'
import { musicSourceAdapter, type MusicSourceSummaryDto } from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: MusicSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: [source: MusicSourceSummaryDto]
  savedAndDebug: [source: MusicSourceSummaryDto]
}>()

const { t } = useI18n()
const { t: tm } = useMusicI18n()

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      void ensureMusicLocale()
      advancedOpen.value = false
    }
  },
  { immediate: true },
)

type EditTab =
  | 'basic'
  | 'search'
  | 'explore'
  | 'track_detail'
  | 'stream'
  | 'cover'
  | 'lyric'
  | 'json'

const advancedOpen = ref(false)

const searchAliases: Record<string, readonly string[]> = {
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

const trackDetailAliases: Record<string, readonly string[]> = {
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

const streamAliases: Record<string, readonly string[]> = {
  requestUrl: ['url'],
  streamUrl: ['urlRule', 'playUrl'],
}

const coverAliases: Record<string, readonly string[]> = {
  requestUrl: ['url'],
  coverUrl: ['urlRule'],
}

const lyricAliases: Record<string, readonly string[]> = {
  requestUrl: ['url'],
  lyric: ['syncedLyrics'],
  translatedLyric: ['tlyric'],
  plainLyric: ['plainLyrics'],
}

interface FieldSchemaSpec {
  key: string
  placeholder?: string
  multiline?: boolean
  rows?: number
  fullWidth?: boolean
}

const searchFieldSpecs: readonly FieldSchemaSpec[] = [
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

const exploreFieldSpecs: readonly FieldSchemaSpec[] = searchFieldSpecs.filter(
  (f) => f.key !== 'checkKeyWord',
)

const trackDetailFieldSpecs: readonly FieldSchemaSpec[] = [
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

const streamFieldSpecs: readonly FieldSchemaSpec[] = [
  { key: 'requestUrl', fullWidth: true },
  { key: 'streamUrl', multiline: true, rows: 2, fullWidth: true },
  { key: 'format' },
  { key: 'bitrate' },
  { key: 'expiresInSecs' },
  { key: 'headers', multiline: true, rows: 2, fullWidth: true },
]

const coverFieldSpecs: readonly FieldSchemaSpec[] = [
  { key: 'requestUrl', fullWidth: true },
  { key: 'coverUrl', fullWidth: true },
]

const lyricFieldSpecs: readonly FieldSchemaSpec[] = [
  { key: 'requestUrl', fullWidth: true },
  { key: 'lyric', multiline: true, rows: 2, fullWidth: true },
  { key: 'translatedLyric', multiline: true, rows: 2, fullWidth: true },
  { key: 'plainLyric', multiline: true, rows: 2, fullWidth: true },
]

function toLocalizedFields(specs: readonly FieldSchemaSpec[]): RuleFieldDef[] {
  return specs.map((spec) => ({
    key: spec.key,
    label: tm(`sourceEditor.fields.${spec.key}`),
    hint: spec.key,
    placeholder: spec.placeholder,
    multiline: spec.multiline,
    rows: spec.rows,
    fullWidth: spec.fullWidth,
  }))
}

const searchRuleFields = computed(() => toLocalizedFields(searchFieldSpecs))
const exploreRuleFields = computed(() => toLocalizedFields(exploreFieldSpecs))
const trackDetailRuleFields = computed(() => toLocalizedFields(trackDetailFieldSpecs))
const streamRuleFields = computed(() => toLocalizedFields(streamFieldSpecs))
const coverRuleFields = computed(() => toLocalizedFields(coverFieldSpecs))
const lyricRuleFields = computed(() => toLocalizedFields(lyricFieldSpecs))

function specsToKeys(specs: readonly FieldSchemaSpec[]): RuleFieldDef[] {
  return specs.map((spec) => ({ key: spec.key, label: spec.key }))
}

const searchRuleKeyDefs = specsToKeys(searchFieldSpecs)
const exploreRuleKeyDefs = specsToKeys(exploreFieldSpecs)
const trackDetailRuleKeyDefs = specsToKeys(trackDetailFieldSpecs)
const streamRuleKeyDefs = specsToKeys(streamFieldSpecs)
const coverRuleKeyDefs = specsToKeys(coverFieldSpecs)
const lyricRuleKeyDefs = specsToKeys(lyricFieldSpecs)

interface KeyMap {
  nameKey: string
  urlKey: string
  headerKey: string
  mediaHeaderKey: string
}

const keyMap = reactive<KeyMap>({
  nameKey: 'sourceName',
  urlKey: 'sourceUrl',
  headerKey: 'header',
  mediaHeaderKey: 'mediaHeader',
})

interface MusicSnapshot {
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

const form = reactive({
  sourceName: '',
  sourceUrl: '',
  sourceGroup: '',
  sourceComment: '',
  enabled: true,
  enabledCookieJar: true,
  weight: 0 as number | string,
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
  ruleSearch: {} as Record<string, string>,
  ruleExplore: {} as Record<string, string>,
  ruleTrackDetail: {} as Record<string, string>,
  ruleStream: {} as Record<string, string>,
  ruleCover: {} as Record<string, string>,
  ruleLyric: {} as Record<string, string>,
})

let initialSnapshot: MusicSnapshot | null = null

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

function clearFormState(): void {
  initialSnapshot = null
  keyMap.nameKey = 'sourceName'
  keyMap.urlKey = 'sourceUrl'
  keyMap.headerKey = 'header'
  keyMap.mediaHeaderKey = 'mediaHeader'

  form.sourceName = ''
  form.sourceUrl = ''
  form.sourceGroup = ''
  form.sourceComment = ''
  form.enabled = true
  form.enabledCookieJar = true
  form.weight = 0
  form.concurrentRate = ''
  form.supportedQualities = ''
  form.header = ''
  form.mediaHeader = ''
  form.loginUrl = ''
  form.loginUi = ''
  form.loginCheckJs = ''
  form.jsLib = ''
  form.searchUrl = ''
  form.exploreUrl = ''
  form.ruleSearch = readRuleMap(undefined, searchRuleKeyDefs, searchAliases)
  form.ruleExplore = readRuleMap(undefined, exploreRuleKeyDefs, searchAliases)
  form.ruleTrackDetail = readRuleMap(undefined, trackDetailRuleKeyDefs, trackDetailAliases)
  form.ruleStream = readRuleMap(undefined, streamRuleKeyDefs, streamAliases)
  form.ruleCover = readRuleMap(undefined, coverRuleKeyDefs, coverAliases)
  form.ruleLyric = readRuleMap(undefined, lyricRuleKeyDefs, lyricAliases)
}

function captureSnapshot(): MusicSnapshot {
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

function hydrateFormFromObject(
  obj: Record<string, unknown>,
  srcSummary?: MusicSourceSummaryDto,
): void {
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

  form.ruleSearch = readRuleMap(obj.ruleSearch, searchRuleKeyDefs, searchAliases)
  form.ruleExplore = readRuleMap(obj.ruleExplore, exploreRuleKeyDefs, searchAliases)
  form.ruleTrackDetail = readRuleMap(obj.ruleTrackDetail, trackDetailRuleKeyDefs, trackDetailAliases)
  form.ruleStream = readRuleMap(obj.ruleStream, streamRuleKeyDefs, streamAliases)
  form.ruleCover = readRuleMap(obj.ruleCover, coverRuleKeyDefs, coverAliases)
  form.ruleLyric = readRuleMap(obj.ruleLyric, lyricRuleKeyDefs, lyricAliases)
  initialSnapshot = captureSnapshot()
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
} = useOnlineSourceEdit<MusicSourceSummaryDto, EditTab>({
  open: toRef(props, 'open'),
  source: toRef(props, 'source'),
  adapter: musicSourceAdapter,
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
      ['ruleSearch', form.ruleSearch, snap?.ruleSearch, searchRuleKeyDefs, searchAliases],
      ['ruleExplore', form.ruleExplore, snap?.ruleExplore, exploreRuleKeyDefs, searchAliases],
      [
        'ruleTrackDetail',
        form.ruleTrackDetail,
        snap?.ruleTrackDetail,
        trackDetailRuleKeyDefs,
        trackDetailAliases,
      ],
      ['ruleStream', form.ruleStream, snap?.ruleStream, streamRuleKeyDefs, streamAliases],
      ['ruleCover', form.ruleCover, snap?.ruleCover, coverRuleKeyDefs, coverAliases],
      ['ruleLyric', form.ruleLyric, snap?.ruleLyric, lyricRuleKeyDefs, lyricAliases],
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
    const name = pickFirstString(obj, ['sourceName', 'name']).trim()
    const url = pickFirstString(obj, ['sourceUrl', 'url']).trim()
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

function countConfigured(map: Record<string, string>, extra = ''): number | undefined {
  let n = extra.trim() ? 1 : 0
  for (const val of Object.values(map)) {
    if (val.trim()) n += 1
  }
  return n > 0 ? n : undefined
}

const advancedConfiguredCount = computed(() => {
  const fields = [
    form.header,
    form.mediaHeader,
    form.loginUrl,
    form.loginUi,
    form.loginCheckJs,
    form.jsLib,
    form.concurrentRate,
    variableText.value,
  ]
  let count = fields.filter((v) => v.trim().length > 0).length
  if (numVal(form.weight, 0) !== 0) count += 1
  if (!form.enabledCookieJar) count += 1
  return count
})

const domainLabel = computed(() => t('onlineSources.domainLabel.music'))

const mdTabs = computed<MdTabItem<EditTab>[]>(() => [
  { value: 'basic', label: t('onlineSources.edit.tabBasic'), icon: LuInfo },
  {
    value: 'search',
    label: t('onlineSources.edit.tabSearch'),
    icon: LuSearch,
    badge: countConfigured(form.ruleSearch, form.searchUrl),
  },
  {
    value: 'explore',
    label: t('onlineSources.edit.tabExplore'),
    icon: LuCompass,
    badge: countConfigured(form.ruleExplore, form.exploreUrl),
  },
  {
    value: 'track_detail',
    label: t('onlineSources.edit.tabTrackDetail'),
    icon: LuDisc3,
    badge: countConfigured(form.ruleTrackDetail),
  },
  {
    value: 'stream',
    label: t('onlineSources.edit.tabStream'),
    icon: LuMusic,
    badge: countConfigured(form.ruleStream),
  },
  {
    value: 'cover',
    label: t('onlineSources.edit.tabCover'),
    icon: LuImage,
    badge: countConfigured(form.ruleCover),
  },
  {
    value: 'lyric',
    label: t('onlineSources.debug.modeLyric'),
    icon: LuFileText,
    badge: countConfigured(form.ruleLyric),
  },
  { value: 'json', label: t('onlineSources.edit.tabJson'), icon: LuCode2 },
])

interface ActiveRuleSection {
  title: string
  codeHint: string
  fields: RuleFieldDef[]
  model: Record<string, string>
  setField: (key: string, val: string) => void
}

const activeRuleSection = computed<ActiveRuleSection | null>(() => {
  switch (activeTab.value) {
    case 'search':
      return {
        title: tm('sourceEditor.sectionSearch'),
        codeHint: 'ruleSearch',
        fields: searchRuleFields.value,
        model: form.ruleSearch,
        setField: (k, v) => {
          form.ruleSearch = { ...form.ruleSearch, [k]: v }
        },
      }
    case 'explore':
      return {
        title: tm('sourceEditor.sectionExplore'),
        codeHint: 'ruleExplore',
        fields: exploreRuleFields.value,
        model: form.ruleExplore,
        setField: (k, v) => {
          form.ruleExplore = { ...form.ruleExplore, [k]: v }
        },
      }
    case 'track_detail':
      return {
        title: tm('sourceEditor.sectionTrackDetail'),
        codeHint: 'ruleTrackDetail',
        fields: trackDetailRuleFields.value,
        model: form.ruleTrackDetail,
        setField: (k, v) => {
          form.ruleTrackDetail = { ...form.ruleTrackDetail, [k]: v }
        },
      }
    case 'stream':
      return {
        title: tm('sourceEditor.sectionStream'),
        codeHint: 'ruleStream',
        fields: streamRuleFields.value,
        model: form.ruleStream,
        setField: (k, v) => {
          form.ruleStream = { ...form.ruleStream, [k]: v }
        },
      }
    case 'cover':
      return {
        title: tm('sourceEditor.sectionCover'),
        codeHint: 'ruleCover',
        fields: coverRuleFields.value,
        model: form.ruleCover,
        setField: (k, v) => {
          form.ruleCover = { ...form.ruleCover, [k]: v }
        },
      }
    case 'lyric':
      return {
        title: tm('sourceEditor.sectionLyric'),
        codeHint: 'ruleLyric',
        fields: lyricRuleFields.value,
        model: form.ruleLyric,
        setField: (k, v) => {
          form.ruleLyric = { ...form.ruleLyric, [k]: v }
        },
      }
    default:
      return null
  }
})
</script>

<template>
  <MdDialog
    :open="open"
    :title="
      t('onlineSources.edit.title', {
        domain: domainLabel,
        name: form.sourceName || source?.name || '',
      })
    "
    :subtitle="form.sourceUrl || source?.source_url"
    fullscreen
    @close="emit('close')"
  >
    <div class="mx-auto flex w-full max-w-4xl flex-col gap-5">
      <MdTabs
        :model-value="activeTab"
        :tabs="mdTabs"
        variant="secondary"
        :aria-label="domainLabel"
        @update:model-value="switchTab($event)"
      />

      <!-- Loading -->
      <div
        v-if="loading"
        class="text-body-medium flex items-center justify-center gap-2 py-20 text-on-surface-variant"
      >
        <LuLoader2 class="h-4 w-4 animate-spin" aria-hidden="true" />
        <span>{{ t('onlineSources.edit.loading', { domain: domainLabel }) }}</span>
      </div>

      <!-- Load Error -->
      <div
        v-else-if="loadError"
        role="alert"
        class="text-body-small rounded-corner-lg bg-error-container px-4 py-3 text-on-error-container"
      >
        {{ loadError }}
      </div>

      <!-- Basic Tab -->
      <template v-else-if="activeTab === 'basic'">
        <section class="flex flex-col gap-4 rounded-corner-lg bg-surface-container p-4 sm:p-5">
          <h4 class="text-title-small text-on-surface">
            {{ t('onlineSources.edit.basicSectionTitle') }}
          </h4>

          <div class="grid grid-cols-1 gap-3.5 sm:grid-cols-2">
            <label class="block space-y-1.5">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ t('onlineSources.edit.nameLabel') }}</span>
                <span class="font-mono opacity-60">{{ keyMap.nameKey }}</span>
              </span>
              <input v-model="form.sourceName" type="text" class="md-input text-body-medium w-full py-2" />
            </label>

            <label class="block space-y-1.5">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ t('onlineSources.edit.groupLabel') }}</span>
                <span class="font-mono opacity-60">sourceGroup</span>
              </span>
              <input
                v-model="form.sourceGroup"
                name="group_name"
                type="text"
                :aria-label="t('onlineSources.edit.groupLabel')"
                :placeholder="t('onlineSources.edit.groupPlaceholder')"
                class="md-input text-body-medium w-full py-2"
              />
            </label>

            <label class="block space-y-1.5 sm:col-span-2">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ t('onlineSources.edit.urlLabel') }}</span>
                <span class="font-mono opacity-60">{{ keyMap.urlKey }}</span>
              </span>
              <input
                v-model="form.sourceUrl"
                type="text"
                :placeholder="t('onlineSources.edit.urlPlaceholder')"
                class="md-input text-body-small w-full py-2 font-mono"
              />
            </label>

            <label class="block space-y-1.5 sm:col-span-2">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ t('onlineSources.edit.supportedQualitiesLabel') }}</span>
                <span class="font-mono opacity-60">supportedQualities</span>
              </span>
              <input
                v-model="form.supportedQualities"
                type="text"
                :placeholder="t('onlineSources.edit.supportedQualitiesPlaceholder')"
                class="md-input text-body-small w-full py-2 font-mono"
              />
            </label>

            <label class="block space-y-1.5 sm:col-span-2">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ t('onlineSources.edit.commentLabel') }}</span>
                <span class="font-mono opacity-60">sourceComment</span>
              </span>
              <textarea
                v-model="form.sourceComment"
                rows="2"
                :placeholder="t('onlineSources.edit.commentPlaceholder')"
                class="md-input text-body-small w-full py-2 leading-5"
              />
            </label>
          </div>

          <div class="border-t border-outline-variant pt-3">
            <MdSwitch v-model="form.enabled" :label="t('onlineSources.edit.enabledSwitch')" />
          </div>
        </section>

        <!-- Collapsible Advanced Network, Login & Scripts -->
        <section class="overflow-hidden rounded-corner-lg bg-surface-container">
          <button
            type="button"
            :aria-expanded="advancedOpen"
            class="md-state flex w-full cursor-pointer items-center justify-between gap-3 px-4 py-3.5 text-left sm:px-5"
            @click="advancedOpen = !advancedOpen"
          >
            <div class="flex min-w-0 items-center gap-2.5">
              <LuSlidersHorizontal class="h-4 w-4 shrink-0 text-on-surface-variant" aria-hidden="true" />
              <span class="text-title-small text-on-surface">
                {{ tm('sourceEditor.advancedSectionTitle') }}
              </span>
              <span
                v-if="advancedConfiguredCount > 0"
                class="text-label-small rounded-corner-full bg-secondary-container px-2.5 py-0.5 text-on-secondary-container"
              >
                {{ tm('sourceEditor.advancedConfiguredCount', { count: advancedConfiguredCount }) }}
              </span>
            </div>
            <LuChevronDown
              class="h-4 w-4 shrink-0 text-on-surface-variant transition-transform duration-200 ease-standard"
              :class="advancedOpen ? 'rotate-180' : ''"
              aria-hidden="true"
            />
          </button>

          <div
            v-if="advancedOpen"
            class="flex flex-col gap-3.5 border-t border-outline-variant p-4 sm:p-5"
          >
            <div class="grid grid-cols-1 gap-3.5 sm:grid-cols-2">
              <label class="block space-y-1.5">
                <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                  <span>{{ t('onlineSources.edit.concurrentRateLabel') }}</span>
                  <span class="font-mono opacity-60">concurrentRate</span>
                </span>
                <input
                  v-model="form.concurrentRate"
                  type="text"
                  :placeholder="t('onlineSources.edit.concurrentRatePlaceholder')"
                  class="md-input text-body-small w-full py-2 font-mono"
                />
              </label>

              <label class="block space-y-1.5">
                <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                  <span>{{ t('onlineSources.edit.weightLabel') }}</span>
                  <span class="font-mono opacity-60">weight</span>
                </span>
                <input
                  v-model.number="form.weight"
                  type="number"
                  class="md-input text-body-small w-full py-2 font-mono"
                />
              </label>
            </div>

            <MdSwitch
              v-model="form.enabledCookieJar"
              :label="t('onlineSources.edit.enabledCookieJarSwitch')"
            />

            <label class="block space-y-1.5">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ t('onlineSources.edit.headerLabel') }}</span>
                <span class="font-mono opacity-60">header</span>
              </span>
              <textarea
                v-model="form.header"
                rows="3"
                spellcheck="false"
                :placeholder="t('onlineSources.edit.headerPlaceholder')"
                class="md-input text-body-small w-full py-2 font-mono leading-5"
              />
            </label>

            <label class="block space-y-1.5">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ t('onlineSources.edit.mediaHeaderLabel') }}</span>
                <span class="font-mono opacity-60">mediaHeader</span>
              </span>
              <textarea
                v-model="form.mediaHeader"
                rows="3"
                spellcheck="false"
                :placeholder="t('onlineSources.edit.headerPlaceholder')"
                class="md-input text-body-small w-full py-2 font-mono leading-5"
              />
            </label>

            <label class="block space-y-1.5">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ t('onlineSources.edit.loginUrlLabel') }}</span>
                <span class="font-mono opacity-60">loginUrl</span>
              </span>
              <textarea
                v-model="form.loginUrl"
                rows="2"
                spellcheck="false"
                class="md-input text-body-small w-full py-2 font-mono leading-5"
              />
            </label>

            <label class="block space-y-1.5">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ t('onlineSources.edit.loginUiLabel') }}</span>
                <span class="font-mono opacity-60">loginUi</span>
              </span>
              <textarea
                v-model="form.loginUi"
                rows="3"
                spellcheck="false"
                class="md-input text-body-small w-full py-2 font-mono leading-5"
              />
            </label>

            <label class="block space-y-1.5">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ t('onlineSources.edit.loginCheckJsLabel') }}</span>
                <span class="font-mono opacity-60">loginCheckJs</span>
              </span>
              <textarea
                v-model="form.loginCheckJs"
                rows="2"
                spellcheck="false"
                class="md-input text-body-small w-full py-2 font-mono leading-5"
              />
            </label>

            <label class="block space-y-1.5">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ t('onlineSources.edit.jsLibLabel') }}</span>
                <span class="font-mono opacity-60">jsLib</span>
              </span>
              <textarea
                v-model="form.jsLib"
                rows="3"
                spellcheck="false"
                class="md-input text-body-small w-full py-2 font-mono leading-5"
              />
            </label>

            <label class="block space-y-1.5">
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ t('onlineSources.edit.variableLabel') }}</span>
                <span class="font-mono opacity-60">source.variable</span>
              </span>
              <input
                v-model="variableText"
                type="text"
                spellcheck="false"
                :placeholder="t('onlineSources.edit.variablePlaceholder')"
                class="md-input text-body-small w-full py-2 font-mono"
              />
            </label>
          </div>
        </section>
      </template>

      <!-- JSON Raw Editor Tab -->
      <section
        v-else-if="activeTab === 'json'"
        class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4 sm:p-5"
      >
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex items-center gap-2">
            <h4 class="text-title-small text-on-surface">
              {{ t('onlineSources.edit.jsonTitle') }}
            </h4>
            <span
              v-if="jsonValidation.valid"
              class="text-label-small inline-flex items-center gap-1 rounded-corner-full bg-success-container px-2.5 py-0.5 text-on-success-container"
            >
              <LuCheckCircle2 class="h-3.5 w-3.5" aria-hidden="true" />
              <span>{{ t('onlineSources.edit.jsonValid') }}</span>
            </span>
            <span
              v-else
              class="text-label-small inline-flex items-center gap-1 rounded-corner-full bg-error-container px-2.5 py-0.5 text-on-error-container"
            >
              <LuXCircle class="h-3.5 w-3.5" aria-hidden="true" />
              <span>{{ t('onlineSources.edit.jsonInvalid') }}</span>
            </span>
          </div>

          <div class="flex items-center gap-2">
            <MdButton variant="tonal" size="sm" :icon="LuCode2" @click="formatJson">
              {{ t('onlineSources.edit.formatJsonBtn') }}
            </MdButton>
            <MdButton variant="outlined" size="sm" :icon="LuCopy" @click="copyJson">
              {{ t('onlineSources.edit.copyJsonBtn') }}
            </MdButton>
          </div>
        </div>

        <p v-if="!jsonValidation.valid && jsonValidation.error" class="text-body-small text-error">
          {{ jsonValidation.error }}
        </p>

        <textarea
          :value="jsonText"
          rows="22"
          spellcheck="false"
          class="md-input text-body-small w-full py-2.5 font-mono leading-5"
          @input="
            jsonText = ($event.target as HTMLTextAreaElement).value;
            jsonDirty = true
          "
        />
      </section>

      <!-- Rule Tabs (Search / Explore / Track Detail / Stream / Cover / Lyric) -->
      <template v-else-if="activeRuleSection">
        <section
          v-if="activeTab === 'search'"
          class="flex flex-col gap-2.5 rounded-corner-lg bg-surface-container p-4 sm:p-5"
        >
          <h4 class="text-title-small text-on-surface">
            {{ t('onlineSources.edit.searchUrlLabel') }}
            <span class="text-label-small ml-1.5 font-mono text-on-surface-variant">searchUrl</span>
          </h4>
          <textarea
            v-model="form.searchUrl"
            rows="3"
            spellcheck="false"
            class="md-input text-body-small w-full py-2 font-mono leading-5"
          />
        </section>

        <section
          v-else-if="activeTab === 'explore'"
          class="flex flex-col gap-2.5 rounded-corner-lg bg-surface-container p-4 sm:p-5"
        >
          <h4 class="text-title-small text-on-surface">
            {{ t('onlineSources.edit.exploreUrlLabel') }}
            <span class="text-label-small ml-1.5 font-mono text-on-surface-variant">exploreUrl</span>
          </h4>
          <textarea
            v-model="form.exploreUrl"
            rows="4"
            spellcheck="false"
            class="md-input text-body-small w-full py-2 font-mono leading-5"
          />
        </section>

        <section class="flex flex-col gap-4 rounded-corner-lg bg-surface-container p-4 sm:p-5">
          <div class="flex items-center justify-between gap-2">
            <h4 class="text-title-small text-on-surface">
              {{ activeRuleSection.title }}
            </h4>
            <span class="text-label-small font-mono text-on-surface-variant">
              {{ activeRuleSection.codeHint }}
            </span>
          </div>

          <div class="grid grid-cols-1 gap-3.5 sm:grid-cols-2">
            <label
              v-for="field in activeRuleSection.fields"
              :key="field.key"
              class="block space-y-1.5"
              :class="field.fullWidth ? 'sm:col-span-2' : ''"
            >
              <span class="text-label-medium flex items-center gap-1.5 text-on-surface-variant">
                <span>{{ field.label }}</span>
                <span v-if="field.hint" class="font-mono opacity-60">{{ field.hint }}</span>
              </span>
              <textarea
                v-if="field.multiline"
                :value="activeRuleSection.model[field.key] ?? ''"
                :rows="field.rows ?? 2"
                :placeholder="field.placeholder"
                spellcheck="false"
                class="md-input text-body-small w-full py-2 font-mono leading-5"
                @input="
                  activeRuleSection.setField(
                    field.key,
                    ($event.target as HTMLTextAreaElement).value,
                  )
                "
              />
              <input
                v-else
                :value="activeRuleSection.model[field.key] ?? ''"
                type="text"
                :placeholder="field.placeholder"
                spellcheck="false"
                class="md-input text-body-small w-full py-2 font-mono"
                @input="
                  activeRuleSection.setField(field.key, ($event.target as HTMLInputElement).value)
                "
              />
            </label>
          </div>
        </section>
      </template>
    </div>

    <template #footer>
      <MdButton variant="text" :disabled="saving" @click="emit('close')">
        {{ t('common.cancel') }}
      </MdButton>
      <MdButton
        variant="tonal"
        :icon="LuFlaskConical"
        :disabled="loading || saving || Boolean(loadError) || !jsonValidation.valid"
        @click="saveSource(true)"
      >
        {{ t('onlineSources.edit.saveAndDebugBtn') }}
      </MdButton>
      <MdButton
        variant="filled"
        :icon="LuSave"
        :loading="saving"
        :disabled="loading || saving || Boolean(loadError) || !jsonValidation.valid"
        @click="saveSource(false)"
      >
        {{ saving ? t('onlineSources.edit.savingBtn') : t('onlineSources.edit.saveBtn') }}
      </MdButton>
    </template>
  </MdDialog>
</template>
