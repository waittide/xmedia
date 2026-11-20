<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuArrowLeft,
  LuCheck,
  LuChevronDown,
  LuChevronUp,
  LuImage,
  LuKey,
  LuLayers,
  LuLoader2,
  LuSearch,
  LuSparkles,
  LuZap,
} from 'vue-icons-plus/lu'
import { resolveScraperCoverUrl, toolApi } from '../../api'
import type {
  AggregateScrapeResponse,
  FieldSourceBadge,
  MediaFileDetail,
  ProviderStatusDto,
  ScrapeCandidate,
  ScrapedMetadata,
  ScrapeTargetKind,
} from '../../types'
import {
  buildDefaultQueryFromDetail,
  inferTargetKindFromDetail,
  type ApplyFillMode,
  type ScrapedFieldKey,
} from '../inspector/useMetadataForm'
import type { CandidatePreviewState } from './CandidateDetailPreviewPane.vue'

const props = defineProps<{
  detail: MediaFileDetail | null
  autoSearchTrigger: number
  externalKeyword?: string
  externalArtist?: string
  selectedPreviewKey?: string | null
}>()

const emit = defineEmits<{
  fillAll: [meta: ScrapedMetadata, mode: ApplyFillMode]
  fillField: [field: ScrapedFieldKey, meta: ScrapedMetadata]
  directApply: [meta: ScrapedMetadata, mode: ApplyFillMode]
  selectPreview: [preview: CandidatePreviewState | null]
  openSettings: []
}>()

const keyword = ref('')
const artistOrAuthor = ref('')
const yearInput = ref('')
const targetKind = ref<ScrapeTargetKind>('book')
const fillMode = ref<ApplyFillMode>('overwrite')

const providers = ref<ProviderStatusDto[]>([])
const enabledProviderIds = ref<string[]>([])
const activeSourceFilter = ref<string>('all')

const searching = ref(false)
const searchError = ref<string | null>(null)
const candidates = ref<ScrapeCandidate[]>([])
const mergedMeta = ref<ScrapedMetadata | null>(null)
const mergedFieldSources = ref<Record<string, FieldSourceBadge>>({})
const mergedExpanded = ref(false)
const expandedKey = ref<string | null>(null)
const selectedKey = ref<string | null>(null)
const loadingDetailKey = ref<string | null>(null)
const detailCache = ref<Map<string, ScrapedMetadata>>(new Map())
const brokenCoverUrls = ref<Set<string>>(new Set())
const recentFlash = ref<string | null>(null)

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

function mergedSourceLabelFor(fieldKey: ScrapedFieldKey): string | null {
  const map = mergedFieldSources.value
  const lookupKeys: Record<ScrapedFieldKey, string[]> = {
    cover: ['cover_url'],
    title: ['title'],
    original_title: ['original_title'],
    creators: ['creators'],
    directors: ['illustrators_or_directors'],
    cast: ['cast'],
    series_or_album: ['series_or_album'],
    publisher_or_studio: ['publisher_or_studio'],
    year: ['published_date', 'year'],
    isbn: ['isbn'],
    rating: ['rating'],
    language: ['language'],
    track_or_episode: ['track_or_episode_number'],
    disc_or_season: ['disc_or_season_number'],
    genres: ['genres'],
    tags: ['tags', 'genres'],
    summary: ['summary'],
    lyrics: ['synced_lyrics', 'plain_lyrics'],
  }
  for (const k of lookupKeys[fieldKey] ?? []) {
    const badge = map[k]
    if (badge?.provider_name) return badge.provider_name
  }
  return null
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
  const cached = detailCache.value.get(key)
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
    const cached = detailCache.value.get(candKey(hit))
    emit('selectPreview', {
      key: candKey(hit),
      isMerged: false,
      sourceLabel: hit.provider_name,
      meta: cached ?? buildLightMetadata(hit),
      loading: loadingDetailKey.value === candKey(hit),
      fieldSources: null,
      fillMode: fillMode.value,
    })
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

async function loadProvidersForDetail(d: MediaFileDetail): Promise<void> {
  const target = inferTargetKindFromDetail(d)
  targetKind.value = target
  try {
    const list = await toolApi.listProviders(target, d.entry.media_kind)
    providers.value = list
    // Keep currently enabled ids if valid, else select all configured providers
    const configuredIds = list.filter((p) => p.configured).map((p) => p.id)
    const retained = enabledProviderIds.value.filter((id) => configuredIds.includes(id))
    enabledProviderIds.value = retained.length > 0 ? retained : configuredIds
  } catch {
    providers.value = []
  }
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
    enabledProviderIds.value = [...enabledProviderIds.value, p.id]
  }
}

