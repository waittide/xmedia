<script setup lang="ts">
import { computed, reactive, toRef } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuCode2,
  LuCompass,
  LuDisc3,
  LuFileText,
  LuImage,
  LuInfo,
  LuMusic,
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
import AppSwitch from '@/components/ui/AppSwitch.vue'
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

type EditTab =
  | 'basic'
  | 'search'
  | 'explore'
  | 'track_detail'
  | 'stream'
  | 'cover'
  | 'lyric'
  | 'json'

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

const searchRuleFields: RuleFieldDef[] = [
  { key: 'checkKeyWord', label: 'checkKeyWord' },
  { key: 'trackList', label: 'trackList', fullWidth: true },
  { key: 'trackId', label: 'trackId' },
  { key: 'title', label: 'title' },
  { key: 'artist', label: 'artist' },
  { key: 'album', label: 'album' },
  { key: 'duration', label: 'duration' },
  { key: 'durationUnit', label: 'durationUnit', placeholder: 's | ms' },
  { key: 'coverUrl', label: 'coverUrl', fullWidth: true },
  { key: 'coverId', label: 'coverId' },
  { key: 'urlId', label: 'urlId' },
  { key: 'lyricId', label: 'lyricId' },
  { key: 'format', label: 'format' },
  { key: 'playUrl', label: 'playUrl', fullWidth: true },
  { key: 'detailUrl', label: 'detailUrl', fullWidth: true },
]

const exploreRuleFields: RuleFieldDef[] = searchRuleFields.filter((f) => f.key !== 'checkKeyWord')

const trackDetailRuleFields: RuleFieldDef[] = [
  { key: 'requestUrl', label: 'requestUrl', fullWidth: true },
  { key: 'init', label: 'init', multiline: true, rows: 2, fullWidth: true },
  { key: 'trackId', label: 'trackId' },
  { key: 'title', label: 'title' },
  { key: 'artist', label: 'artist' },
  { key: 'album', label: 'album' },
  { key: 'duration', label: 'duration' },
  { key: 'durationUnit', label: 'durationUnit', placeholder: 's | ms' },
  { key: 'coverUrl', label: 'coverUrl', fullWidth: true },
  { key: 'coverId', label: 'coverId' },
  { key: 'urlId', label: 'urlId' },
  { key: 'lyricId', label: 'lyricId' },
  { key: 'format', label: 'format' },
  { key: 'playUrl', label: 'playUrl', fullWidth: true },
  { key: 'lyric', label: 'lyric', multiline: true, rows: 2, fullWidth: true },
]

const streamRuleFields: RuleFieldDef[] = [
  { key: 'requestUrl', label: 'requestUrl', fullWidth: true },
  { key: 'streamUrl', label: 'streamUrl', multiline: true, rows: 2, fullWidth: true },
  { key: 'format', label: 'format' },
  { key: 'bitrate', label: 'bitrate' },
  { key: 'expiresInSecs', label: 'expiresInSecs' },
  { key: 'headers', label: 'headers', multiline: true, rows: 2, fullWidth: true },
]

const coverRuleFields: RuleFieldDef[] = [
  { key: 'requestUrl', label: 'requestUrl', fullWidth: true },
  { key: 'coverUrl', label: 'coverUrl', fullWidth: true },
]

