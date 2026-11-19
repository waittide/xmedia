<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuChevronLeft,
  LuChevronRight,
  LuExternalLink,
  LuKey,
  LuLock,
  LuRefreshCw,
  LuSearch,
  LuSparkles,
  LuX,
  LuZap,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import ScrapePreviewPane from './scrape/ScrapePreviewPane.vue'
import ScrapeSourcePane from './scrape/ScrapeSourcePane.vue'
import { toolApi } from '../api'
import { mergeScrapedClientSide } from '../merger'
import { useToolTaskStore } from '../tasks'
import type {
  FieldSourceBadge,
  MediaFileDetail,
  MediaFileEntry,
  MergedScrapedResult,
  ProviderBriefDto,
  ProviderStatusDto,
  PublicSettingsDto,
  ScrapeCandidate,
  ScrapeQuery,
  ScrapeTargetKind,
  ScrapedMetadata,
} from '../types'

const props = defineProps<{
  open: boolean
  detail: MediaFileDetail | null
  settings: PublicSettingsDto | null
  queue?: MediaFileEntry[]
  queueIndex?: number
}>()

const emit = defineEmits<{
  close: []
  applied: [detail: MediaFileDetail]
  batchApplied: [
    lastDetail: MediaFileDetail | null,
    summary?: { success: number; failed: number },
  ]
  selectQueueIndex: [index: number]
  openSettings: []
  settingsUpdated: [settings: PublicSettingsDto]
}>()

const targetKind = ref<ScrapeTargetKind>('music_track')
const providers = ref<ProviderStatusDto[]>([])
const enabledProviderIds = ref<string[]>([])
const ignoredProviderIds = ref<Set<string>>(new Set())

const keyword = ref('')
const artistOrAuthor = ref('')
const albumOrSeries = ref('')
const isbn = ref('')
const yearInput = ref('')
const season = ref('')
const episode = ref('')
const durationSecs = ref<number | undefined>(undefined)

const searching = ref(false)
const candidates = ref<ScrapeCandidate[]>([])
const activeSourceTab = ref<'merged' | string>('merged')
const previewMode = ref<'merged' | 'single'>('merged')
const brokenCoverUrls = ref<Set<string>>(new Set())
const disabledFields = ref<Set<string>>(new Set())

// Per-provider selected candidate & fetched detail for reactive waterfall merging
const selectedByProvider = ref<Record<string, ScrapeCandidate>>({})
const detailsByProvider = ref<Record<string, ScrapedMetadata>>({})
const loadingProviderId = ref<string | null>(null)

const applyCover = ref(true)
const writeAlbumCover = ref(false)
const organizeByTemplate = ref(false)
const onlyFillMissing = ref(false)
const writeSidecarLrc = ref(true)
const taskStore = useToolTaskStore()
const applying = ref(false)
const batchProgress = ref<string | null>(null)
const batchProgressPct = ref<number | null>(null)
const errorMsg = ref<string | null>(null)

let hydratingFromDetail = false
let queueRequestSeq = 0
let searchRequestSeq = 0

const queueList = computed<MediaFileEntry[]>(() => props.queue ?? [])
const currentQueueIdx = computed<number>(() => props.queueIndex ?? 0)
const hasQueue = computed<boolean>(() => queueList.value.length > 1)
const hasNextInQueue = computed<boolean>(
  () => hasQueue.value && currentQueueIdx.value < queueList.value.length - 1,
)

const targetTabs = computed<Array<{ value: ScrapeTargetKind; label: string }>>(() => {
  const kind = props.detail?.entry.media_kind
  if (kind === 'music') {
    return [
      { value: 'music_track', label: '单曲多源刮削 (含封面+歌词)' },
      { value: 'music_album', label: '整张专辑' },
      { value: 'lyrics', label: '仅检索歌词' },
    ]
  }
  if (kind === 'video') {
    return [
      { value: 'tv_show', label: '剧集 / 番剧 (含 SxxExx)' },
      { value: 'movie', label: '电影' },
    ]
  }
  if (kind === 'comic') {
    return [
      { value: 'comic', label: '漫画系列 / 单行本' },
      { value: 'book', label: '轻小说 / 图书源' },
    ]
  }
  return [{ value: 'book', label: '电子书 / 出版物' }]
})

const orderedActiveBriefs = computed<ProviderBriefDto[]>(() =>
  enabledProviderIds.value
    .map((id) => providers.value.find((p) => p.id === id && p.configured))
    .filter((p): p is ProviderStatusDto => Boolean(p))
    .map((p) => ({ id: p.id, name: p.name })),
)

const mergedResult = computed<MergedScrapedResult | null>(() =>
  mergeScrapedClientSide(
    orderedActiveBriefs.value,
    detailsByProvider.value,
    targetKind.value,
    brokenCoverUrls.value,
    ignoredProviderIds.value,
    {
      keyword: keyword.value,
      author: artistOrAuthor.value,
      isbn: isbn.value,
    },
  ),
)

const activeSingleProviderDetail = computed<ScrapedMetadata | null>(() => {
  if (activeSourceTab.value === 'merged') return null
  return detailsByProvider.value[activeSourceTab.value] ?? null
})