async function runSearch(): Promise<void> {
  if (!props.detail) return
  const kw = keyword.value.trim()
  if (!kw) return
  searching.value = true
  searchError.value = null
  candidates.value = []
  mergedMeta.value = null
  mergedFieldSources.value = {}
  mergedExpanded.value = false
  expandedKey.value = null
  selectedKey.value = null
  activeSourceFilter.value = 'all'
  emit('selectPreview', null)

  try {
    const parsedYear = yearInput.value.trim() ? Number.parseInt(yearInput.value.trim(), 10) : undefined
    const q = {
      ...buildDefaultQueryFromDetail(props.detail),
      target: targetKind.value,
      keyword: kw,
      artist_or_author: artistOrAuthor.value.trim() || undefined,
      year: parsedYear && Number.isFinite(parsedYear) ? parsedYear : undefined,
      providers: enabledProviderIds.value.length > 0 ? enabledProviderIds.value : undefined,
    }
    const resp: AggregateScrapeResponse = await toolApi.aggregateScrapers(q)
    candidates.value = resp.candidates
    mergedMeta.value = resp.merged?.metadata ?? null
    mergedFieldSources.value = resp.merged?.field_sources ?? {}
    const nextCache = new Map(detailCache.value)
    for (const [pid, detail] of Object.entries(resp.details_by_provider ?? {})) {
      if (detail) {
        nextCache.set(`${pid}::${detail.external_id}`, detail)
      }
    }
    detailCache.value = nextCache

    if (mergedMeta.value) {
      selectMergedPreview()
    } else if (candidates.value.length > 0) {
      void selectCandidatePreview(candidates.value[0]!)
    }
  } catch (err) {
    searchError.value = err instanceof Error ? err.message : String(err)
  } finally {
    searching.value = false
  }
}

async function resolveCandidateDetail(c: ScrapeCandidate): Promise<ScrapedMetadata | null> {
  const key = candKey(c)
  const cached = detailCache.value.get(key)
  if (cached) return cached
  loadingDetailKey.value = key
  try {
    const meta = await toolApi.getScraperDetail(c.provider_id, c.external_id, c.target)
    const next = new Map(detailCache.value)
    next.set(key, meta)
    detailCache.value = next
    return meta
  } catch (err) {
    searchError.value = err instanceof Error ? err.message : String(err)
    return null
  } finally {
    if (loadingDetailKey.value === key) {
      loadingDetailKey.value = null
    }
  }
}

async function handleToggleExpand(c: ScrapeCandidate): Promise<void> {
  const key = candKey(c)
  if (expandedKey.value === key) {
    expandedKey.value = null
    return
  }
  expandedKey.value = key
  await selectCandidatePreview(c)
}

async function handleFillCandidateAll(c: ScrapeCandidate): Promise<void> {
  const meta = await resolveCandidateDetail(c)
  if (!meta) return
  emit('fillAll', meta, fillMode.value)
  flashFeedback(`已整条回填「${meta.title || c.title}」`)
}

async function handleDirectApplyCandidate(c: ScrapeCandidate): Promise<void> {
  const meta = await resolveCandidateDetail(c)
  if (!meta) return
  emit('directApply', meta, fillMode.value)
}

function handleQuickFillFromCandidate(c: ScrapeCandidate, field: ScrapedFieldKey): void {
  const cached = detailCache.value.get(candKey(c))
  if (cached) {
    emit('fillField', field, cached)
    flashFeedback('已填入字段')
    return
  }
  emit('fillField', field, buildLightMetadata(c))
  flashFeedback('已填入字段')
}

