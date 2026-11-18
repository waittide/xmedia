<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuArrowDown,
  LuArrowUp,
  LuBan,
  LuCheck,
  LuChevronLeft,
  LuChevronRight,
  LuExternalLink,
  LuImage,
  LuKey,
  LuLayers,
  LuLock,
  LuRefreshCw,
  LuSearch,
  LuSparkles,
  LuX,
  LuZap,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import { toolApi } from '../api'
import { mergeScrapedClientSide } from '../merger'
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
const organizeByTemplate = ref(false)
const onlyFillMissing = ref(false)
const writeSidecarLrc = ref(true)
const applying = ref(false)
const batchProgress = ref<string | null>(null)
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

function candidatesCountFor(providerId: string): number {
  return candidates.value.filter((c) => c.provider_id === providerId).length
}

function badgeName(key: string): string | null {
  return effectiveView.value?.sources[key]?.provider_name ?? null
}

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
  organizeByTemplate.value = d.entry.media_kind === 'video' || d.entry.media_kind === 'comic'
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

function isCoverUsable(url: string | null | undefined): url is string {
  return Boolean(url && !brokenCoverUrls.value.has(url))
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

function isFieldEnabled(key: string): boolean {
  return !disabledFields.value.has(key)
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
    for (let i = 0; i < slice.length; i += 1) {
      const entry = slice[i]!
      batchProgress.value = `锁定作品应用中 (${currentQueueIdx.value + i + 1} / ${queueList.value.length})：${entry.file_name}`
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
        )
        successCount += 1
      } catch {
        failCount += 1
      }
    }
    emit('batchApplied', lastUpdated, { success: successCount, failed: failCount })
    emit('close')
  } finally {
    batchProgress.value = null
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

  try {
    const slice = queueList.value.slice(currentQueueIdx.value)
    for (let i = 0; i < slice.length; i += 1) {
      const entry = slice[i]!
      batchProgress.value = `正在自动刮削 (${currentQueueIdx.value + i + 1} / ${queueList.value.length})：${entry.file_name}`
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
          )
          successCount += 1
        } else {
          failCount += 1
        }
      } catch {
        failCount += 1
      }
    }
    emit('batchApplied', lastUpdated, { success: successCount, failed: failCount })
    emit('close')
  } finally {
    batchProgress.value = null
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
          <div class="flex min-h-0 flex-col overflow-hidden md:col-span-5">
            <!-- Tabs: ✦ 多源聚合 | 源1 (n) | 源2 (n) ... -->
            <div
              class="flex shrink-0 items-center gap-1 overflow-x-auto border-b border-[var(--ctp-surface0)]/65 bg-[var(--ctp-mantle)]/30 px-3 py-2"
            >
              <button
                v-for="stab in sourceTabs"
                :key="stab.id"
                type="button"
                class="inline-flex shrink-0 items-center gap-1 rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
                :class="
                  activeSourceTab === stab.id
                    ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-xs'
                    : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/65 hover:text-[var(--ctp-text)]'
                "
                @click="switchSourceTab(stab.id)"
              >
                <span>{{ stab.label }}</span>
                <span
                  class="rounded-full px-1.5 py-0.2 font-mono text-[10px]"
                  :class="
                    activeSourceTab === stab.id
                      ? 'bg-black/15 text-[var(--ctp-on-accent)]'
                      : 'bg-[var(--ctp-mantle)] text-[var(--ctp-subtext0)]'
                  "
                >
                  {{ stab.count }}
                </span>
              </button>
            </div>

            <div class="flex-1 overflow-y-auto p-3">
              <div v-if="searching" class="py-12 text-center text-xs text-[var(--ctp-subtext0)]">
                正在并发检索各优先级刮削源并合成缺失字段…
              </div>

              <!-- 1. Default `✦ 多源聚合` Queue View -->
              <div v-else-if="activeSourceTab === 'merged'" class="space-y-2">
                <div class="px-1 text-[11px] text-[var(--ctp-subtext0)]">
                  按下方优先级顺序互补填充，可上下调整优先级或点入单源更换匹配条目：
                </div>

                <div
                  v-for="(brief, idx) in orderedActiveBriefs"
                  :key="brief.id"
                  class="rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 p-2.5 transition"
                  :class="ignoredProviderIds.has(brief.id) ? 'opacity-50' : ''"
                >
                  <div class="flex items-center justify-between gap-2">
                    <div class="flex items-center gap-1.5">
                      <span
                        class="flex h-5 w-5 items-center justify-center rounded-md bg-[var(--ctp-accent-soft)] font-mono text-[10px] font-bold text-[var(--ctp-accent)]"
                      >
                        #{{ idx + 1 }}
                      </span>
                      <span class="text-xs font-bold text-[var(--ctp-text)]">{{ brief.name }}</span>
                      <span
                        v-if="ignoredProviderIds.has(brief.id)"
                        class="rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] text-[var(--ctp-subtext0)]"
                      >
                        已排除
                      </span>
                    </div>

                    <div class="flex items-center gap-1">
                      <button
                        type="button"
                        class="flex h-6 w-6 items-center justify-center rounded-md transition"
                        :class="
                          ignoredProviderIds.has(brief.id)
                            ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                            : 'text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)]/70 hover:text-amber-500'
                        "
                        :title="ignoredProviderIds.has(brief.id) ? '恢复此源参与聚合' : '在本次聚合中排除此源'"
                        @click="toggleProviderIgnored(brief.id)"
                      >
                        <LuBan class="h-3.5 w-3.5" />
                      </button>
                      <button
                        type="button"
                        :disabled="idx === 0"
                        class="flex h-6 w-6 items-center justify-center rounded-md text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-accent)] disabled:opacity-30"
                        title="上移优先级"
                        @click="moveActiveProviderOrder(idx, -1)"
                      >
                        <LuArrowUp class="h-3.5 w-3.5" />
                      </button>
                      <button
                        type="button"
                        :disabled="idx === orderedActiveBriefs.length - 1"
                        class="flex h-6 w-6 items-center justify-center rounded-md text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-accent)] disabled:opacity-30"
                        title="下移优先级"
                        @click="moveActiveProviderOrder(idx, 1)"
                      >
                        <LuArrowDown class="h-3.5 w-3.5" />
                      </button>
                      <button
                        v-if="candidatesCountFor(brief.id) > 0"
                        type="button"
                        class="inline-flex items-center gap-0.5 rounded-lg bg-[var(--ctp-canvas)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent-soft)]"
                        @click="switchSourceTab(brief.id)"
                      >
                        <span>换选 ({{ candidatesCountFor(brief.id) }})</span>
                        <LuChevronRight class="h-3 w-3" />
                      </button>
                    </div>
                  </div>

                  <div
                    v-if="selectedByProvider[brief.id]"
                    class="mt-2 flex items-center gap-2.5 rounded-xl bg-[var(--ctp-canvas)]/80 p-2"
                  >
                    <div
                      class="flex h-11 w-9 shrink-0 items-center justify-center overflow-hidden rounded-md bg-[var(--ctp-mantle)]"
                    >
                      <img
                        v-if="isCoverUsable(selectedByProvider[brief.id]?.cover_url)"
                        :src="selectedByProvider[brief.id]!.cover_url!"
                        alt=""
                        class="h-full w-full object-cover"
                        loading="lazy"
                        @error="markCoverBroken(selectedByProvider[brief.id]?.cover_url)"
                      />
                      <LuImage v-else class="h-3.5 w-3.5 text-[var(--ctp-subtext0)]/55" />
                    </div>
                    <div class="min-w-0 flex-1">
                      <div class="truncate text-xs font-semibold text-[var(--ctp-text)]">
                        {{ selectedByProvider[brief.id]!.title }}
                      </div>
                      <div class="truncate text-[11px] text-[var(--ctp-subtext0)]">
                        {{ selectedByProvider[brief.id]!.subtitle || '已参与聚合' }}
                      </div>
                    </div>
                    <LuRefreshCw
                      v-if="loadingProviderId === brief.id"
                      class="h-3.5 w-3.5 shrink-0 animate-spin text-[var(--ctp-accent)]"
                    />
                  </div>
                  <div v-else class="mt-1.5 px-1 text-[11px] text-[var(--ctp-subtext0)]">
                    未检索到匹配条目
                  </div>
                </div>
              </div>

              <!-- 2. Individual Provider Candidate List -->
              <div
                v-else-if="candidatesForActiveSource.length === 0"
                class="py-12 text-center text-xs text-[var(--ctp-subtext0)]"
              >
                该刮削源下暂无匹配条目
              </div>
              <div v-else class="space-y-1.5">
                <button
                  v-for="cand in candidatesForActiveSource"
                  :key="`${cand.provider_id}-${cand.external_id}`"
                  type="button"
                  class="flex w-full items-start gap-3 rounded-2xl p-2.5 text-left transition"
                  :class="
                    selectedByProvider[cand.provider_id]?.external_id === cand.external_id
                      ? 'bg-[var(--ctp-accent-soft)] ring-1 ring-[var(--ctp-accent)]'
                      : 'hover:bg-[var(--ctp-surface0)]/55'
                  "
                  @click="selectCandidateForProvider(cand)"
                >
                  <div
                    class="flex h-14 w-11 shrink-0 items-center justify-center overflow-hidden rounded-lg bg-[var(--ctp-mantle)]"
                  >
                    <img
                      v-if="isCoverUsable(cand.cover_url)"
                      :src="cand.cover_url"
                      alt=""
                      class="h-full w-full object-cover"
                      loading="lazy"
                      @error="markCoverBroken(cand.cover_url)"
                    />
                    <LuImage v-else class="h-4 w-4 text-[var(--ctp-subtext0)]/55" />
                  </div>
                  <div class="min-w-0 flex-1">
                    <div class="flex items-center gap-1.5">
                      <span
                        v-if="selectedByProvider[cand.provider_id]?.external_id === cand.external_id"
                        class="inline-flex items-center gap-0.5 rounded-md bg-[var(--ctp-accent)] px-1.5 py-0.2 text-[10px] font-semibold text-[var(--ctp-on-accent)]"
                      >
                        <LuCheck class="h-2.5 w-2.5" />
                        当前采用
                      </span>
                      <span v-if="cand.year" class="text-[10px] text-[var(--ctp-subtext0)]">
                        {{ cand.year }}
                      </span>
                    </div>
                    <div class="mt-1 truncate text-xs font-semibold text-[var(--ctp-text)]">
                      {{ cand.title }}
                    </div>
                    <div
                      v-if="cand.subtitle"
                      class="mt-0.5 truncate text-[11px] text-[var(--ctp-subtext0)]"
                    >
                      {{ cand.subtitle }}
                    </div>
                  </div>
                </button>
              </div>
            </div>
          </div>

          <!-- RIGHT COLUMN: Provenance-Annotated Merged Preview -->
          <div class="overflow-y-auto p-4 md:col-span-7">
            <div
              v-if="errorMsg"
              class="mb-3 rounded-xl border border-red-500/35 bg-red-500/10 px-3 py-2 text-xs text-red-400"
            >
              {{ errorMsg }}
            </div>

            <!-- Preview Mode Header -->
            <div
              v-if="effectiveView"
              class="mb-3 flex flex-wrap items-center justify-between gap-2 rounded-xl bg-[var(--ctp-mantle)]/55 px-3 py-2"
            >
              <div class="flex items-center gap-1.5 text-xs font-semibold text-[var(--ctp-text)]">
                <LuLayers class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                <span>
                  {{
                    activeSourceTab !== 'merged' && previewMode === 'single'
                      ? `单源原始信息（${activeSingleBadge?.provider_name}）`
                      : '多源优先级互补合成结果（每项右侧标注实际来源）'
                  }}
                </span>
              </div>

              <div
                v-if="activeSourceTab !== 'merged'"
                class="flex items-center gap-1 rounded-lg bg-[var(--ctp-canvas)] p-0.5"
              >
                <button
                  type="button"
                  class="rounded-md px-2 py-0.5 text-[11px] font-semibold transition"
                  :class="
                    previewMode === 'merged'
                      ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                      : 'text-[var(--ctp-subtext1)]'
                  "
                  @click="previewMode = 'merged'"
                >
                  看聚合后
                </button>
                <button
                  type="button"
                  class="rounded-md px-2 py-0.5 text-[11px] font-semibold transition"
                  :class="
                    previewMode === 'single'
                      ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                      : 'text-[var(--ctp-subtext1)]'
                  "
                  @click="previewMode = 'single'"
                >
                  仅看此源
                </button>
              </div>
            </div>

            <div v-if="searching" class="py-16 text-center text-xs text-[var(--ctp-subtext0)]">
              正在合成多源元信息…
            </div>
            <div v-else-if="effectiveView" class="space-y-3.5">
              <!-- Top Block: Cover (with Source Badge) + Core Annotated Fields -->
              <div class="flex flex-col gap-4 sm:flex-row">
                <div class="flex shrink-0 flex-col items-center gap-1.5">
                  <div
                    class="flex h-38 w-28 items-center justify-center overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] shadow-md"
                  >
                    <img
                      v-if="isCoverUsable(effectiveView.metadata.cover_url)"
                      :src="effectiveView.metadata.cover_url"
                      alt=""
                      class="h-full w-full object-cover"
                      @error="markCoverBroken(effectiveView.metadata.cover_url)"
                    />
                    <div
                      v-else
                      class="flex flex-col items-center gap-1 text-[var(--ctp-subtext0)]"
                    >
                      <LuImage class="h-6 w-6 opacity-50" />
                      <span class="text-[10px]">暂无封面</span>
                    </div>
                  </div>
                  <span
                    v-if="isCoverUsable(effectiveView.metadata.cover_url) && badgeName('cover_url')"
                    class="rounded-full bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
                  >
                    封面 · {{ badgeName('cover_url') }}
                  </span>
                  <span
                    v-if="isCoverUsable(effectiveView.metadata.backdrop_url) && badgeName('backdrop_url')"
                    class="rounded-full bg-[var(--ctp-mantle)] px-2 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext1)]"
                  >
                    背景图 · {{ badgeName('backdrop_url') }}
                  </span>
                </div>

                <!-- Field-by-Field Provenance List with Inclusion Toggles -->
                <div class="min-w-0 flex-1 space-y-2 rounded-2xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/35 p-3">
                  <!-- Title -->
                  <div
                    v-if="effectiveView.metadata.title"
                    class="flex items-start justify-between gap-2 transition"
                    :class="!isFieldEnabled('title') ? 'opacity-45' : ''"
                  >
                    <label class="flex min-w-0 cursor-pointer items-start gap-2">
                      <input
                        type="checkbox"
                        :checked="isFieldEnabled('title')"
                        class="mt-1 h-3.5 w-3.5 accent-[var(--ctp-accent)]"
                        @change="toggleFieldEnabled('title')"
                      />
                      <div class="min-w-0">
                        <span class="text-[10px] text-[var(--ctp-subtext0)]">标题</span>
                        <div class="text-sm font-bold text-[var(--ctp-text)]">
                          {{ effectiveView.metadata.title }}
                        </div>
                      </div>
                    </label>
                    <span
                      v-if="badgeName('title')"
                      class="shrink-0 rounded-md bg-[var(--ctp-accent-soft)] px-1.5 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
                    >
                      {{ badgeName('title') }}
                    </span>
                  </div>

                  <!-- Original Title -->
                  <div
                    v-if="
                      effectiveView.metadata.original_title &&
                      effectiveView.metadata.original_title !== effectiveView.metadata.title
                    "
                    class="flex items-center justify-between gap-2 text-xs transition"
                    :class="!isFieldEnabled('original_title') ? 'opacity-45' : ''"
                  >
                    <label class="flex min-w-0 cursor-pointer items-center gap-2">
                      <input
                        type="checkbox"
                        :checked="isFieldEnabled('original_title')"
                        class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
                        @change="toggleFieldEnabled('original_title')"
                      />
                      <span class="truncate text-[var(--ctp-subtext1)]">
                        <b class="font-medium text-[var(--ctp-subtext0)]">原名：</b>
                        {{ effectiveView.metadata.original_title }}
                      </span>
                    </label>
                    <span
                      v-if="badgeName('original_title')"
                      class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
                    >
                      {{ badgeName('original_title') }}
                    </span>
                  </div>

                  <!-- Series / Album -->
                  <div
                    v-if="effectiveView.metadata.series_or_album"
                    class="flex items-center justify-between gap-2 text-xs transition"
                    :class="!isFieldEnabled('series_or_album') ? 'opacity-45' : ''"
                  >
                    <label class="flex min-w-0 cursor-pointer items-center gap-2">
                      <input
                        type="checkbox"
                        :checked="isFieldEnabled('series_or_album')"
                        class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
                        @change="toggleFieldEnabled('series_or_album')"
                      />
                      <span class="truncate text-[var(--ctp-subtext1)]">
                        <b class="font-medium text-[var(--ctp-subtext0)]">系列 / 专辑：</b>
                        {{ effectiveView.metadata.series_or_album }}
                      </span>
                    </label>
                    <span
                      v-if="badgeName('series_or_album')"
                      class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
                    >
                      {{ badgeName('series_or_album') }}
                    </span>
                  </div>

                  <!-- Track / Episode Number -->
                  <div
                    v-if="
                      effectiveView.metadata.track_or_episode_number ||
                      effectiveView.metadata.disc_or_season_number !== null
                    "
                    class="flex items-center justify-between gap-2 text-xs transition"
                    :class="!isFieldEnabled('numbers') ? 'opacity-45' : ''"
                  >
                    <label class="flex min-w-0 cursor-pointer items-center gap-2">
                      <input
                        type="checkbox"
                        :checked="isFieldEnabled('numbers')"
                        class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
                        @change="toggleFieldEnabled('numbers')"
                      />
                      <span class="text-[var(--ctp-subtext1)]">
                        <b class="font-medium text-[var(--ctp-subtext0)]">季碟 / 序号：</b>
                        #{{ effectiveView.metadata.disc_or_season_number ?? 1 }} -
                        {{ effectiveView.metadata.track_or_episode_number ?? 1 }}
                      </span>
                    </label>
                    <span
                      v-if="
                        badgeName('track_or_episode_number') ||
                        badgeName('disc_or_season_number')
                      "
                      class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
                    >
                      {{
                        badgeName('track_or_episode_number') ||
                        badgeName('disc_or_season_number')
                      }}
                    </span>
                  </div>

                  <!-- Creators -->
                  <div
                    v-if="effectiveView.metadata.creators.length > 0"
                    class="flex items-center justify-between gap-2 text-xs transition"
                    :class="!isFieldEnabled('creators') ? 'opacity-45' : ''"
                  >
                    <label class="flex min-w-0 cursor-pointer items-center gap-2">
                      <input
                        type="checkbox"
                        :checked="isFieldEnabled('creators')"
                        class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
                        @change="toggleFieldEnabled('creators')"
                      />
                      <span class="truncate text-[var(--ctp-subtext1)]">
                        <b class="font-medium text-[var(--ctp-subtext0)]">创作者：</b>
                        {{ effectiveView.metadata.creators.join(' / ') }}
                      </span>
                    </label>
                    <span
                      v-if="badgeName('creators')"
                      class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
                    >
                      {{ badgeName('creators') }}
                    </span>
                  </div>

                  <!-- Directors / Pencillers -->
                  <div
                    v-if="effectiveView.metadata.illustrators_or_directors.length > 0"
                    class="flex items-center justify-between gap-2 text-xs transition"
                    :class="!isFieldEnabled('illustrators_or_directors') ? 'opacity-45' : ''"
                  >
                    <label class="flex min-w-0 cursor-pointer items-center gap-2">
                      <input
                        type="checkbox"
                        :checked="isFieldEnabled('illustrators_or_directors')"
                        class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
                        @change="toggleFieldEnabled('illustrators_or_directors')"
                      />
                      <span class="truncate text-[var(--ctp-subtext1)]">
                        <b class="font-medium text-[var(--ctp-subtext0)]">导演 / 作画：</b>
                        {{ effectiveView.metadata.illustrators_or_directors.join(' / ') }}
                      </span>
                    </label>
                    <span
                      v-if="badgeName('illustrators_or_directors')"
                      class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
                    >
                      {{ badgeName('illustrators_or_directors') }}
                    </span>
                  </div>

                  <!-- Cast -->
                  <div
                    v-if="effectiveView.metadata.cast.length > 0"
                    class="flex items-center justify-between gap-2 text-xs transition"
                    :class="!isFieldEnabled('cast') ? 'opacity-45' : ''"
                  >
                    <label class="flex min-w-0 cursor-pointer items-center gap-2">
                      <input
                        type="checkbox"
                        :checked="isFieldEnabled('cast')"
                        class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
                        @change="toggleFieldEnabled('cast')"
                      />
                      <span class="truncate text-[var(--ctp-subtext1)]">
                        <b class="font-medium text-[var(--ctp-subtext0)]">主演 / 声优：</b>
                        {{ effectiveView.metadata.cast.slice(0, 6).join(' / ') }}
                      </span>
                    </label>
                    <span
                      v-if="badgeName('cast')"
                      class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
                    >
                      {{ badgeName('cast') }}
                    </span>
                  </div>

                  <!-- Publisher / Studio -->
                  <div
                    v-if="effectiveView.metadata.publisher_or_studio"
                    class="flex items-center justify-between gap-2 text-xs transition"
                    :class="!isFieldEnabled('publisher_or_studio') ? 'opacity-45' : ''"
                  >
                    <label class="flex min-w-0 cursor-pointer items-center gap-2">
                      <input
                        type="checkbox"
                        :checked="isFieldEnabled('publisher_or_studio')"
                        class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
                        @change="toggleFieldEnabled('publisher_or_studio')"
                      />
                      <span class="truncate text-[var(--ctp-subtext1)]">
                        <b class="font-medium text-[var(--ctp-subtext0)]">出品 / 出版社：</b>
                        {{ effectiveView.metadata.publisher_or_studio }}
                      </span>
                    </label>
                    <span
                      v-if="badgeName('publisher_or_studio')"
                      class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
                    >
                      {{ badgeName('publisher_or_studio') }}
                    </span>
                  </div>

                  <!-- Year / Rating / ISBN -->
                  <div class="flex flex-wrap items-center gap-2 pt-0.5 text-xs">
                    <label
                      v-if="effectiveView.metadata.year"
                      class="inline-flex cursor-pointer items-center gap-1 rounded-lg bg-[var(--ctp-canvas)] px-2 py-0.5 text-[11px] transition"
                      :class="!isFieldEnabled('year') ? 'opacity-45' : ''"
                    >
                      <input
                        type="checkbox"
                        :checked="isFieldEnabled('year')"
                        class="h-3 w-3 accent-[var(--ctp-accent)]"
                        @change="toggleFieldEnabled('year')"
                      />
                      <span>年份 {{ effectiveView.metadata.year }}</span>
                      <span v-if="badgeName('year')" class="text-[10px] text-[var(--ctp-accent)]">
                        · {{ badgeName('year') }}
                      </span>
                    </label>
                    <label
                      v-if="effectiveView.metadata.rating"
                      class="inline-flex cursor-pointer items-center gap-1 rounded-lg bg-[var(--ctp-canvas)] px-2 py-0.5 text-[11px] transition"
                      :class="!isFieldEnabled('rating') ? 'opacity-45' : ''"
                    >
                      <input
                        type="checkbox"
                        :checked="isFieldEnabled('rating')"
                        class="h-3 w-3 accent-[var(--ctp-accent)]"
                        @change="toggleFieldEnabled('rating')"
                      />
                      <span>★ {{ effectiveView.metadata.rating.toFixed(1) }}</span>
                      <span v-if="badgeName('rating')" class="text-[10px] text-[var(--ctp-accent)]">
                        · {{ badgeName('rating') }}
                      </span>
                    </label>
                    <label
                      v-if="effectiveView.metadata.isbn"
                      class="inline-flex cursor-pointer items-center gap-1 rounded-lg bg-[var(--ctp-canvas)] px-2 py-0.5 font-mono text-[11px] transition"
                      :class="!isFieldEnabled('isbn') ? 'opacity-45' : ''"
                    >
                      <input
                        type="checkbox"
                        :checked="isFieldEnabled('isbn')"
                        class="h-3 w-3 accent-[var(--ctp-accent)]"
                        @change="toggleFieldEnabled('isbn')"
                      />
                      <span>ISBN {{ effectiveView.metadata.isbn }}</span>
                      <span v-if="badgeName('isbn')" class="font-sans text-[10px] text-[var(--ctp-accent)]">
                        · {{ badgeName('isbn') }}
                      </span>
                    </label>
                  </div>

                  <!-- Genres & Tags -->
                  <div
                    v-if="
                      effectiveView.metadata.genres.length > 0 ||
                      effectiveView.metadata.tags.length > 0
                    "
                    class="flex items-start justify-between gap-2 pt-1 transition"
                    :class="!isFieldEnabled('tags') ? 'opacity-45' : ''"
                  >
                    <label class="flex cursor-pointer items-start gap-2">
                      <input
                        type="checkbox"
                        :checked="isFieldEnabled('tags')"
                        class="mt-1 h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
                        @change="toggleFieldEnabled('tags')"
                      />
                      <div class="flex flex-wrap gap-1">
                        <span
                          v-for="tag in [
                            ...effectiveView.metadata.genres,
                            ...effectiveView.metadata.tags,
                          ].slice(0, 8)"
                          :key="tag"
                          class="rounded-full bg-[var(--ctp-canvas)] px-2 py-0.5 text-[10px] text-[var(--ctp-subtext1)]"
                        >
                          {{ tag }}
                        </span>
                      </div>
                    </label>
                    <span
                      v-if="badgeName('genres') || badgeName('tags')"
                      class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
                    >
                      {{ badgeName('genres') || badgeName('tags') }}
                    </span>
                  </div>
                </div>
              </div>

              <!-- Video Jellyfin Scraped Artwork Strip (Fanart / Season Poster / Episode Thumb) -->
              <div
                v-if="
                  detail.entry.media_kind === 'video' &&
                  (isCoverUsable(effectiveView.metadata.backdrop_url) ||
                    isCoverUsable(effectiveView.metadata.season_poster_url) ||
                    isCoverUsable(effectiveView.metadata.episode_still_url))
                "
                class="rounded-2xl border border-[var(--ctp-surface0)]/70 bg-[var(--ctp-mantle)]/45 p-3"
              >
                <div class="mb-2 flex items-center justify-between">
                  <span class="text-[11px] font-semibold text-[var(--ctp-subtext0)]">
                    Jellyfin 附属刮削图片（写入 fanart / Season 海报 / 单集 thumb）
                  </span>
                </div>
                <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-3">
                  <div
                    v-if="isCoverUsable(effectiveView.metadata.backdrop_url)"
                    class="space-y-1 rounded-xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-canvas)] p-2"
                  >
                    <div class="aspect-video overflow-hidden rounded-lg bg-[var(--ctp-mantle)]">
                      <img
                        :src="effectiveView.metadata.backdrop_url"
                        alt=""
                        class="h-full w-full object-cover"
                        @error="markCoverBroken(effectiveView.metadata.backdrop_url)"
                      />
                    </div>
                    <div class="flex items-center justify-between text-[10px]">
                      <span class="font-medium text-[var(--ctp-subtext1)]">fanart.jpg</span>
                      <span v-if="badgeName('backdrop_url')" class="font-semibold text-[var(--ctp-accent)]">
                        {{ badgeName('backdrop_url') }}
                      </span>
                    </div>
                  </div>

                  <div
                    v-if="isCoverUsable(effectiveView.metadata.season_poster_url)"
                    class="space-y-1 rounded-xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-canvas)] p-2"
                  >
                    <div class="aspect-video overflow-hidden rounded-lg bg-[var(--ctp-mantle)]">
                      <img
                        :src="effectiveView.metadata.season_poster_url"
                        alt=""
                        class="h-full w-full object-contain"
                        @error="markCoverBroken(effectiveView.metadata.season_poster_url)"
                      />
                    </div>
                    <div class="flex items-center justify-between text-[10px]">
                      <span class="font-medium text-[var(--ctp-subtext1)]">Season/poster.jpg</span>
                      <span
                        v-if="badgeName('season_poster_url')"
                        class="font-semibold text-[var(--ctp-accent)]"
                      >
                        {{ badgeName('season_poster_url') }}
                      </span>
                    </div>
                  </div>

                  <div
                    v-if="isCoverUsable(effectiveView.metadata.episode_still_url)"
                    class="space-y-1 rounded-xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-canvas)] p-2"
                  >
                    <div class="aspect-video overflow-hidden rounded-lg bg-[var(--ctp-mantle)]">
                      <img
                        :src="effectiveView.metadata.episode_still_url"
                        alt=""
                        class="h-full w-full object-cover"
                        @error="markCoverBroken(effectiveView.metadata.episode_still_url)"
                      />
                    </div>
                    <div class="flex items-center justify-between text-[10px]">
                      <span class="font-medium text-[var(--ctp-subtext1)]">单集 -thumb.jpg</span>
                      <span
                        v-if="badgeName('episode_still_url')"
                        class="font-semibold text-[var(--ctp-accent)]"
                      >
                        {{ badgeName('episode_still_url') }}
                      </span>
                    </div>
                  </div>
                </div>
              </div>

              <!-- Summary Block with Provenance Badge -->
              <div
                v-if="effectiveView.metadata.summary"
                class="rounded-2xl border border-[var(--ctp-surface0)]/70 bg-[var(--ctp-mantle)]/55 p-3 transition"
                :class="!isFieldEnabled('summary') ? 'opacity-45' : ''"
              >
                <div class="mb-1.5 flex items-center justify-between gap-2">
                  <label class="flex cursor-pointer items-center gap-2">
                    <input
                      type="checkbox"
                      :checked="isFieldEnabled('summary')"
                      class="h-3.5 w-3.5 accent-[var(--ctp-accent)]"
                      @change="toggleFieldEnabled('summary')"
                    />
                    <span class="text-[11px] font-semibold text-[var(--ctp-subtext0)]">
                      {{
                        effectiveView.metadata.target === 'tv_show'
                          ? '单集简介 (写入 <episodedetails>)'
                          : '内容简介'
                      }}
                    </span>
                  </label>
                  <span
                    v-if="badgeName('summary')"
                    class="rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
                  >
                    {{ badgeName('summary') }}
                  </span>
                </div>
                <p class="text-xs leading-relaxed whitespace-pre-line text-[var(--ctp-text)]">
                  {{ effectiveView.metadata.summary }}
                </p>
              </div>

              <!-- Series / Season NFO Summary Preview for TV Shows -->
              <div
                v-if="
                  effectiveView.metadata.target === 'tv_show' &&
                  (effectiveView.metadata.show_summary || effectiveView.metadata.season_summary) &&
                  (effectiveView.metadata.show_summary !== effectiveView.metadata.summary ||
                    effectiveView.metadata.season_title)
                "
                class="rounded-2xl border border-[var(--ctp-surface0)]/70 bg-[var(--ctp-mantle)]/40 p-3"
              >
                <div class="mb-1.5 flex items-center justify-between gap-2">
                  <span class="text-[11px] font-semibold text-[var(--ctp-subtext0)]">
                    剧集 / 季级简介 (写入 tvshow.nfo &amp; season.nfo{{
                      effectiveView.metadata.season_title
                        ? ` · ${effectiveView.metadata.season_title}`
                        : ''
                    }})
                  </span>
                  <span
                    v-if="badgeName('season_summary') || badgeName('show_summary')"
                    class="rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
                  >
                    {{ badgeName('season_summary') || badgeName('show_summary') }}
                  </span>
                </div>
                <p class="text-xs leading-relaxed whitespace-pre-line text-[var(--ctp-subtext1)]">
                  {{ effectiveView.metadata.season_summary || effectiveView.metadata.show_summary }}
                </p>
              </div>

              <!-- Lyrics Block with Provenance Badge -->
              <div
                v-if="
                  effectiveView.metadata.synced_lyrics || effectiveView.metadata.plain_lyrics
                "
                class="rounded-2xl border border-[var(--ctp-surface0)]/70 bg-[var(--ctp-mantle)]/55 p-3 transition"
                :class="!isFieldEnabled('lyrics') ? 'opacity-45' : ''"
              >
                <div class="mb-1.5 flex items-center justify-between gap-2">
                  <label class="flex cursor-pointer items-center gap-2">
                    <input
                      type="checkbox"
                      :checked="isFieldEnabled('lyrics')"
                      class="h-3.5 w-3.5 accent-[var(--ctp-accent)]"
                      @change="toggleFieldEnabled('lyrics')"
                    />
                    <span class="text-[11px] font-semibold text-[var(--ctp-subtext0)]">
                      歌词预览 ({{
                        effectiveView.metadata.synced_lyrics ? 'LRC 时间轴同步歌词' : '纯文本歌词'
                      }})
                    </span>
                  </label>
                  <span
                    v-if="badgeName('lyrics')"
                    class="rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
                  >
                    {{ badgeName('lyrics') }}
                  </span>
                </div>
                <pre
                  class="max-h-48 overflow-y-auto font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-[var(--ctp-text)]"
                >{{ effectiveView.metadata.synced_lyrics || effectiveView.metadata.plain_lyrics }}</pre>
              </div>
            </div>

            <div
              v-else
              class="flex h-full items-center justify-center py-16 text-xs text-[var(--ctp-subtext0)]"
            >
              暂无可用刮削数据，请调整关键词或启用更多刮削源后重试
            </div>
          </div>
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
                    : '刮削后同时按模板整理文件名/目录'
              "
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