const activeSingleBadge = computed<FieldSourceBadge | null>(() => {
  if (activeSourceTab.value === 'merged') return null
  const p = providers.value.find((x) => x.id === activeSourceTab.value)
  return p ? { provider_id: p.id, provider_name: p.name } : null
})

// Metadata & provenance map currently displayed in the right pane
const effectiveView = computed<{
  metadata: ScrapedMetadata
  sources: Record<string, FieldSourceBadge>
} | null>(() => {
  if (activeSourceTab.value !== 'merged' && previewMode.value === 'single') {
    const single = activeSingleProviderDetail.value
    if (!single) return null
    const badge = activeSingleBadge.value ?? {
      provider_id: single.provider_id,
      provider_name: single.provider_id,
    }
    return {
      metadata: single,
      sources: {
        title: badge,
        original_title: badge,
        series_or_album: badge,
        creators: badge,
        illustrators_or_directors: badge,
        cast: badge,
        publisher_or_studio: badge,
        year: badge,
        isbn: badge,
        rating: badge,
        track_or_episode_number: badge,
        disc_or_season_number: badge,
        summary: badge,
        show_summary: badge,
        season_title: badge,
        season_summary: badge,
        genres: badge,
        tags: badge,
        cover_url: badge,
        backdrop_url: badge,
        season_poster_url: badge,
        episode_still_url: badge,
        lyrics: badge,
      },
    }
  }
  if (!mergedResult.value) return null
  return {
    metadata: mergedResult.value.metadata,
    sources: mergedResult.value.field_sources,
  }
})

const sourceTabs = computed<Array<{ id: string; label: string; count: number }>>(() => {
  const counts = new Map<string, number>()
  for (const c of candidates.value) {
    counts.set(c.provider_id, (counts.get(c.provider_id) ?? 0) + 1)
  }
  const tabs: Array<{ id: string; label: string; count: number }> = [
    {
      id: 'merged',
      label: '✦ 多源聚合',
      count: Object.keys(detailsByProvider.value).length,
    },
  ]
  for (const brief of orderedActiveBriefs.value) {
    tabs.push({
      id: brief.id,
      label: brief.name,
      count: counts.get(brief.id) ?? 0,
    })
  }
  return tabs
})

const candidatesForActiveSource = computed<ScrapeCandidate[]>(() => {
  if (activeSourceTab.value === 'merged') return []
  return candidates.value.filter((c) => c.provider_id === activeSourceTab.value)
})

async function refreshProvidersForTarget(target: ScrapeTargetKind): Promise<void> {
  try {
    const domain = props.detail?.entry.media_kind
    const list = await toolApi.listProviders(target, domain)
    providers.value = list
    // Returned `list` is already ordered by user's `scraper_priorities` on the backend!
    enabledProviderIds.value = list.filter((p) => p.configured).map((p) => p.id)
  } catch {
    providers.value = []
  }
}

function stripChapterSuffix(raw: string): string {
  return raw
    .replace(/^(?:\[[^\]]*\]|\([^)]*\)|\s)+/, '')
    .replace(
      /[\s_.-]*(?:第\s*\d+(?:\.\d+)?\s*[话話回章集卷册]|(?:vol|ch|chapter)\.?\s*\d+(?:\.\d+)?).*$/i,
      '',
    )
    .replace(/_/g, ' ')
    .trim()
}

function stripYearAndNoiseSuffix(raw: string): string {
  return raw
    .replace(/\s*[\(\[](?:19\d{2}|20\d{2})[\)\]]\s*$/, '')
    .trim()
}

function buildAutoQueryFromDetail(d: MediaFileDetail): ScrapeQuery {
  const m = d.metadata
  if (m.kind === 'music') {
    return {
      target: 'music_track',
      keyword: m.data.title || d.entry.display_title,
      artist_or_author: m.data.artist || undefined,
      album_or_series: m.data.album || undefined,
      year: m.data.year ?? undefined,
      duration_secs: m.data.duration_secs > 0 ? m.data.duration_secs : undefined,
    }
  }
  if (m.kind === 'video') {
    const rawKw = (m.data.is_episode && m.data.show_title) || m.data.title || d.entry.display_title
    return {
      target: m.data.is_episode ? 'tv_show' : 'movie',
      keyword: stripYearAndNoiseSuffix(rawKw),
      year: m.data.year ?? undefined,
      season: m.data.season_number ?? undefined,
      episode: m.data.episode_number ?? undefined,
      duration_secs: m.data.duration_secs > 0 ? m.data.duration_secs : undefined,
    }
  }
  if (m.kind === 'comic') {
    const fallbackClean = stripChapterSuffix(m.data.title || d.entry.display_title)
    return {
      target: 'comic',
      keyword: m.data.series || fallbackClean || m.data.title || d.entry.display_title,
      artist_or_author: m.data.writers[0] || undefined,
      year: m.data.year ?? undefined,
    }
  }
  return {
    target: 'book',
    keyword: m.data.title || d.entry.display_title,
    artist_or_author: m.data.authors[0] || undefined,
    isbn: m.data.isbn || undefined,
  }
}