function handleFillDetailField(field: ScrapedFieldKey, meta: ScrapedMetadata, label: string): void {
  emit('fillField', field, meta)
  flashFeedback(`已填入${label}`)
}

function handleFillMergedAll(): void {
  if (!mergedMeta.value) return
  emit('fillAll', mergedMeta.value, fillMode.value)
  flashFeedback('已将多源聚合结果回填到表单')
}

watch(
  () => props.detail?.entry.rel_path,
  async () => {
    const d = props.detail
    if (!d) {
      candidates.value = []
      mergedMeta.value = null
      mergedFieldSources.value = {}
      mergedExpanded.value = false
      selectedKey.value = null
      emit('selectPreview', null)
      return
    }
    const q = buildDefaultQueryFromDetail(d)
    keyword.value = q.keyword
    artistOrAuthor.value = q.artist_or_author ?? ''
    yearInput.value = q.year ? String(q.year) : ''
    candidates.value = []
    mergedMeta.value = null
    mergedFieldSources.value = {}
    mergedExpanded.value = false
    expandedKey.value = null
    selectedKey.value = null
    searchError.value = null
    emit('selectPreview', null)
    await loadProvidersForDetail(d)
  },
  { immediate: true },
)

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

interface ClickableFieldItem {
  key: ScrapedFieldKey
  label: string
  value: string
  multiline?: boolean
}

function extractClickableFields(meta: ScrapedMetadata): ClickableFieldItem[] {
  const list: ClickableFieldItem[] = []
  if (meta.title) list.push({ key: 'title', label: '标题', value: meta.title })
  if (meta.original_title && meta.original_title !== meta.title) {
    list.push({ key: 'original_title', label: '原作名', value: meta.original_title })
  }
  if (meta.creators.length > 0) {
    list.push({ key: 'creators', label: '作者/艺人', value: meta.creators.join(' / ') })
  }
  if (meta.illustrators_or_directors.length > 0) {
    list.push({
      key: 'directors',
      label: '画师/导演',
      value: meta.illustrators_or_directors.join(' / '),
    })
  }
  if (meta.series_or_album) {
    list.push({ key: 'series_or_album', label: '系列/专辑', value: meta.series_or_album })
  }
  if (meta.publisher_or_studio) {
    list.push({ key: 'publisher_or_studio', label: '出版/制作', value: meta.publisher_or_studio })
  }
  if (meta.published_date || meta.year) {
    list.push({ key: 'year', label: '年份/日期', value: meta.published_date || String(meta.year) })
  }
  if (meta.isbn) list.push({ key: 'isbn', label: 'ISBN', value: meta.isbn })
  if (meta.rating) list.push({ key: 'rating', label: '评分', value: String(meta.rating) })
  if (meta.language) list.push({ key: 'language', label: '语言', value: meta.language })
  if (meta.disc_or_season_number) {
    list.push({ key: 'disc_or_season', label: '季/碟号', value: String(meta.disc_or_season_number) })
  }
  if (meta.track_or_episode_number) {
    list.push({ key: 'track_or_episode', label: '集/音轨', value: String(meta.track_or_episode_number) })
  }
  if (meta.cast.length > 0) {
    list.push({ key: 'cast', label: '演员', value: meta.cast.slice(0, 8).join(' / ') })
  }
  if (meta.genres.length > 0) {
    list.push({ key: 'genres', label: '流派', value: meta.genres.join(' / ') })
  }
  if (meta.tags.length > 0) {
    list.push({ key: 'tags', label: '标签', value: meta.tags.slice(0, 12).join(' / ') })
  }
  if (meta.summary) {
    list.push({ key: 'summary', label: '简介', value: meta.summary, multiline: true })
  }
  if (meta.synced_lyrics || meta.plain_lyrics) {
    list.push({
      key: 'lyrics',
      label: meta.synced_lyrics ? '同步歌词(LRC)' : '纯文本歌词',
      value: (meta.synced_lyrics || meta.plain_lyrics || '').slice(0, 180),
      multiline: true,
    })
  }
  return list
}

