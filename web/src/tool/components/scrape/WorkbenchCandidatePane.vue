<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { LuLoader2, LuSearch, LuSparkles } from 'vue-icons-plus/lu'
import { toolApi } from '../../api'
import { mergeScrapedClientSide } from '../../merger'
import type {
  AggregateScrapeResponse,
  FieldSourceBadge,
  MediaFileDetail,
  MergedScrapedResult,
  ProviderBriefDto,
  ProviderStatusDto,
  PublicSettingsDto,
  ScrapeCandidate,
  ScrapedMetadata,
  ScrapeTargetKind,
} from '../../types'
import {
  buildDefaultQueryFromDetail,
  inferTargetKindFromDetail,
  type ApplyFillMode,
} from '../inspector/useMetadataForm'
import type { CandidatePreviewState } from './CandidateDetailPreviewPane.vue'
import {
  buildCandidateFallbackMeta,
  buildLightMetadata,
  candKey,
} from './candidateMetaHelpers'
import WorkbenchCandidateCard from './WorkbenchCandidateCard.vue'
import WorkbenchMergedCard from './WorkbenchMergedCard.vue'
import WorkbenchSearchHeader from './WorkbenchSearchHeader.vue'

const MAX_DETAIL_CACHE = 128

const props = defineProps<{
  detail: MediaFileDetail | null
  autoSearchTrigger: number
  settings?: PublicSettingsDto | null
  externalKeyword?: string
  externalArtist?: string
  selectedPreviewKey?: string | null
}>()

const emit = defineEmits<{
  directApply: [meta: ScrapedMetadata, mode: ApplyFillMode]
  selectPreview: [preview: CandidatePreviewState | null]
  openSettings: []
  settingsUpdated: [settings: PublicSettingsDto]
}>()

const keyword = ref('')
const artistOrAuthor = ref('')
const albumOrSeries = ref('')
const isbn = ref('')
const yearInput = ref('')
const seasonInput = ref('')
const episodeInput = ref('')
const durationSecs = ref<number | undefined>(undefined)
const targetKind = ref<ScrapeTargetKind>('book')
const fillMode = ref<ApplyFillMode>('overwrite')

const providers = ref<ProviderStatusDto[]>([])
const enabledProviderIds = ref<string[]>([])
const ignoredProviderIds = ref<Set<string>>(new Set())
const activeSourceFilter = ref<string>('all')

const searching = ref(false)
const searchError = ref<string | null>(null)
const candidates = ref<ScrapeCandidate[]>([])
const serverMergedResult = ref<MergedScrapedResult | null>(null)
const selectedByProvider = ref<Record<string, ScrapeCandidate>>({})
const detailsByProvider = ref<Record<string, ScrapedMetadata>>({})
const waterfallExpanded = ref(false)
const selectedKey = ref<string | null>(null)
const loadingDetailKey = ref<string | null>(null)
const detailCache = new Map<string, ScrapedMetadata>()
const brokenCoverUrls = ref<Set<string>>(new Set())
const recentFlash = ref<string | null>(null)

let hydratingFromDetail = false
let searchRequestSeq = 0
let providerLoadSeq = 0
let pendingAutoSearch = false

function getCachedDetail(key: string): ScrapedMetadata | undefined {
  const hit = detailCache.get(key)
  if (!hit) return undefined
  detailCache.delete(key)
  detailCache.set(key, hit)
  return hit
}

function setCachedDetail(key: string, value: ScrapedMetadata): void {
  if (detailCache.has(key)) {
    detailCache.delete(key)
  } else if (detailCache.size >= MAX_DETAIL_CACHE) {
    const oldestKey = detailCache.keys().next().value
    if (oldestKey !== undefined) {
      detailCache.delete(oldestKey)
    }
  }
  detailCache.set(key, value)
}

const targetTabs = computed<Array<{ value: ScrapeTargetKind; label: string }>>(() => {
  const kind = props.detail?.entry.media_kind
  if (kind === 'music') {
    return [
      { value: 'music_track', label: '单曲' },
      { value: 'music_album', label: '整张专辑' },
      { value: 'lyrics', label: '仅歌词' },
    ]
  }
  if (kind === 'video') {
    return [
      { value: 'tv_show', label: '剧集 / 番剧' },
      { value: 'movie', label: '电影' },
    ]
  }
  if (kind === 'comic') {
    return [
      { value: 'comic', label: '漫画源' },
      { value: 'book', label: '图书 / 轻小说源' },
    ]
  }
  return [{ value: 'book', label: '书籍' }]
})