function seedFromDetail(d: MediaFileDetail): void {
  errorMsg.value = null
  candidates.value = []
  selectedByProvider.value = {}
  detailsByProvider.value = {}
  ignoredProviderIds.value = new Set()
  disabledFields.value = new Set()
  activeSourceTab.value = 'merged'
  previewMode.value = 'merged'
  artistOrAuthor.value = ''
  albumOrSeries.value = ''
  isbn.value = ''
  yearInput.value = ''
  season.value = ''
  episode.value = ''

  const q = buildAutoQueryFromDetail(d)
  hydratingFromDetail = true
  targetKind.value = q.target ?? 'book'
  keyword.value = q.keyword
  artistOrAuthor.value = q.artist_or_author ?? ''
  albumOrSeries.value = q.album_or_series ?? ''
  isbn.value = q.isbn ?? ''
  yearInput.value = q.year ? String(q.year) : ''
  season.value = q.season !== undefined && q.season !== null ? String(q.season) : ''
  episode.value = q.episode !== undefined && q.episode !== null ? String(q.episode) : ''
  durationSecs.value = q.duration_secs ?? undefined
  applyCover.value = true
  writeAlbumCover.value = false
  organizeByTemplate.value = true
  onlyFillMissing.value = false
}

watch(
  () => [props.open, props.detail] as const,
  async ([isOpen, d]) => {
    if (!isOpen || !d) return
    const reqToken = ++queueRequestSeq
    seedFromDetail(d)
    await refreshProvidersForTarget(targetKind.value)
    hydratingFromDetail = false
    if (reqToken !== queueRequestSeq) return
    if (keyword.value.trim() || isbn.value.trim()) {
      void runSearch()
    }
  },
)

watch(targetKind, async (next, prev) => {
  if (!props.open || hydratingFromDetail) return
  const d = props.detail
  if (d && d.metadata.kind === 'music') {
    if (next === 'music_album') {
      const alb = albumOrSeries.value.trim() || d.metadata.data.album.trim()
      if (alb) keyword.value = alb
    } else if (prev === 'music_album') {
      const song = d.metadata.data.title.trim() || d.entry.display_title
      if (song) keyword.value = song
    }
  }
  await refreshProvidersForTarget(next)
  if (keyword.value.trim() || isbn.value.trim()) {
    void runSearch()
  }
})

function toggleProvider(p: ProviderStatusDto): void {
  if (!p.configured) {
    emit('openSettings')
    return
  }
  if (enabledProviderIds.value.includes(p.id)) {
    enabledProviderIds.value = enabledProviderIds.value.filter((id) => id !== p.id)
  } else {
    // Re-insert preserving `providers.value` priority order
    const nextSet = new Set([...enabledProviderIds.value, p.id])
    enabledProviderIds.value = providers.value
      .filter((x) => nextSet.has(x.id))
      .map((x) => x.id)
  }
}