defineExpose({
  runSearch,
})
</script>

<template>
  <aside
    class="flex h-full min-h-0 flex-col border-l border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/40"
  >
    <!-- Header & Search Bar -->
    <div class="shrink-0 space-y-2.5 border-b border-[var(--ctp-surface0)]/75 p-3">
      <div class="flex items-center justify-between gap-2">
        <div class="flex items-center gap-1.5 text-xs font-bold text-[var(--ctp-text)]">
          <LuSparkles class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
          <span>在线候选源</span>
          <span
            v-if="candidates.length > 0"
            class="rounded-full bg-[var(--ctp-surface0)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext1)]"
          >
            {{ candidates.length }}
          </span>
        </div>

        <!-- Fill Mode Toggle -->
        <div
          class="flex items-center rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-0.5 text-[10px]"
          title="控制整条回填到表单时的行为"
        >
          <button
            type="button"
            class="rounded-md px-2 py-0.5 font-semibold transition"
            :class="
              fillMode === 'overwrite'
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
            "
            @click="fillMode = 'overwrite'"
          >
            覆盖
          </button>
          <button
            type="button"
            class="rounded-md px-2 py-0.5 font-semibold transition"
            :class="
              fillMode === 'fill_missing'
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
            "
            @click="fillMode = 'fill_missing'"
          >
            仅补空
          </button>
        </div>
      </div>

      <!-- Search Inputs -->
      <div class="flex gap-1.5">
        <input
          v-model="keyword"
          type="text"
          :disabled="!detail"
          placeholder="标题 / 关键词 / ID…"
          class="h-8 min-w-0 flex-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-50"
          @keydown.enter.prevent="runSearch"
        />
        <button
          type="button"
          :disabled="!detail || searching || !keyword.trim()"
          class="inline-flex h-8 shrink-0 items-center gap-1 rounded-xl bg-[var(--ctp-accent)] px-3 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-45"
          @click="runSearch"
        >
          <LuLoader2 v-if="searching" class="h-3.5 w-3.5 animate-spin" />
          <LuSearch v-else class="h-3.5 w-3.5" />
          <span>搜索</span>
        </button>
      </div>

      <div class="grid grid-cols-3 gap-1.5">
        <input
          v-model="artistOrAuthor"
          type="text"
          :disabled="!detail"
          placeholder="作者/歌手（可选）"
          class="col-span-2 h-7 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 text-[11px] text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-50"
          @keydown.enter.prevent="runSearch"
        />
        <input
          v-model="yearInput"
          type="text"
          :disabled="!detail"
          placeholder="年份"
          class="h-7 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 text-[11px] text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-50"
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
          class="inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-medium transition"
          :class="
            !p.configured
              ? 'border-dashed border-amber-500/45 bg-amber-500/10 text-amber-400'
              : enabledProviderIds.includes(p.id)
                ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext0)]'
          "
          @click="toggleProvider(p)"
        >
          <LuKey v-if="!p.configured && p.requires_api_key" class="h-2.5 w-2.5" />
          <span>{{ p.name }}</span>
        </button>
      </div>

      <!-- Flash Feedback -->
      <div
        v-if="recentFlash"
        class="flex items-center gap-1.5 rounded-lg bg-emerald-500/15 px-2.5 py-1 text-[11px] font-medium text-emerald-500"
      >
        <LuCheck class="h-3 w-3 shrink-0" />
        <span class="truncate">{{ recentFlash }}</span>
      </div>
      <div
        v-if="searchError"
        class="rounded-lg bg-red-500/12 px-2.5 py-1.5 text-[11px] text-red-400"
      >
        {{ searchError }}
      </div>
    </div>

    <!-- Candidate Body -->
    <div class="min-h-0 flex-1 overflow-y-auto p-3 space-y-2.5">
      <div
        v-if="!detail"
        class="flex h-full flex-col items-center justify-center text-center text-xs text-[var(--ctp-subtext0)]"
      >
        <LuSparkles class="mb-2 h-6 w-6 opacity-40" />
        <p>在左栏选择一个文件开始刮削</p>
      </div>

      <div
        v-else-if="searching"
        class="flex flex-col items-center justify-center gap-2 py-12 text-xs text-[var(--ctp-subtext0)]"
      >
        <LuLoader2 class="h-5 w-5 animate-spin text-[var(--ctp-accent)]" />
        <span>正在并行查询 {{ enabledProviderIds.length }} 个数据源…</span>
      </div>

      <div
        v-else-if="candidates.length === 0"
        class="flex flex-col items-center justify-center gap-2 py-12 text-center text-xs text-[var(--ctp-subtext0)]"
      >
        <LuSearch class="h-6 w-6 opacity-35" />
        <p>点击上方「搜索」拉取在线候选</p>
        <p class="max-w-[220px] text-[11px] opacity-75">
          点候选的「← 整条填入」或点任意字段胶囊即可回填至中栏，确认后再保存
        </p>
        <button
          v-if="configuredProviders.length > 0 && keyword.trim()"
          type="button"
          class="mt-1 inline-flex items-center gap-1 rounded-xl border border-[var(--ctp-accent)]/60 bg-[var(--ctp-accent-soft)] px-3 py-1.5 text-[11px] font-semibold text-[var(--ctp-accent)] transition hover:opacity-90"
          @click="runSearch"
        >
          <LuSearch class="h-3 w-3" />
          <span>立即按当前标题搜索</span>
        </button>
      </div>

      <template v-else>
        <!-- Multi-source Merged Quick Card -->
        <div
          v-if="mergedMeta"
          class="cursor-pointer rounded-2xl border p-2.5 transition"
          :class="
            selectedKey === '__merged__'
              ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)]/50 ring-1 ring-[var(--ctp-accent)]/50'
              : 'border-[var(--ctp-accent)]/40 bg-[var(--ctp-accent-soft)]/30 hover:border-[var(--ctp-accent)]/75'
          "
          @click="selectMergedPreview"
        >
          <div class="flex items-center justify-between gap-2">
            <div class="flex items-center gap-1.5 text-[11px] font-bold text-[var(--ctp-accent)]">
              <LuLayers class="h-3.5 w-3.5" />
              <span>多源瀑布聚合结果</span>
              <span
                class="rounded-full bg-[var(--ctp-canvas)]/80 px-1.5 py-0.5 text-[9px] font-medium text-[var(--ctp-subtext1)]"
              >
                {{ extractClickableFields(mergedMeta).length }} 项字段
              </span>
            </div>
            <span
              v-if="selectedKey === '__merged__'"
              class="rounded-full bg-[var(--ctp-accent)] px-1.5 py-0.5 text-[9px] font-semibold text-[var(--ctp-on-accent)]"
            >
              预览中
            </span>
          </div>

          <div class="mt-2 flex items-start gap-2.5">
            <!-- Merged Cover Thumbnail (with broken-image fallback) -->
            <button
              type="button"
              class="group relative h-14 w-10 shrink-0 overflow-hidden rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]"
              :title="isCoverUsable(mergedMeta.cover_url) ? '点击仅填入这张封面' : '无可用封面'"
              :disabled="!isCoverUsable(mergedMeta.cover_url)"
              @click.stop="handleFillDetailField('cover', mergedMeta, '封面')"
            >
              <img
                v-if="isCoverUsable(mergedMeta.cover_url)"
                :src="resolveScraperCoverUrl(mergedMeta.cover_url)"
                alt=""
                referrerpolicy="no-referrer"
                class="h-full w-full object-cover"
                @error="markCoverBroken(mergedMeta.cover_url)"
              />
              <div
                v-else
                class="flex h-full w-full flex-col items-center justify-center gap-0.5 text-[var(--ctp-subtext0)]/55"
              >
                <LuImage class="h-3.5 w-3.5" />
                <span class="text-[8px]">无图</span>
              </div>
              <span
                v-if="isCoverUsable(mergedMeta.cover_url)"
                class="absolute inset-0 flex items-center justify-center bg-black/55 text-[9px] font-bold text-white opacity-0 transition group-hover:opacity-100"
              >
                用图
              </span>
            </button>

            <div class="min-w-0 flex-1">
              <div class="line-clamp-2 text-xs font-bold text-[var(--ctp-text)]">
                {{ mergedMeta.title || '—' }}
              </div>
              <div class="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-[var(--ctp-subtext0)]">
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

              <!-- Unified Action Bar (Matches Candidate Cards) -->
              <div class="mt-2 flex flex-wrap items-center gap-1.5">
                <button
                  type="button"
                  class="inline-flex items-center gap-1 rounded-lg bg-[var(--ctp-accent)] px-2 py-1 text-[10px] font-semibold text-[var(--ctp-on-accent)] transition hover:opacity-95"
                  title="把多源聚合结果填入左侧表单（暂不落盘）"
                  @click.stop="handleFillMergedAll"
                >
                  <LuArrowLeft class="h-3 w-3" />
                  <span>整条填入</span>
                </button>

                <button
                  type="button"
                  class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1 text-[10px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
                  @click.stop="mergedExpanded = !mergedExpanded; selectMergedPreview()"
                >
                  <LuChevronUp v-if="mergedExpanded" class="h-3 w-3" />
                  <LuChevronDown v-else class="h-3 w-3" />
                  <span>{{ mergedExpanded ? '收起字段' : '逐项挑选' }}</span>
                </button>

                <button
                  type="button"
                  class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-accent)]/50 bg-[var(--ctp-canvas)] px-2 py-1 text-[10px] font-semibold text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent)] hover:text-[var(--ctp-on-accent)]"
                  title="直接将聚合结果写入文件并保存"
                  @click.stop="emit('directApply', mergedMeta, fillMode)"
                >
                  <LuZap class="h-3 w-3" />
                  <span>直写</span>
                </button>
              </div>
            </div>
          </div>

          <!-- Collapsible Field-by-Field Drawer for Aggregated Result -->
          <div
            v-if="mergedExpanded"
            class="mt-2.5 border-t border-[var(--ctp-accent)]/25 pt-2"
            @click.stop
          >
            <div class="mb-1.5 text-[10px] font-medium text-[var(--ctp-subtext0)]">
              点击任意字段即可单独填入左侧表单：
            </div>
            <div class="flex flex-wrap gap-1.5">
              <button
                v-if="isCoverUsable(mergedMeta.cover_url)"
                type="button"
                class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-accent)]/40 bg-[var(--ctp-canvas)] px-2 py-1 text-[10px] font-semibold text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent)] hover:text-[var(--ctp-on-accent)]"
                @click="handleFillDetailField('cover', mergedMeta, '封面')"
              >
                <LuImage class="h-3 w-3" />
                <span>填入高清封面</span>
                <span v-if="mergedSourceLabelFor('cover')" class="opacity-75">
                  · {{ mergedSourceLabelFor('cover') }}
                </span>
              </button>
              <button
                v-for="field in extractClickableFields(mergedMeta)"
                :key="field.key"
                type="button"
                class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/90 px-2 py-1 text-left text-[10px] text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] hover:bg-[var(--ctp-accent-soft)]/40"
                :class="field.multiline ? 'w-full' : 'max-w-full'"
                :title="`点击填入「${field.label}」`"
                @click="handleFillDetailField(field.key, mergedMeta!, field.label)"
              >
                <span class="mr-1 font-bold text-[var(--ctp-accent)]">← {{ field.label }}</span>
                <span
                  v-if="mergedSourceLabelFor(field.key)"
                  class="mr-1 rounded bg-[var(--ctp-surface0)]/80 px-1 py-0.2 text-[9px] font-medium text-[var(--ctp-subtext0)]"
                >
                  {{ mergedSourceLabelFor(field.key) }}
                </span>
                <span
                  :class="
                    field.multiline
                      ? 'mt-0.5 block line-clamp-3 text-[var(--ctp-subtext1)]'
                      : 'text-[var(--ctp-subtext1)]'
                  "
                >
                  {{ field.value }}
                </span>
              </button>
            </div>
          </div>
        </div>

        <!-- Source Filter Bar -->
        <div v-if="sourceCounts.length > 1" class="flex flex-wrap gap-1">
          <button
            type="button"
            class="rounded-lg px-2 py-0.5 text-[10px] font-semibold transition"
            :class="
              activeSourceFilter === 'all'
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'bg-[var(--ctp-surface0)]/60 text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
            "
            @click="activeSourceFilter = 'all'"
          >
            全部 ({{ candidates.length }})
          </button>
          <button
            v-for="src in sourceCounts"
            :key="src.id"
            type="button"
            class="rounded-lg px-2 py-0.5 text-[10px] font-semibold transition"
            :class="
              activeSourceFilter === src.id
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'bg-[var(--ctp-surface0)]/60 text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
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
          class="cursor-pointer rounded-xl border p-2.5 transition"
          :class="
            selectedKey === candKey(c)
              ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)]/25 ring-1 ring-[var(--ctp-accent)]/45'
              : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] hover:border-[var(--ctp-accent)]/50'
          "
          @click="selectCandidatePreview(c)"
        >
          <div class="flex items-start gap-2.5">
            <!-- Clickable Cover Thumbnail -->
            <button
              type="button"
              class="group relative h-14 w-10 shrink-0 overflow-hidden rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]"
              :title="isCoverUsable(c.cover_url) ? '点击直接填入这张封面' : '无可用封面'"
              :disabled="!isCoverUsable(c.cover_url)"
              @click.stop="handleQuickFillFromCandidate(c, 'cover')"
            >
              <img
                v-if="isCoverUsable(c.cover_url)"
                :src="resolveScraperCoverUrl(c.cover_url)"
                alt=""
                referrerpolicy="no-referrer"
                class="h-full w-full object-cover"
                @error="markCoverBroken(c.cover_url)"
              />
              <div v-else class="flex h-full w-full flex-col items-center justify-center gap-0.5 text-[var(--ctp-subtext0)]/50">
                <LuImage class="h-3.5 w-3.5" />
                <span class="text-[8px]">无图</span>
              </div>
              <span
                v-if="isCoverUsable(c.cover_url)"
                class="absolute inset-0 flex items-center justify-center bg-black/55 text-[9px] font-bold text-white opacity-0 transition group-hover:opacity-100"
              >
                用图
              </span>
            </button>

            <!-- Candidate Info -->
            <div class="min-w-0 flex-1">
              <div class="flex items-start justify-between gap-1.5">
                <button
                  type="button"
                  class="line-clamp-2 text-left text-xs font-bold text-[var(--ctp-text)] transition hover:text-[var(--ctp-accent)]"
                  title="点击在右侧预览此条目详情"
                  @click.stop="selectCandidatePreview(c)"
                >
                  {{ c.title }}
                </button>
                <span
                  class="shrink-0 rounded-md px-1.5 py-0.5 text-[9px] font-semibold"
                  :class="
                    selectedKey === candKey(c)
                      ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                      : 'bg-[var(--ctp-surface0)]/80 text-[var(--ctp-subtext1)]'
                  "
                >
                  {{ c.provider_name }}
                </span>
              </div>

              <div class="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-[var(--ctp-subtext0)]">
                <button
                  v-if="c.subtitle"
                  type="button"
                  class="truncate hover:text-[var(--ctp-accent)] hover:underline"
                  title="点击单独填入作者/副标题"
                  @click.stop="handleQuickFillFromCandidate(c, 'creators')"
                >
                  {{ c.subtitle }}
                </button>
                <button
                  v-if="c.year"
                  type="button"
                  class="font-mono hover:text-[var(--ctp-accent)] hover:underline"
                  title="点击单独填入年份"
                  @click.stop="handleQuickFillFromCandidate(c, 'year')"
                >
                  {{ c.year }}
                </button>
              </div>

              <!-- Quick Action Buttons -->
              <div class="mt-2 flex flex-wrap items-center gap-1.5">
                <button
                  type="button"
                  :disabled="loadingDetailKey === candKey(c)"
                  class="inline-flex items-center gap-1 rounded-lg bg-[var(--ctp-accent-soft)] px-2 py-1 text-[10px] font-semibold text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent)] hover:text-[var(--ctp-on-accent)] disabled:opacity-50"
                  title="拉取完整详情并整条回填到左侧表单"
                  @click.stop="handleFillCandidateAll(c)"
                >
                  <LuLoader2 v-if="loadingDetailKey === candKey(c)" class="h-3 w-3 animate-spin" />
                  <LuArrowLeft v-else class="h-3 w-3" />
                  <span>整条填入</span>
                </button>

                <button
                  type="button"
                  class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2 py-1 text-[10px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
                  @click.stop="handleToggleExpand(c)"
                >
                  <LuChevronUp v-if="expandedKey === candKey(c)" class="h-3 w-3" />
                  <LuChevronDown v-else class="h-3 w-3" />
                  <span>逐项挑选</span>
                </button>

                <button
                  type="button"
                  :disabled="loadingDetailKey === candKey(c)"
                  class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2 py-1 text-[10px] font-medium text-[var(--ctp-subtext1)] transition hover:border-emerald-500/60 hover:text-emerald-500 disabled:opacity-50"
                  title="直接写入磁盘（含封面），无需再点保存"
                  @click.stop="handleDirectApplyCandidate(c)"
                >
                  <LuZap class="h-3 w-3" />
                  <span>直写</span>
                </button>
              </div>
            </div>
          </div>

          <!-- Expanded Field-by-Field Click-to-Fill Drawer -->
          <div
            v-if="expandedKey === candKey(c)"
            class="mt-2.5 border-t border-[var(--ctp-surface0)]/70 pt-2"
            @click.stop
          >
            <div
              v-if="loadingDetailKey === candKey(c)"
              class="flex items-center justify-center gap-1.5 py-3 text-[11px] text-[var(--ctp-subtext0)]"
            >
              <LuLoader2 class="h-3.5 w-3.5 animate-spin text-[var(--ctp-accent)]" />
              <span>正在拉取完整详情…</span>
            </div>
            <div v-else-if="detailCache.get(candKey(c))" class="space-y-1.5">
              <div class="text-[10px] font-medium text-[var(--ctp-subtext0)]">
                点击任意字段即可单独填入左侧表单：
              </div>
              <div class="flex flex-wrap gap-1.5">
                <button
                  v-if="isCoverUsable(detailCache.get(candKey(c))?.cover_url)"
                  type="button"
                  class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-accent)]/40 bg-[var(--ctp-accent-soft)]/50 px-2 py-1 text-[10px] font-semibold text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent)] hover:text-[var(--ctp-on-accent)]"
                  @click="handleFillDetailField('cover', detailCache.get(candKey(c))!, '封面')"
                >
                  <LuImage class="h-3 w-3" />
                  <span>填入高清封面</span>
                </button>
                <button
                  v-for="field in extractClickableFields(detailCache.get(candKey(c))!)"
                  :key="field.key"
                  type="button"
                  class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/80 px-2 py-1 text-left text-[10px] text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] hover:bg-[var(--ctp-accent-soft)]/40"
                  :class="field.multiline ? 'w-full' : 'max-w-full'"
                  :title="`点击填入「${field.label}」`"
                  @click="handleFillDetailField(field.key, detailCache.get(candKey(c))!, field.label)"
                >
                  <span class="mr-1 font-bold text-[var(--ctp-accent)]">← {{ field.label }}</span>
                  <span :class="field.multiline ? 'line-clamp-3 block mt-0.5 text-[var(--ctp-subtext1)]' : 'text-[var(--ctp-subtext1)]'">
                    {{ field.value }}
                  </span>
                </button>
              </div>
            </div>
          </div>
        </div>
      </template>
    </div>
  </aside>
</template>