const canSearch = computed<boolean>(() =>
  Boolean(props.detail && (keyword.value.trim().length > 0 || isbn.value.trim().length > 0)),
)

const orderedActiveBriefs = computed<ProviderBriefDto[]>(() =>
  enabledProviderIds.value
    .map((id) => providers.value.find((p) => p.id === id && p.configured))
    .filter((p): p is ProviderStatusDto => Boolean(p))
    .map((p) => ({ id: p.id, name: p.name })),
)

const mergedResult = computed<MergedScrapedResult | null>(() => {
  if (Object.keys(detailsByProvider.value).length > 0) {
    return mergeScrapedClientSide(
      orderedActiveBriefs.value,
      detailsByProvider.value,
      targetKind.value,
      brokenCoverUrls.value,
      ignoredProviderIds.value,
      {
        keyword: keyword.value.trim() || undefined,
        author: artistOrAuthor.value.trim() || undefined,
        isbn: isbn.value.trim() || undefined,
      },
    )
  }
  return serverMergedResult.value
})

const mergedMeta = computed<ScrapedMetadata | null>(() => mergedResult.value?.metadata ?? null)
const mergedFieldSources = computed<Record<string, FieldSourceBadge>>(
  () => mergedResult.value?.field_sources ?? {},
)

const providerCandidateCounts = computed<Record<string, number>>(() => {
  const counts: Record<string, number> = {}
  for (const c of candidates.value) {
    counts[c.provider_id] = (counts[c.provider_id] ?? 0) + 1
  }
  return counts
})

const configuredProviders = computed(() => providers.value.filter((p) => p.configured))

const visibleCandidates = computed(() => {
  if (activeSourceFilter.value === 'all') return candidates.value
  return candidates.value.filter((c) => c.provider_id === activeSourceFilter.value)
})

const sourceCounts = computed(() => {
  const m = new Map<string, { id: string; name: string; count: number }>()
  for (const c of candidates.value) {
    const existing = m.get(c.provider_id)
    if (existing) existing.count += 1
    else m.set(c.provider_id, { id: c.provider_id, name: c.provider_name, count: 1 })
  }
  return Array.from(m.values())
})

function isCoverUsable(url: string | null | undefined): boolean {
  const trimmed = url?.trim()
  return Boolean(trimmed && !brokenCoverUrls.value.has(trimmed))
}

function markCoverBroken(url: string | null | undefined): void {
  const trimmed = url?.trim()
  if (!trimmed || brokenCoverUrls.value.has(trimmed)) return
  const next = new Set(brokenCoverUrls.value)
  next.add(trimmed)
  brokenCoverUrls.value = next
}

function currentEntryKey(): string {
  return props.detail ? `${props.detail.entry.root_id}::${props.detail.entry.rel_path}` : ''
}

function flashFeedback(label: string): void {
  recentFlash.value = label
  setTimeout(() => {
    if (recentFlash.value === label) recentFlash.value = null
  }, 1800)
}

function selectMergedPreview(): void {
  if (!mergedMeta.value) return
  selectedKey.value = '__merged__'
  emit('selectPreview', {
    key: '__merged__',
    isMerged: true,
    sourceLabel: '多源瀑布聚合结果',
    meta: mergedMeta.value,
    loading: false,
    fieldSources: mergedFieldSources.value,
    fillMode: fillMode.value,
  })
}

async function resolveCandidateDetail(c: ScrapeCandidate): Promise<ScrapedMetadata | null> {
  const key = candKey(c)
  const cached = getCachedDetail(key)
  if (cached) return cached
  loadingDetailKey.value = key
  try {
    const meta = await toolApi.getScraperDetail(c.provider_id, c.external_id, c.target)
    setCachedDetail(key, meta)
    return meta
  } catch {
    const fallback = buildCandidateFallbackMeta(c)
    setCachedDetail(key, fallback)
    return fallback
  } finally {
    if (loadingDetailKey.value === key) loadingDetailKey.value = null
  }
}