async function moveActiveProviderOrder(idx: number, delta: -1 | 1): Promise<void> {
  const list = [...enabledProviderIds.value]
  const targetIdx = idx + delta
  if (targetIdx < 0 || targetIdx >= list.length) return
  const [moved] = list.splice(idx, 1)
  list.splice(targetIdx, 0, moved!)
  enabledProviderIds.value = list

  // Also reorder `providers` locally and persist to user's domain `scraper_priorities`!
  const reorderedProviders = [...providers.value].sort((a, b) => {
    const pa = list.indexOf(a.id)
    const pb = list.indexOf(b.id)
    return (pa === -1 ? 999 : pa) - (pb === -1 ? 999 : pb)
  })
  providers.value = reorderedProviders

  const dom = props.detail?.entry.media_kind
  if (dom && props.settings) {
    const existingDomOrder = props.settings.scraper_priorities[dom]
    const fullOrder = [
      ...list,
      ...existingDomOrder.filter((id) => !list.includes(id)),
    ]
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

function markCoverBroken(url: string | null | undefined): void {
  if (!url) return
  const next = new Set(brokenCoverUrls.value)
  next.add(url)
  brokenCoverUrls.value = next
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

function toggleFieldEnabled(key: string): void {
  const next = new Set(disabledFields.value)
  if (next.has(key)) {
    next.delete(key)
  } else {
    next.add(key)
  }
  disabledFields.value = next
}

function buildFilteredMetadata(meta: ScrapedMetadata): ScrapedMetadata {
  const copy: ScrapedMetadata = { ...meta }
  if (disabledFields.value.has('title')) copy.title = null
  if (disabledFields.value.has('original_title')) copy.original_title = null
  if (disabledFields.value.has('series_or_album')) copy.series_or_album = null
  if (disabledFields.value.has('numbers')) {
    copy.track_or_episode_number = null
    copy.disc_or_season_number = null
  }
  if (disabledFields.value.has('creators')) copy.creators = []
  if (disabledFields.value.has('illustrators_or_directors')) copy.illustrators_or_directors = []
  if (disabledFields.value.has('cast')) copy.cast = []
  if (disabledFields.value.has('publisher_or_studio')) copy.publisher_or_studio = null
  if (disabledFields.value.has('year')) {
    copy.year = null
    copy.published_date = null
  }
  if (disabledFields.value.has('rating')) copy.rating = null
  if (disabledFields.value.has('isbn')) copy.isbn = null
  if (disabledFields.value.has('tags')) {
    copy.genres = []
    copy.tags = []
  }
  if (disabledFields.value.has('summary')) {
    copy.summary = null
    copy.show_summary = null
    copy.season_title = null
    copy.season_summary = null
  }
  if (disabledFields.value.has('lyrics')) {
    copy.synced_lyrics = null
    copy.plain_lyrics = null
  }
  return copy
}

async function runSearch(): Promise<void> {
  if (!keyword.value.trim() && !isbn.value.trim()) return
  const searchSeq = ++searchRequestSeq
  searching.value = true
  errorMsg.value = null
  candidates.value = []
  activeSourceTab.value = 'merged'
  previewMode.value = 'merged'
  brokenCoverUrls.value = new Set()
  selectedByProvider.value = {}
  detailsByProvider.value = {}

  const parsedSeason = season.value.trim() ? Number.parseInt(season.value, 10) : undefined
  const parsedEpisode = episode.value.trim() ? Number.parseInt(episode.value, 10) : undefined

  try {
    const agg = await toolApi.aggregateScrapers({
      target: targetKind.value,
      providers: enabledProviderIds.value,
      keyword: keyword.value.trim(),
      artist_or_author: artistOrAuthor.value.trim() || undefined,
      album_or_series: albumOrSeries.value.trim() || undefined,
      isbn: isbn.value.trim() || undefined,
      year: yearInput.value.trim() ? Number.parseInt(yearInput.value, 10) : undefined,
      season: parsedSeason !== undefined && Number.isFinite(parsedSeason) ? parsedSeason : undefined,
      episode: parsedEpisode !== undefined && Number.isFinite(parsedEpisode) ? parsedEpisode : undefined,
      duration_secs: durationSecs.value,
    })
    if (searchSeq !== searchRequestSeq) return
    candidates.value = agg.candidates
    selectedByProvider.value = agg.selected_by_provider
    detailsByProvider.value = agg.details_by_provider
  } catch (err) {
    if (searchSeq !== searchRequestSeq) return
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (searchSeq === searchRequestSeq) {
      searching.value = false
    }
  }
}

function stripSeasonEpisodeSuffix(rawExtId: string): string {
  return rawExtId.replace(/:S\d+E\d+$/i, '')
}

function buildEffectiveExternalId(
  rawExtId: string,
  target: ScrapeTargetKind,
  customSeason?: number | null,
  customEpisode?: number | null,
): string {
  if (target !== 'tv_show') return rawExtId
  const baseId = stripSeasonEpisodeSuffix(rawExtId)
  const s =
    customSeason !== undefined && customSeason !== null
      ? customSeason
      : season.value.trim()
        ? Number.parseInt(season.value, 10)
        : 1
  const e =
    customEpisode !== undefined && customEpisode !== null
      ? customEpisode
      : episode.value.trim()
        ? Number.parseInt(episode.value, 10)
        : 0
  if (Number.isFinite(e) && e > 0) {
    const safeS = Number.isFinite(s) && s >= 0 ? s : 1
    return `${baseId}:S${safeS}E${e}`
  }
  return baseId
}

let seTimer: ReturnType<typeof setTimeout> | null = null
watch([season, episode], () => {
  if (!props.open || hydratingFromDetail || targetKind.value !== 'tv_show') return
  if (Object.keys(selectedByProvider.value).length === 0) return
  if (seTimer) clearTimeout(seTimer)
  seTimer = setTimeout(async () => {
    for (const cand of Object.values(selectedByProvider.value)) {
      if (cand.target === 'tv_show') {
        await selectCandidateForProvider(cand)
      }
    }
  }, 350)
})

function switchSourceTab(tabId: string): void {
  activeSourceTab.value = tabId
  previewMode.value = tabId === 'merged' ? 'merged' : 'single'
}

async function selectCandidateForProvider(cand: ScrapeCandidate): Promise<void> {
  const effectiveExtId = buildEffectiveExternalId(cand.external_id, cand.target)
  const updatedCand: ScrapeCandidate = {
    ...cand,
    external_id: effectiveExtId,
  }
  selectedByProvider.value = {
    ...selectedByProvider.value,
    [cand.provider_id]: updatedCand,
  }
  loadingProviderId.value = cand.provider_id
  errorMsg.value = null
  try {
    const detail = await toolApi.getScraperDetail(cand.provider_id, effectiveExtId, cand.target)
    detailsByProvider.value = {
      ...detailsByProvider.value,
      [cand.provider_id]: detail,
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    loadingProviderId.value = null
  }
}

async function applyMetadata(mode: 'merged' | 'single' = 'merged'): Promise<void> {
  const targetMeta =
    mode === 'single' ? activeSingleProviderDetail.value : mergedResult.value?.metadata
  if (!props.detail || !targetMeta || applying.value) return
  applying.value = true
  errorMsg.value = null
  try {
    const updated = await toolApi.applyScrapedMetadata(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
      props.detail.entry.media_kind,
      buildFilteredMetadata(targetMeta),
      applyCover.value,
      writeSidecarLrc.value,
      organizeByTemplate.value,
      onlyFillMissing.value,
      writeAlbumCover.value,
    )
    emit('applied', updated)
    if (hasNextInQueue.value) {
      emit('selectQueueIndex', currentQueueIdx.value + 1)
    } else {
      emit('close')
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    applying.value = false
  }
}

async function runLockedQueueScrape(): Promise<void> {
  const anchorView = effectiveView.value
  if (applying.value || queueList.value.length === 0 || !anchorView) return
  applying.value = true
  errorMsg.value = null

  const useSingleMode =
    activeSourceTab.value !== 'merged' && previewMode.value === 'single' && activeSingleProviderDetail.value
  const anchorMeta = useSingleMode ? activeSingleProviderDetail.value! : anchorView.metadata
  const lockedSelected = { ...selectedByProvider.value }
  const anchorDetails = { ...detailsByProvider.value }
  const anchorOrder = [...orderedActiveBriefs.value]
  const fallbackSeason = season.value.trim() ? Number.parseInt(season.value, 10) : 1

  let lastUpdated: MediaFileDetail | null = null
  let successCount = 0
  let failCount = 0

  try {
    const slice = queueList.value.slice(currentQueueIdx.value)
    const taskId = `scrape:locked:${Date.now()}`
    taskStore.startTask({
      id: taskId,
      kind: 'scrape',
      title: `锁定作品批量刮削 (${slice.length} 项)`,
      total: slice.length,
      ephemeral: false,
    })
    for (let i = 0; i < slice.length; i += 1) {
      const entry = slice[i]!
      batchProgress.value = `锁定作品应用中 (${currentQueueIdx.value + i + 1} / ${queueList.value.length})：${entry.file_name}`
      batchProgressPct.value = Math.min(100, Math.round((i / slice.length) * 100))
      taskStore.updateTask(taskId, {
        processed: i,
        total: slice.length,
        detail: entry.file_name,
      })
      try {
        const d = await toolApi.inspectFile(entry.root_id, entry.rel_path, entry.media_kind)
        const q = buildAutoQueryFromDetail(d)
        let payloadToApply: ScrapedMetadata = { ...anchorMeta }

        if (entry.media_kind === 'video' && targetKind.value === 'tv_show') {
          const epS =
            q.season !== undefined && q.season !== null
              ? q.season
              : Number.isFinite(fallbackSeason)
                ? fallbackSeason
                : 1
          const epE = q.episode ?? null
          if (epE && epE > 0) {
            const perProvDetails: Record<string, ScrapedMetadata> = { ...anchorDetails }
            await Promise.all(
              Object.values(lockedSelected).map(async (cand) => {
                if (cand.target !== 'tv_show') return
                if (!useSingleMode && ignoredProviderIds.value.has(cand.provider_id)) return
                if (useSingleMode && cand.provider_id !== anchorMeta.provider_id) return
                const epExtId = buildEffectiveExternalId(cand.external_id, 'tv_show', epS, epE)
                try {
                  perProvDetails[cand.provider_id] = await toolApi.getScraperDetail(
                    cand.provider_id,
                    epExtId,
                    'tv_show',
                  )
                } catch {
                  // Retain series-level fallback for providers whose episode fetch fails
                }
              }),
            )
            if (useSingleMode) {
              payloadToApply = {
                ...(perProvDetails[anchorMeta.provider_id] ?? anchorMeta),
                disc_or_season_number: epS,
                track_or_episode_number: epE,
              }
            } else {
              const reMerged = mergeScrapedClientSide(
                anchorOrder,
                perProvDetails,
                'tv_show',
                brokenCoverUrls.value,
                ignoredProviderIds.value,
              )
              payloadToApply = {
                ...(reMerged?.metadata ?? anchorMeta),
                disc_or_season_number: epS,
                track_or_episode_number: epE,
              }
            }
          }
        } else if (entry.media_kind === 'comic') {
          // Locking a comic work applies series-level metadata while keeping each chapter's own title/number
          payloadToApply = {
            ...anchorMeta,
            track_or_episode_number: null,
            disc_or_season_number: null,
          }
        } else if (entry.media_kind === 'music') {
          // Locking a music work applies album-level metadata without clobbering individual song titles/numbers
          payloadToApply = {
            ...anchorMeta,
            target: 'music_album',
            series_or_album: anchorMeta.series_or_album || anchorMeta.title,
            track_or_episode_number: null,
            disc_or_season_number: null,
          }
        }

        lastUpdated = await toolApi.applyScrapedMetadata(
          entry.root_id,
          entry.rel_path,
          entry.media_kind,
          buildFilteredMetadata(payloadToApply),
          applyCover.value,
          writeSidecarLrc.value,
          organizeByTemplate.value,
          onlyFillMissing.value,
          writeAlbumCover.value,
        )
        successCount += 1
      } catch {
        failCount += 1
      }
      taskStore.updateTask(taskId, {
        processed: i + 1,
        total: slice.length,
        detail: entry.file_name,
      })
      batchProgressPct.value = Math.min(100, Math.round(((i + 1) / slice.length) * 100))
    }
    taskStore.completeTask(
      taskId,
      `成功写入 ${successCount} 项${failCount > 0 ? `，跳过/失败 ${failCount} 项` : ''}`,
    )
    emit('batchApplied', lastUpdated, { success: successCount, failed: failCount })
    emit('close')
  } finally {
    batchProgress.value = null
    batchProgressPct.value = null
    applying.value = false
  }
}

async function runBatchAutoScrape(): Promise<void> {
  if (applying.value || queueList.value.length === 0) return
  applying.value = true
  errorMsg.value = null
  let lastUpdated: MediaFileDetail | null = null
  let successCount = 0
  let failCount = 0
  const slice = queueList.value.slice(currentQueueIdx.value)
  const taskId = `scrape:auto:${Date.now()}`
  taskStore.startTask({
    id: taskId,
    kind: 'scrape',
    title: `全自动批量刮削 (${slice.length} 项)`,
    total: slice.length,
    ephemeral: false,
  })

  try {
    for (let i = 0; i < slice.length; i += 1) {
      const entry = slice[i]!
      batchProgress.value = `正在自动刮削 (${currentQueueIdx.value + i + 1} / ${queueList.value.length})：${entry.file_name}`
      batchProgressPct.value = Math.min(100, Math.round((i / slice.length) * 100))
      taskStore.updateTask(taskId, {
        processed: i,
        total: slice.length,
        detail: entry.file_name,
      })
      try {
        const d = await toolApi.inspectFile(entry.root_id, entry.rel_path, entry.media_kind)
        const q = buildAutoQueryFromDetail(d)
        const agg = await toolApi.aggregateScrapers({
          ...q,
          providers: enabledProviderIds.value,
        })
        if (agg.merged.metadata.title) {
          lastUpdated = await toolApi.applyScrapedMetadata(
            entry.root_id,
            entry.rel_path,
            entry.media_kind,
            buildFilteredMetadata(agg.merged.metadata),
            applyCover.value,
            writeSidecarLrc.value,
            organizeByTemplate.value,
            onlyFillMissing.value,
            writeAlbumCover.value,
          )
          successCount += 1
        } else {
          failCount += 1
        }
      } catch {
        failCount += 1
      }
      taskStore.updateTask(taskId, {
        processed: i + 1,
        total: slice.length,
        detail: entry.file_name,
      })
      batchProgressPct.value = Math.min(100, Math.round(((i + 1) / slice.length) * 100))
    }
    taskStore.completeTask(
      taskId,
      `成功刮削 ${successCount} 项${failCount > 0 ? `，未命中/失败 ${failCount} 项` : ''}`,
    )
    emit('batchApplied', lastUpdated, { success: successCount, failed: failCount })
    emit('close')
  } finally {
    batchProgress.value = null
    batchProgressPct.value = null
    applying.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open && detail"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-3 backdrop-blur-xs sm:p-6"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[92vh] w-full max-w-5xl flex-col overflow-hidden rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] shadow-2xl"
      >
        <!-- Header -->
        <div
          class="flex shrink-0 flex-wrap items-center justify-between gap-3 border-b border-[var(--ctp-surface0)]/80 px-5 py-3.5"
        >
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-2">
              <LuSparkles class="h-4 w-4 text-[var(--ctp-accent)]" />
              <h2 class="text-sm font-bold text-[var(--ctp-text)]">多源优先级互补在线刮削</h2>
              <span
                v-if="hasQueue"
                class="rounded-full bg-[var(--ctp-accent-soft)] px-2.5 py-0.5 font-mono text-[11px] font-bold text-[var(--ctp-accent)]"
              >
                队列 {{ currentQueueIdx + 1 }} / {{ queueList.length }}
              </span>
            </div>
            <p class="mt-0.5 truncate text-xs text-[var(--ctp-subtext0)]">
              {{ batchProgress ?? `目标文件：${detail.entry.rel_path}` }}
            </p>
          </div>

          <div class="flex items-center gap-1.5">
            <template v-if="hasQueue">
              <button
                type="button"
                :disabled="applying || currentQueueIdx <= 0"
                class="inline-flex h-8 items-center gap-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)] disabled:opacity-40"
                title="切换到队列上一项"
                @click="emit('selectQueueIndex', currentQueueIdx - 1)"
              >
                <LuChevronLeft class="h-3.5 w-3.5" />
                <span>上一项</span>
              </button>
              <button
                type="button"
                :disabled="applying || !hasNextInQueue"
                class="inline-flex h-8 items-center gap-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)] disabled:opacity-40"
                title="跳过当前项，查看下一项"
                @click="emit('selectQueueIndex', currentQueueIdx + 1)"
              >
                <span>跳过</span>
                <LuChevronRight class="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                :disabled="applying || !effectiveView"
                class="inline-flex h-8 items-center gap-1.5 rounded-xl border border-[var(--ctp-accent)]/40 bg-[var(--ctp-canvas)] px-3 text-xs font-semibold text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent-soft)] disabled:opacity-50"
                title="锁定当前选定的作品源并应用至剩余队列（剧集按SxxExx自动取分集、漫画/专辑保护分话与分轨字段）"
                @click="runLockedQueueScrape"
              >
                <LuLock class="h-3.5 w-3.5" />
                <span>锁定作品应用剩余 ({{ queueList.length - currentQueueIdx }})</span>
              </button>
              <button
                type="button"
                :disabled="applying"
                class="inline-flex h-8 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent-soft)] px-3 text-xs font-semibold text-[var(--ctp-accent)] transition hover:opacity-90 disabled:opacity-50"
                title="对剩余队列逐个重新检索并自动写入各自首条聚合结果"
                @click="runBatchAutoScrape"
              >
                <LuZap class="h-3.5 w-3.5" />
                <span>逐项重搜剩余</span>
              </button>
            </template>

            <button
              type="button"
              class="flex h-8 w-8 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
              @click="emit('close')"
            >
              <LuX class="h-4 w-4" />
            </button>
          </div>
        </div>

        <div
          v-if="batchProgressPct !== null"
          class="h-1 w-full shrink-0 overflow-hidden bg-[var(--ctp-surface0)]"
        >
          <div
            class="h-full bg-[var(--ctp-accent)] transition-all duration-150"
            :style="{ width: `${batchProgressPct}%` }"
          />
        </div>

        <!-- Target Tabs + Ordered Provider Pills + Query Bar -->
        <div
          class="shrink-0 space-y-3 border-b border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/40 px-5 py-3.5"
        >
          <div class="flex flex-wrap items-center justify-between gap-2">
            <!-- Target Selector -->
            <div class="flex flex-wrap items-center gap-1 rounded-xl bg-[var(--ctp-mantle)] p-1">
              <button
                v-for="tab in targetTabs"
                :key="tab.value"
                type="button"
                class="rounded-lg px-3 py-1 text-xs font-semibold transition"
                :class="
                  targetKind === tab.value
                    ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                "
                @click="targetKind = tab.value"
              >
                {{ tab.label }}
              </button>
            </div>

            <!-- Priority-Ordered Provider Toggle Pills -->
            <div class="flex flex-wrap items-center gap-1.5">
              <span class="text-[11px] text-[var(--ctp-subtext0)]">优先级源：</span>
              <button
                v-for="prov in providers"
                :key="prov.id"
                type="button"
                :title="prov.description"
                class="inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-[11px] font-medium transition"
                :class="
                  !prov.configured
                    ? 'border-dashed border-amber-500/45 bg-amber-500/10 text-amber-400'
                    : enabledProviderIds.includes(prov.id)
                      ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                      : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext0)]'
                "
                @click="toggleProvider(prov)"
              >
                <span
                  v-if="prov.configured && enabledProviderIds.includes(prov.id)"
                  class="font-mono text-[10px] font-bold"
                >
                  #{{ enabledProviderIds.indexOf(prov.id) + 1 }}
                </span>
                <LuKey v-else-if="!prov.configured" class="h-3 w-3" />
                <span>{{ prov.name }}</span>
                <span v-if="!prov.configured" class="text-[10px]">(去配置)</span>
              </button>
            </div>
          </div>

          <form class="grid grid-cols-1 gap-2 sm:grid-cols-12" @submit.prevent="runSearch">
            <input
              v-model="keyword"
              type="text"
              placeholder="标题 / 关键词…"
              class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              :class="
                detail.entry.media_kind === 'video' && targetKind === 'movie'
                  ? 'sm:col-span-8'
                  : 'sm:col-span-4'
              "
            />
            <input
              v-if="detail.entry.media_kind !== 'video'"
              v-model="artistOrAuthor"
              type="text"
              :placeholder="detail.entry.media_kind === 'music' ? '歌手 (可选)…' : '作者 (可选)…'"
              class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-3"
            />
            <input
              v-if="detail.entry.media_kind === 'book'"
              v-model="isbn"
              type="text"
              placeholder="ISBN (可选)…"
              class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-3"
            />
            <input
              v-if="detail.entry.media_kind === 'music'"
              v-model="albumOrSeries"
              type="text"
              placeholder="专辑 (可选)…"
              class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-3"
            />
            <input
              v-if="detail.entry.media_kind === 'comic'"
              v-model="yearInput"
              type="number"
              min="1800"
              max="2100"
              placeholder="年份 (可选)"
              class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-3"
            />
            <input
              v-if="detail.entry.media_kind === 'video'"
              v-model="yearInput"
              type="number"
              min="1880"
              max="2100"
              placeholder="年份"
              class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-2"
            />
            <template v-if="detail.entry.media_kind === 'video' && targetKind === 'tv_show'">
              <input
                v-model="season"
                type="number"
                min="0"
                placeholder="季 S"
                class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-2"
              />
              <input
                v-model="episode"
                type="number"
                min="0"
                placeholder="集 E"
                class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-2"
              />
            </template>
            <button
              type="submit"
              :disabled="searching"
              class="inline-flex items-center justify-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] transition hover:opacity-95 disabled:opacity-50 sm:col-span-2"
            >
              <LuRefreshCw v-if="searching" class="h-3.5 w-3.5 animate-spin" />
              <LuSearch v-else class="h-3.5 w-3.5" />
              <span>搜索聚合</span>
            </button>
          </form>
        </div>

        <!-- Split Pane: Left Source / Priority Selector + Right Provenance-Annotated Preview -->
        <div
          class="grid min-h-0 flex-1 grid-cols-1 divide-y divide-[var(--ctp-surface0)]/75 overflow-hidden md:grid-cols-12 md:divide-x md:divide-y-0"
        >
          <!-- LEFT COLUMN -->
          <ScrapeSourcePane
            :source-tabs="sourceTabs"
            :active-source-tab="activeSourceTab"
            :searching="searching"
            :ordered-active-briefs="orderedActiveBriefs"
            :ignored-provider-ids="ignoredProviderIds"
            :selected-by-provider="selectedByProvider"
            :loading-provider-id="loadingProviderId"
            :candidates-for-active-source="candidatesForActiveSource"
            :broken-cover-urls="brokenCoverUrls"
            @switch-source-tab="switchSourceTab"
            @toggle-provider-ignored="toggleProviderIgnored"
            @move-provider-order="moveActiveProviderOrder"
            @select-candidate="selectCandidateForProvider"
            @mark-cover-broken="markCoverBroken"
          />

          <!-- RIGHT COLUMN: Provenance-Annotated Merged Preview -->
          <ScrapePreviewPane
            :effective-view="effectiveView"
            :active-source-tab="activeSourceTab"
            :preview-mode="previewMode"
            :active-single-provider-name="activeSingleBadge?.provider_name"
            :searching="searching"
            :error-msg="errorMsg"
            :disabled-fields="disabledFields"
            :broken-cover-urls="brokenCoverUrls"
            :is-video="detail?.entry.media_kind === 'video'"
            @update:preview-mode="previewMode = $event"
            @toggle-field="toggleFieldEnabled"
            @mark-cover-broken="markCoverBroken"
          />
        </div>

        <!-- Footer -->
        <div
          class="flex shrink-0 flex-wrap items-center justify-between gap-3 border-t border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 px-5 py-3.5"
        >
          <div class="flex flex-wrap items-center gap-4">
            <AppCheckbox
              v-if="
                effectiveView?.metadata.cover_url ||
                effectiveView?.metadata.backdrop_url ||
                effectiveView?.metadata.season_poster_url ||
                effectiveView?.metadata.episode_still_url
              "
              v-model="applyCover"
              :label="
                detail.entry.media_kind === 'video'
                  ? '同步下载刮削图片'
                  : detail.entry.media_kind === 'comic'
                    ? '同步保存系列封面 (cover)'
                    : '同时下载并写入封面'
              "
            />
            <AppCheckbox
              v-model="organizeByTemplate"
              :label="
                detail.entry.media_kind === 'video'
                  ? '同时按 Jellyfin 模板归档移动并生成层级 NFO'
                  : detail.entry.media_kind === 'comic'
                    ? '同时按 Komga/Kavita 系列目录模板归档移动'
                    : detail.entry.media_kind === 'music'
                      ? '同时按 Picard/Navidrome 专辑目录模板归档移动'
                      : '同时按 Calibre 独立书目模板归档移动'
              "
            />
            <AppCheckbox
              v-if="
                detail.entry.media_kind === 'music' &&
                applyCover &&
                effectiveView?.metadata.cover_url
              "
              v-model="writeAlbumCover"
              label="同时在专辑目录写入 cover 封面"
            />
            <AppCheckbox
              v-model="onlyFillMissing"
              label="仅补齐本地缺失字段"
            />
            <AppCheckbox
              v-if="
                detail.entry.media_kind === 'music' &&
                effectiveView?.metadata.synced_lyrics
              "
              v-model="writeSidecarLrc"
              label="同步生成 .lrc 时间轴歌词"
            />
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <button
              type="button"
              class="rounded-xl px-3.5 py-2 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/65"
              @click="emit('close')"
            >
              取消
            </button>

            <template v-if="activeSourceTab !== 'merged' && previewMode === 'single' && activeSingleProviderDetail">
              <button
                type="button"
                :disabled="!mergedResult || applying"
                class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 py-2 text-xs font-semibold text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
                @click="applyMetadata('merged')"
              >
                <span>仍应用多源聚合</span>
              </button>
              <button
                type="button"
                :disabled="applying"
                class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
                @click="applyMetadata('single')"
              >
                <LuRefreshCw v-if="applying" class="h-3.5 w-3.5 animate-spin" />
                <LuExternalLink v-else class="h-3.5 w-3.5" />
                <span>应用此单源 ({{ activeSingleBadge?.provider_name }})</span>
              </button>
            </template>

            <button
              v-else
              type="button"
              :disabled="!mergedResult || applying"
              class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
              @click="applyMetadata('merged')"
            >
              <LuRefreshCw v-if="applying" class="h-3.5 w-3.5 animate-spin" />
              <LuExternalLink v-else class="h-3.5 w-3.5" />
              <span>
                {{
                  applying
                    ? batchProgress
                      ? '正在批量刮削…'
                      : '正在写入原文件…'
                    : hasNextInQueue
                      ? `应用聚合结果并继续 (${currentQueueIdx + 1}/${queueList.length})`
                      : '应用聚合结果并写入原文件'
                }}
              </span>
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