const lyricRuleFields: RuleFieldDef[] = [
  { key: 'requestUrl', label: 'requestUrl', fullWidth: true },
  { key: 'lyric', label: 'lyric', multiline: true, rows: 2, fullWidth: true },
  { key: 'translatedLyric', label: 'translatedLyric', multiline: true, rows: 2, fullWidth: true },
  { key: 'plainLyric', label: 'plainLyric', multiline: true, rows: 2, fullWidth: true },
]

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
    return raw.map((v) => String(v ?? '').trim()).filter(Boolean).join(', ')
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
  form.ruleSearch = readRuleMap(undefined, searchRuleFields, searchAliases)
  form.ruleExplore = readRuleMap(undefined, exploreRuleFields, searchAliases)
  form.ruleTrackDetail = readRuleMap(undefined, trackDetailRuleFields, trackDetailAliases)
  form.ruleStream = readRuleMap(undefined, streamRuleFields, streamAliases)
  form.ruleCover = readRuleMap(undefined, coverRuleFields, coverAliases)
  form.ruleLyric = readRuleMap(undefined, lyricRuleFields, lyricAliases)
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

  form.ruleSearch = readRuleMap(obj.ruleSearch, searchRuleFields, searchAliases)
  form.ruleExplore = readRuleMap(obj.ruleExplore, exploreRuleFields, searchAliases)
  form.ruleTrackDetail = readRuleMap(obj.ruleTrackDetail, trackDetailRuleFields, trackDetailAliases)
  form.ruleStream = readRuleMap(obj.ruleStream, streamRuleFields, streamAliases)
  form.ruleCover = readRuleMap(obj.ruleCover, coverRuleFields, coverAliases)
  form.ruleLyric = readRuleMap(obj.ruleLyric, lyricRuleFields, lyricAliases)
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
      ['ruleSearch', form.ruleSearch, snap?.ruleSearch, searchRuleFields, searchAliases],
      ['ruleExplore', form.ruleExplore, snap?.ruleExplore, exploreRuleFields, searchAliases],
      [
        'ruleTrackDetail',
        form.ruleTrackDetail,
        snap?.ruleTrackDetail,
        trackDetailRuleFields,
        trackDetailAliases,
      ],
      ['ruleStream', form.ruleStream, snap?.ruleStream, streamRuleFields, streamAliases],
      ['ruleCover', form.ruleCover, snap?.ruleCover, coverRuleFields, coverAliases],
      ['ruleLyric', form.ruleLyric, snap?.ruleLyric, lyricRuleFields, lyricAliases],
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

const domainLabel = computed(() => t('onlineSources.domainLabel.music'))

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
    id: 'track_detail',
    label: t('onlineSources.edit.tabTrackDetail'),
    icon: LuDisc3,
    activeDot: hasAnyRule(form.ruleTrackDetail),
  },
  {
    id: 'stream',
    label: t('onlineSources.edit.tabStream'),
    icon: LuMusic,
    activeDot: hasAnyRule(form.ruleStream),
  },
  {
    id: 'cover',
    label: t('onlineSources.edit.tabCover'),
    icon: LuImage,
    activeDot: hasAnyRule(form.ruleCover),
  },
  {
    id: 'lyric',
    label: t('onlineSources.debug.modeLyric'),
    icon: LuFileText,
    activeDot: hasAnyRule(form.ruleLyric),
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
              <span class="font-mono opacity-65">sourceGroup</span>
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

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.supportedQualitiesLabel') }}
              <span class="font-mono opacity-65">supportedQualities</span>
            </span>
            <input
              v-model="form.supportedQualities"
              type="text"
              :placeholder="t('onlineSources.edit.supportedQualitiesPlaceholder')"
              class="md-input w-full py-2 font-mono text-xs"
            />
          </label>

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
              <span class="font-mono opacity-65">sourceComment</span>
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
              rows="3"
              spellcheck="false"
              :placeholder="t('onlineSources.edit.headerPlaceholder')"
              class="md-input w-full py-2 font-mono text-xs leading-5"
            />
          </label>

          <label class="block space-y-1">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.edit.mediaHeaderLabel') }} <span class="font-mono opacity-65">mediaHeader</span>
            </span>
            <textarea
              v-model="form.mediaHeader"
              rows="3"
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

    <template v-else-if="activeTab === 'track_detail'">
      <RuleFieldsSection
        v-model="form.ruleTrackDetail"
        title="ruleTrackDetail"
        :fields="trackDetailRuleFields"
      />
    </template>

    <template v-else-if="activeTab === 'stream'">
      <RuleFieldsSection
        v-model="form.ruleStream"
        title="ruleStream"
        :fields="streamRuleFields"
      />
    </template>

    <template v-else-if="activeTab === 'cover'">
      <RuleFieldsSection
        v-model="form.ruleCover"
        title="ruleCover"
        :fields="coverRuleFields"
      />
    </template>

    <template v-else-if="activeTab === 'lyric'">
      <RuleFieldsSection
        v-model="form.ruleLyric"
        title="ruleLyric"
        :fields="lyricRuleFields"
      />
    </template>
  </OnlineSourceEditShell>
</template>