async function selectCandidatePreview(c: ScrapeCandidate): Promise<void> {
  const entryKey = currentEntryKey()
  const key = candKey(c)
  selectedKey.value = key
  const cached = getCachedDetail(key)
  if (cached) {
    emit('selectPreview', {
      key,
      isMerged: false,
      sourceLabel: c.provider_name,
      meta: cached,
      loading: false,
      fieldSources: null,
      fillMode: fillMode.value,
    })
    return
  }

  emit('selectPreview', {
    key,
    isMerged: false,
    sourceLabel: c.provider_name,
    meta: buildLightMetadata(c),
    loading: true,
    fieldSources: null,
    fillMode: fillMode.value,
  })

  const full = await resolveCandidateDetail(c)
  if (selectedKey.value !== key || currentEntryKey() !== entryKey) return
  emit('selectPreview', {
    key,
    isMerged: false,
    sourceLabel: c.provider_name,
    meta: full ?? buildLightMetadata(c),
    loading: false,
    fieldSources: null,
    fillMode: fillMode.value,
  })
}

async function loadProvidersForTarget(
  target: ScrapeTargetKind,
  domain: MediaFileDetail['entry']['media_kind'],
): Promise<void> {
  const seq = ++providerLoadSeq
  try {
    const list = await toolApi.listProviders(target, domain)
    if (seq !== providerLoadSeq) return
    providers.value = list
    const configuredIds = list.filter((p) => p.configured).map((p) => p.id)
    const retained = configuredIds.filter((id) => enabledProviderIds.value.includes(id))
    enabledProviderIds.value = retained.length > 0 ? retained : configuredIds
  } catch {
    if (seq !== providerLoadSeq) return
    providers.value = []
  }
}

async function loadProvidersForDetail(d: MediaFileDetail): Promise<void> {
  const target = inferTargetKindFromDetail(d)
  targetKind.value = target
  await loadProvidersForTarget(target, d.entry.media_kind)
}

function toggleProvider(p: ProviderStatusDto): void {
  if (!p.configured) {
    emit('openSettings')
    return
  }
  if (enabledProviderIds.value.includes(p.id)) {
    if (enabledProviderIds.value.length <= 1) return
    enabledProviderIds.value = enabledProviderIds.value.filter((id) => id !== p.id)
  } else {
    const nextSet = new Set([...enabledProviderIds.value, p.id])
    enabledProviderIds.value = providers.value
      .filter((x) => nextSet.has(x.id))
      .map((x) => x.id)
  }
}

function toggleProviderIgnored(providerId: string): void {
  const next = new Set(ignoredProviderIds.value)
  if (next.has(providerId)) next.delete(providerId)
  else next.add(providerId)
  ignoredProviderIds.value = next
}

function clearIgnoredProviders(): void {
  ignoredProviderIds.value = new Set()
}

async function moveActiveProviderOrder(idx: number, delta: -1 | 1): Promise<void> {
  const list = [...enabledProviderIds.value]
  const targetIdx = idx + delta
  if (targetIdx < 0 || targetIdx >= list.length) return
  const [moved] = list.splice(idx, 1)
  list.splice(targetIdx, 0, moved!)
  enabledProviderIds.value = list

  providers.value = [...providers.value].sort((a, b) => {
    const pa = list.indexOf(a.id)
    const pb = list.indexOf(b.id)
    return (pa === -1 ? 999 : pa) - (pb === -1 ? 999 : pb)
  })

  const dom = props.detail?.entry.media_kind
  if (dom && props.settings) {
    const existingDomOrder = props.settings.scraper_priorities[dom] ?? []
    const fullOrder = [...list, ...existingDomOrder.filter((id) => !list.includes(id))]
    try {
      const nextSettings = await toolApi.updateSettings({
        scraper_priorities: {
          ...props.settings.scraper_priorities,
          [dom]: fullOrder,
        },
      })
      emit('settingsUpdated', nextSettings)
    } catch {
      // Non-fatal UI convenience sync
    }
  }
}

function isProviderRepresentative(c: ScrapeCandidate): boolean {
  return selectedByProvider.value[c.provider_id]?.external_id === c.external_id
}

async function useCandidateForMerge(c: ScrapeCandidate): Promise<void> {
  const entryKey = currentEntryKey()
  const full = (await resolveCandidateDetail(c)) ?? buildLightMetadata(c)
  if (currentEntryKey() !== entryKey) return
  selectedByProvider.value = { ...selectedByProvider.value, [c.provider_id]: c }
  detailsByProvider.value = { ...detailsByProvider.value, [c.provider_id]: full }
  if (ignoredProviderIds.value.has(c.provider_id)) {
    const nextIgnored = new Set(ignoredProviderIds.value)
    nextIgnored.delete(c.provider_id)
    ignoredProviderIds.value = nextIgnored
  }
  selectMergedPreview()
  flashFeedback(`已将「${c.title}」设为 ${c.provider_name} 聚合代表`)
}

