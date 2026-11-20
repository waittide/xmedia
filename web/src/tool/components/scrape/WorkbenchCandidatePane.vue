<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuArrowDown,
  LuArrowUp,
  LuBan,
  LuCheck,
  LuImage,
  LuKey,
  LuLayers,
  LuLoader2,
  LuSearch,
  LuSlidersHorizontal,
  LuSparkles,
  LuZap,
} from 'vue-icons-plus/lu'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import { resolveScraperCoverUrl, toolApi } from '../../api'
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

function getCachedDetail(key: string): ScrapedMetadata | undefined {
  const hit = detailCache.get(key)
  if (!hit) return undefined
  // Refresh LRU order in place
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

const fillModeOptions = [
  { value: 'overwrite' as const, label: '覆盖' },
  { value: 'fill_missing' as const, label: '仅补空' },
]

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
  return [{ value: 'book', label: '电子书' }]
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

function buildLightMetadata(c: ScrapeCandidate): ScrapedMetadata {
  return {
    provider_id: c.provider_id,
    external_id: c.external_id,
    target: c.target,
    title: c.title,
    original_title: null,
    series_or_album: null,
    creators: c.subtitle ? [c.subtitle] : [],
    illustrators_or_directors: [],
    cast: [],
    publisher_or_studio: null,
    year: c.year,
    published_date: null,
    language: null,
    isbn: null,
    rating: null,
    track_or_episode_number: null,
    disc_or_season_number: null,
    summary: c.summary,
    genres: [],
    tags: [],
    cover_url: c.cover_url,
    backdrop_url: null,
    synced_lyrics: c.synced_lyrics,
    plain_lyrics: c.plain_lyrics,
  }
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

async function selectCandidatePreview(c: ScrapeCandidate): Promise<void> {
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
  if (selectedKey.value !== key) return
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

watch(
  () => props.selectedPreviewKey,
  (val) => {
    if (val === null) {
      selectedKey.value = null
    }
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
    const cached = getCachedDetail(key)
    emit('selectPreview', {
      key,
      isMerged: false,
      sourceLabel: hit.provider_name,
      meta: cached ?? buildLightMetadata(hit),
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

function flashFeedback(label: string): void {
  recentFlash.value = label
  setTimeout(() => {
    if (recentFlash.value === label) {
      recentFlash.value = null
    }
  }, 1800)
}

function candKey(c: ScrapeCandidate): string {
  return `${c.provider_id}::${c.external_id}`
}

const configuredProviders = computed(() => providers.value.filter((p) => p.configured))

const visibleCandidates = computed(() => {
  if (activeSourceFilter.value === 'all') return candidates.value
  return candidates.value.filter((c) => c.provider_id === activeSourceFilter.value)
})

const sourceCounts = computed(() => {
  const m = new Map<string, { id: string; name: string; count: number }>()
  for (const c of candidates.value) {
    const existing = m.get(c.provider_id)
    if (existing) {
      existing.count += 1
    } else {
      m.set(c.provider_id, { id: c.provider_id, name: c.provider_name, count: 1 })
    }
  }
  return Array.from(m.values())
})

async function loadProvidersForTarget(
  target: ScrapeTargetKind,
  domain: MediaFileDetail['entry']['media_kind'],
): Promise<void> {
  try {
    const list = await toolApi.listProviders(target, domain)
    providers.value = list
    const configuredIds = list.filter((p) => p.configured).map((p) => p.id)
    const retained = configuredIds.filter((id) => enabledProviderIds.value.includes(id))
    enabledProviderIds.value = retained.length > 0 ? retained : configuredIds
  } catch {
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
  if (next.has(providerId)) {
    next.delete(providerId)
  } else {
    next.add(providerId)
  }
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

  const reorderedProviders = [...providers.value].sort((a, b) => {
    const pa = list.indexOf(a.id)
    const pb = list.indexOf(b.id)
    return (pa === -1 ? 999 : pa) - (pb === -1 ? 999 : pb)
  })
  providers.value = reorderedProviders

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
  const full = (await resolveCandidateDetail(c)) ?? buildLightMetadata(c)
  selectedByProvider.value = {
    ...selectedByProvider.value,
    [c.provider_id]: c,
  }
  detailsByProvider.value = {
    ...detailsByProvider.value,
    [c.provider_id]: full,
  }
  if (ignoredProviderIds.value.has(c.provider_id)) {
    const nextIgnored = new Set(ignoredProviderIds.value)
    nextIgnored.delete(c.provider_id)
    ignoredProviderIds.value = nextIgnored
  }
  selectMergedPreview()
  flashFeedback(`已将「${c.title}」设为 ${c.provider_name} 聚合代表`)
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
    if (reqSeq === searchRequestSeq) {
      searching.value = false
    }
  }
}

const CN_NOVEL_PROVIDER_IDS = new Set([
  'qidian',
  'fanqie',
  'qimao',
  'zongheng',
  'jjwxc',
  'sfacg',
  'ciweimao',
  'faloo',
])
const MUSIC_PROVIDER_IDS = new Set([
  'qq_music',
  'netease',
  'kugou',
  'kuwo',
  'taihe',
  'qishui',
  'musicbrainz',
  'itunes',
  'lrclib',
])

function buildCandidateFallbackMeta(c: ScrapeCandidate): ScrapedMetadata {
  const creators: string[] = []
  const genres: string[] = []
  let seriesOrAlbum: string | null = null
  let publisherOrStudio: string | null = null
  let language: string | null = null

  const sub = (c.subtitle ?? '').trim()
  if (sub) {
    const parts = sub
      .split(' · ')
      .map((s) => s.trim())
      .filter(Boolean)
    if (CN_NOVEL_PROVIDER_IDS.has(c.provider_id)) {
      if (parts[0]) creators.push(parts[0])
      if (parts[1]) genres.push(parts[1])
    } else if (MUSIC_PROVIDER_IDS.has(c.provider_id)) {
      if (parts[0]) {
        for (const a of parts[0]
          .split(/[/、]/)
          .map((s) => s.trim())
          .filter(Boolean)) {
          if (!creators.includes(a)) creators.push(a)
        }
      }
      if (parts[1]) seriesOrAlbum = parts[1]
    } else if (c.target === 'book' || c.target === 'comic') {
      if (parts[0]) {
        for (const a of parts[0]
          .split(' / ')
          .map((s) => s.trim())
          .filter(Boolean)) {
          if (!creators.includes(a)) creators.push(a)
        }
      }
      if (parts[1]) publisherOrStudio = parts[1]
    }
  }

  if (CN_NOVEL_PROVIDER_IDS.has(c.provider_id)) {
    if (!publisherOrStudio && c.provider_name.trim()) {
      publisherOrStudio = c.provider_name.trim()
    }
    language = 'zh-CN'
  }

  return {
    provider_id: c.provider_id,
    external_id: c.external_id,
    target: c.target,
    title: c.title || null,
    original_title: null,
    series_or_album: seriesOrAlbum,
    creators,
    illustrators_or_directors: [],
    cast: [],
    publisher_or_studio: publisherOrStudio,
    year: c.year ?? null,
    published_date: null,
    language,
    isbn: null,
    rating: null,
    track_or_episode_number: null,
    disc_or_season_number: null,
    summary: c.summary ?? null,
    genres,
    tags: [],
    cover_url: c.cover_url ?? null,
    backdrop_url: null,
    synced_lyrics: c.synced_lyrics ?? null,
    plain_lyrics: c.plain_lyrics ?? null,
  }
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
    if (loadingDetailKey.value === key) {
      loadingDetailKey.value = null
    }
  }
}

async function handleDirectApplyCandidate(c: ScrapeCandidate): Promise<void> {
  const meta = await resolveCandidateDetail(c)
  if (!meta) return
  emit('directApply', meta, fillMode.value)
}

watch(
  () => (props.detail ? `${props.detail.entry.root_id}::${props.detail.entry.rel_path}` : ''),
  async () => {
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
    await loadProvidersForDetail(d)
    hydratingFromDetail = false
  },
  { immediate: true },
)

watch(targetKind, async (next, prev) => {
  if (hydratingFromDetail || !props.detail) return
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
  if (canSearch.value) {
    void runSearch()
  }
})

watch(
  () => props.autoSearchTrigger,
  (val, prev) => {
    if (val > 0 && val !== prev && props.detail) {
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

function countPopulatedFields(meta: ScrapedMetadata): number {
  let count = 0
  if (meta.title) count += 1
  if (meta.original_title && meta.original_title !== meta.title) count += 1
  if (meta.creators.length > 0) count += 1
  if (meta.illustrators_or_directors.length > 0) count += 1
  if (meta.series_or_album) count += 1
  if (meta.publisher_or_studio) count += 1
  if (meta.published_date || meta.year) count += 1
  if (meta.isbn) count += 1
  if (meta.rating) count += 1
  if (meta.language) count += 1
  if (meta.disc_or_season_number) count += 1
  if (meta.track_or_episode_number) count += 1
  if (meta.cast.length > 0) count += 1
  if (meta.genres.length > 0) count += 1
  if (meta.tags.length > 0) count += 1
  if (meta.summary) count += 1
  if (meta.synced_lyrics || meta.plain_lyrics) count += 1
  return count
}

defineExpose({
  runSearch,
})
</script>

<template>
  <aside
    class="flex h-full min-h-0 flex-col bg-[var(--md-sys-color-surface-container-low)]/60"
  >
    <!-- Header & Search Bar -->
    <div class="shrink-0 space-y-2.5 border-b border-[var(--md-sys-color-outline-variant)]/50 p-3">
      <div class="flex items-center justify-between gap-2">
        <div class="flex items-center gap-1.5 text-xs font-bold text-[var(--md-sys-color-on-surface)]">
          <LuSparkles class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
          <span>在线候选源</span>
          <span
            v-if="candidates.length > 0"
            class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ candidates.length }}
          </span>
        </div>

        <!-- Fill Mode Segmented Switcher -->
        <MdSegmentedButton
          v-model="fillMode"
          :options="fillModeOptions"
          size="sm"
        />
      </div>

      <!-- Target Sub-Kind Switcher (Music / Video / Comic) -->
      <MdSegmentedButton
        v-if="detail && targetTabs.length > 1"
        v-model="targetKind"
        :options="targetTabs"
        size="sm"
      />

      <!-- Search Inputs -->
      <div class="flex gap-1.5">
        <input
          v-model="keyword"
          type="text"
          :disabled="!detail"
          placeholder="标题 / 关键词 / ID…"
          class="md-input h-8 min-w-0 flex-1 text-xs disabled:opacity-50"
          @keydown.enter.prevent="runSearch"
        />
        <button
          type="button"
          :disabled="!canSearch || searching"
          class="md-btn md-btn-sm md-btn-filled md-state shrink-0"
          @click="runSearch"
        >
          <LuLoader2 v-if="searching" class="h-3.5 w-3.5 animate-spin" />
          <LuSearch v-else class="h-3.5 w-3.5" />
          <span>搜索</span>
        </button>
      </div>

      <!-- Domain-Specific Secondary Query Inputs -->
      <div v-if="detail?.entry.media_kind === 'book'" class="grid grid-cols-2 gap-1.5">
        <input
          v-model="artistOrAuthor"
          type="text"
          placeholder="作者 (可选)"
          class="md-input h-7 !rounded-lg !px-2 text-[11px]"
          @keydown.enter.prevent="runSearch"
        />
        <input
          v-model="isbn"
          type="text"
          placeholder="ISBN (可选)"
          class="md-input h-7 !rounded-lg !px-2 text-[11px]"
          @keydown.enter.prevent="runSearch"
        />
      </div>

      <div v-else-if="detail?.entry.media_kind === 'music'" class="grid grid-cols-5 gap-1.5">
        <input
          v-model="artistOrAuthor"
          type="text"
          placeholder="歌手 (可选)"
          class="md-input col-span-2 h-7 !rounded-lg !px-2 text-[11px]"
          @keydown.enter.prevent="runSearch"
        />
        <input
          v-model="albumOrSeries"
          type="text"
          placeholder="专辑 (可选)"
          class="md-input col-span-2 h-7 !rounded-lg !px-2 text-[11px]"
          @keydown.enter.prevent="runSearch"
        />
        <input
          v-model="yearInput"
          type="text"
          placeholder="年份"
          class="md-input h-7 !rounded-lg !px-2 text-[11px]"
          @keydown.enter.prevent="runSearch"
        />
      </div>

      <div
        v-else-if="detail?.entry.media_kind === 'video' && targetKind === 'tv_show'"
        class="grid grid-cols-3 gap-1.5"
      >
        <input
          v-model="seasonInput"
          type="text"
          placeholder="季 (S)"
          class="md-input h-7 !rounded-lg !px-2 text-[11px]"
          @keydown.enter.prevent="runSearch"
        />
        <input
          v-model="episodeInput"
          type="text"
          placeholder="集 (E)"
          class="md-input h-7 !rounded-lg !px-2 text-[11px]"
          @keydown.enter.prevent="runSearch"
        />
        <input
          v-model="yearInput"
          type="text"
          placeholder="年份"
          class="md-input h-7 !rounded-lg !px-2 text-[11px]"
          @keydown.enter.prevent="runSearch"
        />
      </div>

      <div v-else-if="detail?.entry.media_kind === 'video'" class="grid grid-cols-2 gap-1.5">
        <input
          v-model="yearInput"
          type="text"
          placeholder="上映年份 (可选)"
          class="md-input h-7 !rounded-lg !px-2 text-[11px]"
          @keydown.enter.prevent="runSearch"
        />
      </div>

      <div v-else class="grid grid-cols-3 gap-1.5">
        <input
          v-model="artistOrAuthor"
          type="text"
          :disabled="!detail"
          placeholder="作者/画师（可选）"
          class="md-input col-span-2 h-7 !rounded-lg !px-2 text-[11px] disabled:opacity-50"
          @keydown.enter.prevent="runSearch"
        />
        <input
          v-model="yearInput"
          type="text"
          :disabled="!detail"
          placeholder="年份"
          class="md-input h-7 !rounded-lg !px-2 text-[11px] disabled:opacity-50"
          @keydown.enter.prevent="runSearch"
        />
      </div>

      <!-- Provider Toggle Chips -->
      <div v-if="providers.length > 0" class="flex flex-wrap gap-1">
        <button
          v-for="p in providers"
          :key="p.id"
          type="button"
          :title="p.description"
          class="md-state inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-medium transition"
          :class="
            !p.configured
              ? 'border-dashed border-[var(--md-sys-color-warning)]/55 bg-[var(--md-sys-color-warning-container)]/50 text-[var(--md-sys-color-on-warning-container)]'
              : enabledProviderIds.includes(p.id)
                ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                : 'border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface-variant)]'
          "
          @click="toggleProvider(p)"
        >
          <span
            v-if="p.configured && enabledProviderIds.includes(p.id)"
            class="font-mono text-[9px] font-bold opacity-80"
          >
            #{{ enabledProviderIds.indexOf(p.id) + 1 }}
          </span>
          <LuKey v-else-if="!p.configured && p.requires_api_key" class="h-2.5 w-2.5" />
          <span>{{ p.name }}</span>
        </button>
      </div>

      <!-- Flash Feedback -->
      <div
        v-if="recentFlash"
        class="flex items-center gap-1.5 rounded-xl bg-[var(--md-sys-color-success-container)] px-2.5 py-1 text-[11px] font-medium text-[var(--md-sys-color-on-success-container)]"
      >
        <LuCheck class="h-3 w-3 shrink-0" />
        <span class="truncate">{{ recentFlash }}</span>
      </div>
      <div
        v-if="searchError"
        class="rounded-xl bg-[var(--md-sys-color-error-container)] px-2.5 py-1.5 text-[11px] text-[var(--md-sys-color-on-error-container)]"
      >
        {{ searchError }}
      </div>
    </div>

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
        <!-- All-Muted Fallback Banner -->
        <div
          v-if="!mergedMeta && ignoredProviderIds.size > 0"
          class="flex items-center justify-between gap-2 rounded-2xl bg-[var(--md-sys-color-warning-container)] px-3 py-2 text-[11px] text-[var(--md-sys-color-on-warning-container)]"
        >
          <span>已临时屏蔽全部命中源（{{ ignoredProviderIds.size }} 个）</span>
          <button
            type="button"
            class="md-state rounded-full border border-current/35 px-2 py-0.5 text-[10px] font-semibold"
            @click="clearIgnoredProviders"
          >
            恢复全部源
          </button>
        </div>

        <!-- Multi-source Merged Quick Card -->
        <div
          v-if="mergedMeta"
          class="cursor-pointer rounded-2xl border p-2.5 transition"
          :class="
            selectedKey === '__merged__'
              ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)]/50 ring-1 ring-[var(--md-sys-color-primary)]/45'
              : 'border-[var(--md-sys-color-primary)]/40 bg-[var(--md-sys-color-primary-container)]/25 hover:border-[var(--md-sys-color-primary)]/75'
          "
          @click="selectMergedPreview"
        >
          <div class="flex items-center justify-between gap-2">
            <div class="flex items-center gap-1.5 text-[11px] font-bold text-[var(--md-sys-color-primary)]">
              <LuLayers class="h-3.5 w-3.5" />
              <span>多源瀑布聚合结果</span>
              <span
                class="rounded-full bg-[var(--md-sys-color-surface)]/85 px-1.5 py-0.5 text-[9px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ countPopulatedFields(mergedMeta) }} 项字段
              </span>
            </div>
            <span
              v-if="selectedKey === '__merged__'"
              class="rounded-full bg-[var(--md-sys-color-primary)] px-1.5 py-0.5 text-[9px] font-semibold text-[var(--md-sys-color-on-primary)]"
            >
              预览中
            </span>
          </div>

          <div class="mt-2 flex items-start gap-2.5">
            <!-- Merged Cover Thumbnail -->
            <button
              type="button"
              class="relative h-14 w-10 shrink-0 overflow-hidden rounded-lg border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface)]"
              title="在右侧预览多源聚合结果"
              @click.stop="selectMergedPreview()"
            >
              <img
                v-if="isCoverUsable(mergedMeta.cover_url)"
                :src="resolveScraperCoverUrl(mergedMeta.cover_url)"
                alt=""
                loading="lazy"
                referrerpolicy="no-referrer"
                class="h-full w-full object-cover"
                @error="markCoverBroken(mergedMeta.cover_url)"
              />
              <div
                v-else
                class="flex h-full w-full flex-col items-center justify-center gap-0.5 text-[var(--md-sys-color-on-surface-variant)]/55"
              >
                <LuImage class="h-3.5 w-3.5" />
                <span class="text-[8px]">无图</span>
              </div>
            </button>

            <div class="min-w-0 flex-1">
              <div class="line-clamp-2 text-xs font-bold text-[var(--md-sys-color-on-surface)]">
                {{ mergedMeta.title || '—' }}
              </div>
              <div class="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                <span v-if="mergedMeta.creators.length > 0" class="truncate">
                  {{ mergedMeta.creators.join(' / ') }}
                </span>
                <span v-if="mergedMeta.publisher_or_studio" class="truncate opacity-80">
                  {{ mergedMeta.publisher_or_studio }}
                </span>
                <span v-if="mergedMeta.published_date || mergedMeta.year" class="font-mono">
                  {{ mergedMeta.published_date || mergedMeta.year }}
                </span>
              </div>

              <!-- Action Bar -->
              <div class="mt-2 flex flex-wrap items-center gap-1.5">
                <button
                  type="button"
                  class="md-state inline-flex items-center gap-1 rounded-full border px-2.5 py-0.5 text-[10px] font-medium transition"
                  :class="
                    waterfallExpanded || ignoredProviderIds.size > 0
                      ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                      : 'border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
                  "
                  title="调整各源优先级、临时屏蔽干扰源或切换单源聚合代表"
                  @click.stop="waterfallExpanded = !waterfallExpanded; selectMergedPreview()"
                >
                  <LuSlidersHorizontal class="h-3 w-3" />
                  <span>
                    调源{{ ignoredProviderIds.size > 0 ? ` (-${ignoredProviderIds.size})` : '' }}
                  </span>
                </button>

                <button
                  type="button"
                  class="md-state inline-flex items-center gap-1 rounded-full border border-[var(--md-sys-color-primary)]/55 bg-[var(--md-sys-color-surface)] px-2.5 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-primary)] transition hover:bg-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-on-primary)]"
                  title="直接将聚合结果写入文件并保存"
                  @click.stop="emit('directApply', mergedMeta, fillMode)"
                >
                  <LuZap class="h-3 w-3" />
                  <span>直写</span>
                </button>
              </div>
            </div>
          </div>

          <!-- Interactive Multi-Source Waterfall Priority & Mute Drawer -->
          <div
            v-if="waterfallExpanded"
            class="mt-2.5 space-y-1.5 border-t border-[var(--md-sys-color-primary)]/25 pt-2"
            @click.stop
          >
            <div class="flex items-center justify-between text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
              <span>按顺序互补填充，可上下调权、屏蔽干扰源或换选候选：</span>
              <button
                v-if="ignoredProviderIds.size > 0"
                type="button"
                class="shrink-0 font-semibold text-[var(--md-sys-color-primary)] hover:underline"
                @click="clearIgnoredProviders"
              >
                全部恢复
              </button>
            </div>
            <div class="space-y-1">
              <div
                v-for="(brief, idx) in orderedActiveBriefs"
                :key="brief.id"
                class="flex items-center justify-between gap-1.5 rounded-xl border px-2 py-1.5 text-[10px] transition"
                :class="
                  ignoredProviderIds.has(brief.id)
                    ? 'border-dashed border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] opacity-55'
                    : 'border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface)]'
                "
              >
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-1">
                    <span class="font-mono text-[9px] font-bold text-[var(--md-sys-color-primary)]">
                      #{{ idx + 1 }}
                    </span>
                    <span
                      class="font-semibold text-[var(--md-sys-color-on-surface)]"
                      :class="ignoredProviderIds.has(brief.id) ? 'line-through' : ''"
                    >
                      {{ brief.name }}
                    </span>
                  </div>
                  <div class="truncate text-[9px] text-[var(--md-sys-color-on-surface-variant)]">
                    {{
                      selectedByProvider[brief.id]?.title ||
                      detailsByProvider[brief.id]?.title ||
                      '未命中'
                    }}
                  </div>
                </div>

                <div class="flex shrink-0 items-center gap-0.5">
                  <button
                    v-if="(providerCandidateCounts[brief.id] ?? 0) > 1"
                    type="button"
                    class="rounded-full border border-[var(--md-sys-color-outline-variant)] px-1.5 py-0.5 text-[9px] font-medium text-[var(--md-sys-color-on-surface-variant)] hover:border-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-primary)]"
                    title="筛选此源下的所有候选以便换选聚合条目"
                    @click="activeSourceFilter = activeSourceFilter === brief.id ? 'all' : brief.id"
                  >
                    换选({{ providerCandidateCounts[brief.id] }})
                  </button>
                  <button
                    type="button"
                    class="rounded-full p-1 transition"
                    :class="
                      ignoredProviderIds.has(brief.id)
                        ? 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
                        : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]'
                    "
                    :title="ignoredProviderIds.has(brief.id) ? '恢复此源参与聚合' : '临时屏蔽此源'"
                    @click="toggleProviderIgnored(brief.id)"
                  >
                    <LuBan class="h-3 w-3" />
                  </button>
                  <button
                    type="button"
                    :disabled="idx === 0"
                    class="rounded-full p-1 text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)] disabled:opacity-30"
                    title="提升优先级"
                    @click="moveActiveProviderOrder(idx, -1)"
                  >
                    <LuArrowUp class="h-3 w-3" />
                  </button>
                  <button
                    type="button"
                    :disabled="idx === orderedActiveBriefs.length - 1"
                    class="rounded-full p-1 text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)] disabled:opacity-30"
                    title="降低优先级"
                    @click="moveActiveProviderOrder(idx, 1)"
                  >
                    <LuArrowDown class="h-3 w-3" />
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>

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

        <!-- Candidate Cards -->
        <div
          v-for="c in visibleCandidates"
          :key="candKey(c)"
          class="cursor-pointer rounded-2xl border p-2.5 transition"
          :class="
            selectedKey === candKey(c)
              ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)]/35 ring-1 ring-[var(--md-sys-color-primary)]/40'
              : 'border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface)] hover:border-[var(--md-sys-color-primary)]/50'
          "
          @click="selectCandidatePreview(c)"
        >
          <div class="flex items-start gap-2.5">
            <!-- Cover Thumbnail -->
            <button
              type="button"
              class="relative h-14 w-10 shrink-0 overflow-hidden rounded-lg border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-high)]"
              title="点击在右侧预览此候选详情"
              @click.stop="selectCandidatePreview(c)"
            >
              <img
                v-if="isCoverUsable(c.cover_url)"
                :src="resolveScraperCoverUrl(c.cover_url)"
                alt=""
                loading="lazy"
                referrerpolicy="no-referrer"
                class="h-full w-full object-cover"
                @error="markCoverBroken(c.cover_url)"
              />
              <div
                v-else
                class="flex h-full w-full flex-col items-center justify-center gap-0.5 text-[var(--md-sys-color-on-surface-variant)]/50"
              >
                <LuImage class="h-3.5 w-3.5" />
                <span class="text-[8px]">无图</span>
              </div>
            </button>

            <!-- Candidate Info -->
            <div class="min-w-0 flex-1">
              <div class="flex items-start justify-between gap-1.5">
                <button
                  type="button"
                  class="line-clamp-2 text-left text-xs font-bold text-[var(--md-sys-color-on-surface)] transition hover:text-[var(--md-sys-color-primary)]"
                  title="点击在右侧预览此条目详情"
                  @click.stop="selectCandidatePreview(c)"
                >
                  {{ c.title }}
                </button>
                <span
                  class="shrink-0 rounded-full px-2 py-0.5 text-[9px] font-semibold"
                  :class="
                    selectedKey === candKey(c)
                      ? 'bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]'
                      : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
                  "
                >
                  {{ c.provider_name }}
                </span>
              </div>

              <div class="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                <span v-if="c.subtitle" class="truncate">{{ c.subtitle }}</span>
                <span v-if="c.year" class="font-mono">{{ c.year }}</span>
              </div>

              <!-- Quick Action Buttons -->
              <div class="mt-2 flex flex-wrap items-center gap-1.5">
                <button
                  v-if="!isProviderRepresentative(c)"
                  type="button"
                  :disabled="loadingDetailKey === candKey(c)"
                  class="md-state inline-flex items-center gap-1 rounded-full border border-[var(--md-sys-color-primary)]/45 bg-[var(--md-sys-color-surface)] px-2.5 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-primary)] transition hover:bg-[var(--md-sys-color-primary-container)] disabled:opacity-50"
                  title="将此候选设为该数据源的聚合代表，并实时重算多源瀑布聚合结果"
                  @click.stop="useCandidateForMerge(c)"
                >
                  <LuLayers class="h-3 w-3" />
                  <span>用于聚合</span>
                </button>
                <span
                  v-else-if="(providerCandidateCounts[c.provider_id] ?? 0) > 1"
                  class="inline-flex items-center gap-0.5 rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[9px] font-semibold text-[var(--md-sys-color-on-primary-container)]"
                  title="当前已作为该数据源的聚合代表"
                >
                  <LuCheck class="h-2.5 w-2.5" />
                  <span>聚合中</span>
                </span>

                <button
                  type="button"
                  :disabled="loadingDetailKey === candKey(c)"
                  class="md-state inline-flex items-center gap-1 rounded-full border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface-container-low)] px-2.5 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)] transition hover:border-[var(--md-sys-color-success)] hover:text-[var(--md-sys-color-success)] disabled:opacity-50"
                  title="直接写入磁盘（含封面），无需再点保存"
                  @click.stop="handleDirectApplyCandidate(c)"
                >
                  <LuZap class="h-3 w-3" />
                  <span>直写</span>
                </button>
              </div>
            </div>
          </div>
        </div>
      </template>
    </div>
  </aside>
</template>