async function handleDirectApplyCandidate(c: ScrapeCandidate): Promise<void> {
  const entryKey = currentEntryKey()
  const meta = await resolveCandidateDetail(c)
  if (!meta || currentEntryKey() !== entryKey) return
  emit('directApply', meta, fillMode.value)
}

async function runSearch(): Promise<void> {
  if (!props.detail) return
  const kw = keyword.value.trim()
  const isbnTrim = isbn.value.trim()
  if (!kw && !isbnTrim) return
  const reqSeq = ++searchRequestSeq
  searching.value = true
  searchError.value = null
  candidates.value = []
  serverMergedResult.value = null
  selectedByProvider.value = {}
  detailsByProvider.value = {}
  ignoredProviderIds.value = new Set()
  waterfallExpanded.value = false
  selectedKey.value = null
  activeSourceFilter.value = 'all'
  emit('selectPreview', null)

  try {
    const parsedYear = yearInput.value.trim() ? Number.parseInt(yearInput.value.trim(), 10) : undefined
    const parsedSeason = seasonInput.value.trim()
      ? Number.parseInt(seasonInput.value.trim(), 10)
      : undefined
    const parsedEpisode = episodeInput.value.trim()
      ? Number.parseInt(episodeInput.value.trim(), 10)
      : undefined
    const q = {
      ...buildDefaultQueryFromDetail(props.detail),
      target: targetKind.value,
      keyword: kw || isbnTrim,
      artist_or_author: artistOrAuthor.value.trim() || undefined,
      album_or_series: albumOrSeries.value.trim() || undefined,
      isbn: isbnTrim || undefined,
      year: parsedYear && Number.isFinite(parsedYear) ? parsedYear : undefined,
      season: parsedSeason && Number.isFinite(parsedSeason) ? parsedSeason : undefined,
      episode: parsedEpisode && Number.isFinite(parsedEpisode) ? parsedEpisode : undefined,
      duration_secs: durationSecs.value,
      providers: enabledProviderIds.value.length > 0 ? enabledProviderIds.value : undefined,
    }
    const resp: AggregateScrapeResponse = await toolApi.aggregateScrapers(q)
    if (reqSeq !== searchRequestSeq) return
    candidates.value = resp.candidates
    serverMergedResult.value = resp.merged ?? null

    const nextDetails: Record<string, ScrapedMetadata> = {}
    for (const [pid, detail] of Object.entries(resp.details_by_provider ?? {})) {
      if (detail) {
        nextDetails[pid] = detail
        setCachedDetail(`${pid}::${detail.external_id}`, detail)
      }
    }
    detailsByProvider.value = nextDetails

    const nextSelected: Record<string, ScrapeCandidate> = {}
    for (const c of resp.candidates) {
      const detailMatch = nextDetails[c.provider_id]
      if (detailMatch && c.external_id === detailMatch.external_id) {
        nextSelected[c.provider_id] = c
      } else if (!nextSelected[c.provider_id]) {
        nextSelected[c.provider_id] = c
      }
    }
    selectedByProvider.value = nextSelected

    if (mergedMeta.value) {
      selectMergedPreview()
    } else if (candidates.value.length > 0) {
      void selectCandidatePreview(candidates.value[0]!)
    }
  } catch (err) {
    if (reqSeq !== searchRequestSeq) return
    searchError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (reqSeq === searchRequestSeq) searching.value = false
  }
}

watch(
  () => props.selectedPreviewKey,
  (val) => {
    if (val === null) selectedKey.value = null
  },
)

watch(fillMode, () => {
  if (!selectedKey.value) return
  if (selectedKey.value === '__merged__' && mergedMeta.value) {
    selectMergedPreview()
    return
  }
  const hit = candidates.value.find((c) => candKey(c) === selectedKey.value)
  if (hit) {
    const key = candKey(hit)
    emit('selectPreview', {
      key,
      isMerged: false,
      sourceLabel: hit.provider_name,
      meta: getCachedDetail(key) ?? buildLightMetadata(hit),
      loading: loadingDetailKey.value === key,
      fieldSources: null,
      fillMode: fillMode.value,
    })
  }
})

watch(mergedResult, (next) => {
  if (selectedKey.value !== '__merged__') return
  if (next?.metadata) {
    emit('selectPreview', {
      key: '__merged__',
      isMerged: true,
      sourceLabel: '多源瀑布聚合结果',
      meta: next.metadata,
      loading: false,
      fieldSources: next.field_sources,
      fillMode: fillMode.value,
    })
  } else {
    emit('selectPreview', null)
  }
})

watch(
  () => (props.detail ? `${props.detail.entry.root_id}::${props.detail.entry.rel_path}` : ''),
  async (entryKey) => {
    const d = props.detail
    searchRequestSeq += 1
    if (!d) {
      candidates.value = []
      serverMergedResult.value = null
      selectedByProvider.value = {}
      detailsByProvider.value = {}
      ignoredProviderIds.value = new Set()
      waterfallExpanded.value = false
      selectedKey.value = null
      pendingAutoSearch = false
      emit('selectPreview', null)
      return
    }
    hydratingFromDetail = true
    const q = buildDefaultQueryFromDetail(d)
    keyword.value = q.keyword
    artistOrAuthor.value = q.artist_or_author ?? ''
    albumOrSeries.value = q.album_or_series ?? ''
    isbn.value = q.isbn ?? ''
    yearInput.value = q.year ? String(q.year) : ''
    seasonInput.value = q.season !== undefined && q.season !== null ? String(q.season) : ''
    episodeInput.value = q.episode !== undefined && q.episode !== null ? String(q.episode) : ''
    durationSecs.value = q.duration_secs ?? undefined
    candidates.value = []
    serverMergedResult.value = null
    selectedByProvider.value = {}
    detailsByProvider.value = {}
    ignoredProviderIds.value = new Set()
    waterfallExpanded.value = false
    selectedKey.value = null
    searchError.value = null
    emit('selectPreview', null)
    try {
      await loadProvidersForDetail(d)
    } finally {
      if (currentEntryKey() === entryKey) hydratingFromDetail = false
    }
    if (currentEntryKey() === entryKey && pendingAutoSearch) {
      pendingAutoSearch = false
      if (props.externalKeyword !== undefined && props.externalKeyword.trim()) {
        keyword.value = props.externalKeyword.trim()
      }
      if (props.externalArtist !== undefined) {
        artistOrAuthor.value = props.externalArtist.trim()
      }
      void runSearch()
    }
  },
  { immediate: true },
)

watch(targetKind, async (next, prev) => {
  if (hydratingFromDetail || !props.detail) return
  const entryKey = currentEntryKey()
  const d = props.detail
  if (d.metadata.kind === 'music') {
    if (next === 'music_album') {
      const alb = albumOrSeries.value.trim() || d.metadata.data.album.trim()
      if (alb) keyword.value = alb
    } else if (prev === 'music_album') {
      const song = d.metadata.data.title.trim() || d.entry.display_title
      if (song) keyword.value = song
    }
  }
  await loadProvidersForTarget(next, d.entry.media_kind)
  if (currentEntryKey() === entryKey && canSearch.value) {
    void runSearch()
  }
})

watch(
  () => props.autoSearchTrigger,
  (val, prev) => {
    if (val > 0 && val !== prev && props.detail) {
      if (hydratingFromDetail) {
        pendingAutoSearch = true
        return
      }
      if (props.externalKeyword !== undefined && props.externalKeyword.trim()) {
        keyword.value = props.externalKeyword.trim()
      }
      if (props.externalArtist !== undefined) {
        artistOrAuthor.value = props.externalArtist.trim()
      }
      void runSearch()
    }
  },
)

defineExpose({
  runSearch,
})
</script>

<template>
  <aside class="flex h-full min-h-0 flex-col bg-[var(--md-sys-color-surface-container-low)]/60">
    <WorkbenchSearchHeader
      :detail="detail"
      :candidates-count="candidates.length"
      :fill-mode="fillMode"
      :target-kind="targetKind"
      :target-tabs="targetTabs"
      :keyword="keyword"
      :artist-or-author="artistOrAuthor"
      :album-or-series="albumOrSeries"
      :isbn="isbn"
      :year-input="yearInput"
      :season-input="seasonInput"
      :episode-input="episodeInput"
      :can-search="canSearch"
      :searching="searching"
      :providers="providers"
      :enabled-provider-ids="enabledProviderIds"
      :recent-flash="recentFlash"
      :search-error="searchError"
      @update:fill-mode="fillMode = $event"
      @update:target-kind="targetKind = $event"
      @update:keyword="keyword = $event"
      @update:artist-or-author="artistOrAuthor = $event"
      @update:album-or-series="albumOrSeries = $event"
      @update:isbn="isbn = $event"
      @update:year-input="yearInput = $event"
      @update:season-input="seasonInput = $event"
      @update:episode-input="episodeInput = $event"
      @search="runSearch"
      @toggle-provider="toggleProvider"
    />

    <!-- Candidate Body -->
    <div class="min-h-0 flex-1 space-y-2.5 overflow-y-auto p-3">
      <div
        v-if="!detail"
        class="flex h-full flex-col items-center justify-center text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuSparkles class="mb-2 h-6 w-6 opacity-40" />
        <p>在左栏选择一个文件开始刮削</p>
      </div>

      <div
        v-else-if="searching"
        class="flex flex-col items-center justify-center gap-2 py-12 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuLoader2 class="h-5 w-5 animate-spin text-[var(--md-sys-color-primary)]" />
        <span>正在并行查询 {{ enabledProviderIds.length }} 个数据源…</span>
      </div>

      <div
        v-else-if="candidates.length === 0"
        class="flex flex-col items-center justify-center gap-2 py-12 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuSearch class="h-6 w-6 opacity-35" />
        <p>点击上方「搜索」拉取在线候选</p>
        <p class="max-w-[220px] text-[11px] opacity-75">
          点击「多源瀑布聚合结果」或任意候选卡片，在右侧预览编辑并保存
        </p>
        <button
          v-if="configuredProviders.length > 0 && canSearch"
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state mt-1"
          @click="runSearch"
        >
          <LuSearch class="h-3 w-3" />
          <span>立即按当前标题搜索</span>
        </button>
      </div>

      <template v-else>
        <WorkbenchMergedCard
          :merged-meta="mergedMeta"
          :selected="selectedKey === '__merged__'"
          :waterfall-expanded="waterfallExpanded"
          :ordered-active-briefs="orderedActiveBriefs"
          :ignored-provider-ids="ignoredProviderIds"
          :selected-by-provider="selectedByProvider"
          :details-by-provider="detailsByProvider"
          :provider-candidate-counts="providerCandidateCounts"
          :active-source-filter="activeSourceFilter"
          :broken-cover-urls="brokenCoverUrls"
          @select-merged="selectMergedPreview"
          @toggle-waterfall="waterfallExpanded = !waterfallExpanded; selectMergedPreview()"
          @direct-apply="emit('directApply', $event, fillMode)"
          @clear-ignored="clearIgnoredProviders"
          @toggle-ignored="toggleProviderIgnored"
          @move-order="moveActiveProviderOrder"
          @filter-source="activeSourceFilter = $event"
          @mark-cover-broken="markCoverBroken"
        />

        <!-- Source Filter Bar -->
        <div v-if="sourceCounts.length > 1" class="flex flex-wrap gap-1">
          <button
            type="button"
            class="md-state rounded-full px-2.5 py-0.5 text-[10px] font-semibold transition"
            :class="
              activeSourceFilter === 'all'
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
            "
            @click="activeSourceFilter = 'all'"
          >
            全部 ({{ candidates.length }})
          </button>
          <button
            v-for="src in sourceCounts"
            :key="src.id"
            type="button"
            class="md-state rounded-full px-2.5 py-0.5 text-[10px] font-semibold transition"
            :class="
              activeSourceFilter === src.id
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
            "
            @click="activeSourceFilter = src.id"
          >
            {{ src.name }} ({{ src.count }})
          </button>
        </div>

        <WorkbenchCandidateCard
          v-for="c in visibleCandidates"
          :key="candKey(c)"
          :candidate="c"
          :selected="selectedKey === candKey(c)"
          :loading-detail="loadingDetailKey === candKey(c)"
          :is-representative="isProviderRepresentative(c)"
          :provider-candidate-count="providerCandidateCounts[c.provider_id] ?? 0"
          :cover-usable="isCoverUsable(c.cover_url)"
          @select-preview="selectCandidatePreview"
          @use-for-merge="useCandidateForMerge"
          @direct-apply="handleDirectApplyCandidate"
          @mark-cover-broken="markCoverBroken"
        />
      </template>
    </div>
  </aside>
</template>
