<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import {
  LuArrowUpDown,
  LuBookOpen,
  LuBookmark,
  LuCheck,
  LuCheckCheck,
  LuChevronDown,
  LuChevronRight,
  LuCloud,
  LuDownload,
  LuImage,
  LuPencil,
  LuPlay,
  LuRefreshCw,
  LuTrash2,
  LuUpload,
} from 'vue-icons-plus/lu'
import {
  MediaDetailTemplate,
  type MediaDetailSpec,
  type MediaDetailTag,
} from '@/components/domain'
import {
  AppCheckbox,
  MdButton,
  MdCard,
  MdDialog,
  MdEmptyState,
  MdMenuItem,
  MdTextField,
} from '@/components/ui'
import { api, withAuthUrl } from '@/domains/comic/api'
import ChapterMetadataModal from '@/domains/comic/components/ChapterMetadataModal.vue'
import ComicChapterRow from '@/domains/comic/components/ComicChapterRow.vue'
import SeriesMetadataModal from '@/domains/comic/components/SeriesMetadataModal.vue'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { onlineComicApi } from '@/domains/comic/onlineApi'
import { useSidebarStore } from '@/domains/comic/store'
import type { ChapterItemDto, SeriesDetailDto, ShelfDetailDto } from '@/domains/comic/types'
import { libraryRoute, parseItemRef, playRoute } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const dialog = useDialogStore()
const snackbar = useSnackbarStore()
const sidebarStore = useSidebarStore()
const upload = useUploadStore()

const detail = ref<SeriesDetailDto | null>(null)
const loading = ref(true)
const loadError = ref<string | null>(null)
const orderAsc = ref(true)
const groupByVolume = ref(true)
const chapterQuery = ref('')
const collapsedVolumes = ref<Record<string, boolean>>({})

const editSeriesOpen = ref(false)
const editingChapter = ref<ChapterItemDto | null>(null)
const shelfModalOpen = ref(false)
const allShelves = ref<ShelfDetailDto[]>([])
const selectedShelfIds = ref<string[]>([])
const savingShelves = ref(false)
const refreshingTrackedToc = ref(false)
const cachingSeries = ref(false)
const cachingChapterIds = ref<Set<string>>(new Set())

const coverInputRef = ref<HTMLInputElement | null>(null)
const chapterUploadRef = ref<HTMLInputElement | null>(null)
let detailSeq = 0

const seriesId = computed(() => {
  const parsed = parseItemRef(route, 'comic')
  if (parsed && parsed.scope === 'local') return parsed.id
  return typeof route.params.id === 'string' ? route.params.id : ''
})

watch(seriesId, () => {
  collapsedVolumes.value = {}
  chapterQuery.value = ''
})

const isManagedSeries = computed(() =>
  sidebarStore.libraries.some(
    (l) => l.id === detail.value?.series.library_id && l.kind === 'managed_upload',
  ),
)

const isTrackedSeries = computed(() =>
  Boolean(detail.value?.online_tracking || detail.value?.series.source_dir.startsWith('online_tracked:')),
)

const trackedCacheStats = computed(() => {
  const chapters = detail.value?.chapters ?? []
  const tracked = chapters.filter((ch) => Boolean(ch.online_tracking))
  const cached = tracked.filter((ch) => Boolean(ch.online_tracking?.cached)).length
  return {
    cached,
    total: tracked.length || chapters.length,
    uncached: Math.max(0, tracked.length - cached),
  }
})

const statusLabel = computed(() => {
  const s = detail.value?.series.status
  if (s === 'completed') return t('series.statusCompleted')
  if (s === 'hiatus') return t('series.statusHiatus')
  return t('series.statusOngoing')
})

const directionLabel = computed(() => {
  const dir = detail.value?.series.reading_direction
  if (dir === 'ltr') return t('series.directionLtr')
  if (dir === 'webtoon') return t('series.directionWebtoon')
  return t('series.directionRtl')
})

const heroSubtitle = computed(() => {
  if (!detail.value) return ''
  const s = detail.value.series
  const creators = [...s.writers, ...s.pencillers].filter(Boolean)
  const parts: string[] = []
  if (creators.length > 0) parts.push(creators.join(' · '))
  parts.push(statusLabel.value)
  if (s.year) parts.push(String(s.year))
  return parts.join(' · ')
})

const overallProgressPercent = computed<number | null>(() => {
  if (!detail.value) return null
  const total = detail.value.series.chapter_count
  if (total <= 0) return null
  if (detail.value.completed_chapters <= 0 && !detail.value.continue_reading) return null
  return Math.min(100, Math.round((detail.value.completed_chapters / total) * 100))
})

const allCompleted = computed(
  () =>
    Boolean(
      detail.value &&
        detail.value.series.chapter_count > 0 &&
        detail.value.completed_chapters >= detail.value.series.chapter_count,
    ),
)

const primaryActionLabel = computed(() => {
  if (!detail.value) return t('series.startReading')
  if (detail.value.continue_reading) {
    return t('series.continueChapter', {
      chapter: detail.value.continue_reading.chapter_number,
      page: detail.value.continue_reading.current_page,
    })
  }
  return t('series.startReading')
})

const detailTags = computed<MediaDetailTag[]>(() => {
  if (!detail.value) return []
  const s = detail.value.series
  const out: MediaDetailTag[] = []
  for (const genre of s.genres) {
    out.push({ label: genre, value: `g:${genre}`, to: libraryRoute('comic', { genre }) })
  }
  for (const tag of s.tags) {
    out.push({ label: `#${tag}`, value: `t:${tag}`, to: libraryRoute('comic', { tag }) })
  }
  for (const writer of s.writers) {
    out.push({
      label: `${t('series.writers')}: ${writer}`,
      value: `w:${writer}`,
      to: libraryRoute('comic', { writer }),
    })
  }
  for (const penciller of s.pencillers) {
    out.push({
      label: `${t('series.pencillers')}: ${penciller}`,
      value: `p:${penciller}`,
      to: libraryRoute('comic', { penciller }),
    })
  }
  return out
})

const detailSpecs = computed<MediaDetailSpec[]>(() => {
  if (!detail.value) return []
  const s = detail.value.series
  const specs: MediaDetailSpec[] = [
    { label: t('series.specStatus'), value: statusLabel.value },
    { label: t('series.specDirection'), value: directionLabel.value },
    {
      label: t('series.specProgress'),
      value: `${detail.value.completed_chapters} / ${s.chapter_count}`,
    },
  ]
  if (s.year) specs.push({ label: t('series.specYear'), value: String(s.year) })
  if (s.age_rating) specs.push({ label: t('series.specAgeRating'), value: s.age_rating })
  if (s.publisher) specs.push({ label: t('series.specPublisher'), value: s.publisher })
  if (s.alt_titles.length > 0) {
    specs.push({ label: t('series.specAltTitles'), value: s.alt_titles.join(' / ') })
  }
  if (auth.isAdmin && s.source_dir) {
    specs.push({ label: t('series.specSourceDir'), value: s.source_dir, mono: true })
  }
  return specs
})

const filteredSortedChapters = computed<ChapterItemDto[]>(() => {
  if (!detail.value) return []
  const q = chapterQuery.value.trim().toLowerCase()
  const base = orderAsc.value ? [...detail.value.chapters] : [...detail.value.chapters].reverse()
  if (!q) return base
  return base.filter(
    (ch) =>
      ch.title.toLowerCase().includes(q) ||
      ch.number_label.toLowerCase().includes(q) ||
      chapterKindBadge(ch).toLowerCase().includes(q),
  )
})

const hasAnyVolumes = computed(() =>
  Boolean(detail.value?.chapters.some((c) => c.volume_number !== null)),
)

interface VolumeGroup {
  key: string
  volumeNumber: number | null
  title: string
  chapters: ChapterItemDto[]
}

const volumeGroups = computed<VolumeGroup[]>(() => {
  const groups = new Map<string, VolumeGroup>()
  for (const ch of filteredSortedChapters.value) {
    const key = ch.volume_number !== null ? String(ch.volume_number) : 'unassigned'
    let group = groups.get(key)
    if (!group) {
      group = {
        key,
        volumeNumber: ch.volume_number,
        title:
          ch.volume_number !== null
            ? t('series.volumeGroupTitle', { vol: ch.volume_number })
            : t('series.unassignedVolume'),
        chapters: [],
      }
      groups.set(key, group)
    }
    group.chapters.push(ch)
  }
  return Array.from(groups.values())
})

function showError(err: unknown): void {
  snackbar.show({
    message: err instanceof Error ? err.message : String(err),
    tone: 'error',
  })
}

async function loadDetail(): Promise<void> {
  const id = seriesId.value
  if (!id) {
    detail.value = null
    loading.value = false
    loadError.value = t('series.detailLoadFailed')
    return
  }
  const seq = ++detailSeq
  loading.value = true
  loadError.value = null
  try {
    const res = await api.getSeriesDetail(id)
    if (seq !== detailSeq) return
    detail.value = res
  } catch (err) {
    if (seq !== detailSeq) return
    loadError.value = err instanceof Error ? err.message : t('series.detailLoadFailed')
  } finally {
    if (seq === detailSeq) {
      loading.value = false
    }
  }
}

async function refreshPage(): Promise<void> {
  await Promise.allSettled([loadDetail(), sidebarStore.refresh()])
}

onMounted(() => {
  void loadDetail()
})
watch([seriesId, () => upload.catalogVersion], () => {
  void loadDetail()
})

function toggleVolumeCollapse(key: string): void {
  collapsedVolumes.value[key] = !collapsedVolumes.value[key]
}

function chapterKindBadge(ch: ChapterItemDto): string {
  if (ch.kind === 'volume') return t('series.kindVolume', { num: ch.number_label })
  if (ch.kind === 'special') return t('series.kindSpecial', { num: ch.number_label })
  return t('series.kindChapter', { num: ch.number_label })
}

function chapterMetaText(ch: ChapterItemDto): string {
  if (ch.online_tracking) {
    if (ch.online_tracking.cached) {
      return `${t('series.pagesCount', { count: ch.page_count })} · ${t('online.trackedCached')}`
    }
    return t('online.trackedUncached')
  }
  return t('series.pagesCount', { count: ch.page_count })
}

function chapterProgressPercent(ch: ChapterItemDto): number | null {
  if (ch.is_completed) return 100
  if (ch.current_page <= 0 || ch.page_count <= 0) return null
  return Math.min(100, Math.round((ch.current_page / ch.page_count) * 100))
}

function primaryReadAction(): void {
  if (!detail.value) return
  if (detail.value.continue_reading) {
    void router.push(
      playRoute(
        { scope: 'local', domain: 'comic', id: detail.value.continue_reading.chapter_id },
        {
          position: detail.value.continue_reading.current_page,
          duration: detail.value.continue_reading.total_pages,
        },
      ),
    )
    return
  }
  const first = detail.value.chapters.find((c) => !c.is_missing)
  if (first) {
    void router.push(
      playRoute(
        { scope: 'local', domain: 'comic', id: first.id },
        { position: 1, duration: first.page_count },
      ),
    )
  }
}

function openChapter(ch: ChapterItemDto): void {
  if (ch.is_missing) return
  const startPage = ch.current_page > 0 && !ch.is_completed ? ch.current_page : 1
  void router.push(
    playRoute(
      { scope: 'local', domain: 'comic', id: ch.id },
      { position: startPage, duration: ch.page_count },
    ),
  )
}

async function toggleFavorite(): Promise<void> {
  if (!detail.value) return
  const next = !detail.value.is_favorite
  detail.value.is_favorite = next
  try {
    await api.setFavorite(detail.value.series.id, next)
  } catch (err) {
    detail.value.is_favorite = !next
    showError(err)
  }
}

async function openShelfDialog(): Promise<void> {
  if (!detail.value) return
  try {
    allShelves.value = await api.listShelves()
    selectedShelfIds.value = [...detail.value.shelf_ids]
    shelfModalOpen.value = true
  } catch (err) {
    showError(err)
  }
}

function setShelfChecked(id: string, checked: boolean): void {
  if (checked) {
    if (!selectedShelfIds.value.includes(id)) selectedShelfIds.value.push(id)
  } else {
    selectedShelfIds.value = selectedShelfIds.value.filter((x) => x !== id)
  }
}

async function saveShelvesSelection(): Promise<void> {
  if (!detail.value || savingShelves.value) return
  savingShelves.value = true
  try {
    await api.setSeriesShelves(detail.value.series.id, selectedShelfIds.value)
    detail.value.shelf_ids = [...selectedShelfIds.value]
    shelfModalOpen.value = false
    await sidebarStore.refresh()
  } catch (err) {
    showError(err)
  } finally {
    savingShelves.value = false
  }
}

async function onSeriesMetadataSaved(): Promise<void> {
  await Promise.allSettled([loadDetail(), sidebarStore.refresh()])
}

async function toggleChapterRead(ch: ChapterItemDto): Promise<void> {
  try {
    await api.setChapterCompleted(ch.id, !ch.is_completed)
    await loadDetail()
  } catch (err) {
    showError(err)
  }
}

async function handleMarkPreviousRead(ch: ChapterItemDto): Promise<void> {
  try {
    await api.markPreviousRead(ch.id)
    await loadDetail()
  } catch (err) {
    showError(err)
  }
}

async function handleMarkSeriesRead(completed: boolean): Promise<void> {
  if (!detail.value) return
  try {
    await api.markSeriesRead(detail.value.series.id, completed)
    await loadDetail()
  } catch (err) {
    showError(err)
  }
}

async function handleMarkVolumeRead(volNum: number): Promise<void> {
  if (!detail.value) return
  try {
    await api.markVolumeRead(detail.value.series.id, volNum, true)
    await loadDetail()
  } catch (err) {
    showError(err)
  }
}

async function onPickCover(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file || !detail.value) {
    input.value = ''
    return
  }
  try {
    await api.uploadSeriesCover(detail.value.series.id, file)
    await loadDetail()
  } catch (err) {
    showError(err)
  } finally {
    input.value = ''
  }
}

function onUploadChaptersToSeries(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0 && detail.value) {
    upload.enqueueFiles(input.files, { targetSeriesId: detail.value.series.id })
    input.value = ''
  }
}

async function handleDeleteSeries(): Promise<void> {
  if (!detail.value) return
  const ok = await dialog.confirm({
    title: t('series.deleteConfirmTitle'),
    description: t('series.deleteConfirmDesc'),
    danger: true,
    confirmText: t('common.delete'),
  })
  if (!ok) return
  try {
    await api.deleteSeries(detail.value.series.id)
    upload.notifyCatalogChanged()
    await router.replace(libraryRoute('comic'))
  } catch (err) {
    showError(err)
  }
}

async function handleDeleteChapter(ch: ChapterItemDto): Promise<void> {
  const ok = await dialog.confirm({
    title: t('series.deleteChapterConfirmTitle'),
    description: t('series.deleteChapterConfirmDesc', {
      title: ch.title || chapterKindBadge(ch),
    }),
    danger: true,
    confirmText: t('common.delete'),
  })
  if (!ok) return
  try {
    await api.deleteChapter(ch.id)
    await sidebarStore.refresh()
    try {
      detail.value = await api.getSeriesDetail(seriesId.value)
      upload.notifyCatalogChanged()
    } catch {
      upload.notifyCatalogChanged()
      await router.replace(libraryRoute('comic'))
    }
  } catch (err) {
    showError(err)
  }
}

async function handleRefreshTrackedToc(): Promise<void> {
  if (!detail.value || !isTrackedSeries.value || refreshingTrackedToc.value) return
  refreshingTrackedToc.value = true
  try {
    const updated = await onlineComicApi.refreshTrackedSeries(detail.value.series.id)
    detail.value = updated
    upload.notifyCatalogChanged()
    void sidebarStore.refresh()
    snackbar.show({
      message: t('online.trackedTocRefreshed', { count: updated.series.chapter_count }),
    })
  } catch (err) {
    showError(err)
  } finally {
    refreshingTrackedToc.value = false
  }
}

async function handleCacheAllChapters(): Promise<void> {
  if (!detail.value || !isTrackedSeries.value || cachingSeries.value) return
  cachingSeries.value = true
  try {
    const report = await onlineComicApi.cacheTrackedSeries(detail.value.series.id)
    await loadDetail()
    upload.notifyCatalogChanged()
    if (report.failed.length > 0) {
      snackbar.show({
        message: t('online.cacheSeriesPartialToast', {
          cached: report.cached_now,
          failed: report.failed.length,
        }),
        tone: 'error',
      })
    } else {
      snackbar.show({
        message: t('online.cacheSeriesReportToast', {
          cached: report.cached_now,
          skipped: report.already_cached,
        }),
      })
    }
  } catch (err) {
    showError(err)
  } finally {
    cachingSeries.value = false
  }
}

async function handleCacheChapter(ch: ChapterItemDto): Promise<void> {
  if (!auth.isAdmin || cachingChapterIds.value.has(ch.id)) return
  const next = new Set(cachingChapterIds.value)
  next.add(ch.id)
  cachingChapterIds.value = next
  const label = ch.title || chapterKindBadge(ch)
  snackbar.show({ message: t('online.cachingChapterToast', { title: label }) })
  try {
    const updated = await onlineComicApi.cacheTrackedChapter(ch.id)
    if (detail.value) {
      detail.value = {
        ...detail.value,
        chapters: detail.value.chapters.map((item) => (item.id === updated.id ? updated : item)),
      }
    }
    snackbar.show({
      message: t('online.chapterCachedToast', {
        title: updated.title || label,
        pages: updated.page_count,
      }),
    })
  } catch (err) {
    showError(err)
  } finally {
    const done = new Set(cachingChapterIds.value)
    done.delete(ch.id)
    cachingChapterIds.value = done
  }
}
</script>

<template>
  <div class="px-4 py-6 sm:px-6 md:px-8">
    <input
      v-if="auth.isAdmin && isManagedSeries"
      ref="chapterUploadRef"
      type="file"
      multiple
      accept=".cbz,.zip,.epub"
      class="hidden"
      @change="onUploadChaptersToSeries"
    />
    <input
      v-if="auth.isAdmin"
      ref="coverInputRef"
      type="file"
      accept="image/*"
      class="hidden"
      @change="onPickCover"
    />

    <MediaDetailTemplate
      :title="detail?.series.title || ''"
      :subtitle="heroSubtitle"
      :cover="detail?.cover_url ? withAuthUrl(detail.cover_url) : null"
      aspect="poster"
      cover-style="book"
      :fallback-icon="LuBookOpen"
      :show-back="true"
      :back-label="t('series.backToLibrary')"
      :back-to="libraryRoute('comic')"
      :progress-percent="overallProgressPercent"
      :progress-label="
        detail
          ? t('series.progressChapters', {
              done: detail.completed_chapters,
              total: detail.series.chapter_count,
            })
          : ''
      "
      :primary-action-label="primaryActionLabel"
      :primary-action-icon="LuPlay"
      :primary-action-disabled="!detail || detail.chapters.length === 0"
      :secondary-action-label="t('series.addToShelf')"
      :secondary-action-icon="LuBookmark"
      :show-favorite="Boolean(detail)"
      :favorite="Boolean(detail?.is_favorite)"
      :description="detail?.series.summary || null"
      :tags="detailTags"
      :specs="detailSpecs"
      :specs-title="t('series.specsTitle')"
      :loading="loading && !detail"
      :error="loadError"
      @primary-action="primaryReadAction"
      @secondary-action="openShelfDialog"
      @toggle-favorite="toggleFavorite"
      @retry="loadDetail"
    >
      <template v-if="detail && isTrackedSeries" #hero-meta>
        <div class="mt-1 flex flex-wrap items-center gap-2">
          <span
            class="text-label-medium inline-flex items-center gap-1.5 rounded-corner-full bg-secondary-container px-2.5 py-0.5 text-on-secondary-container"
          >
            <LuCloud class="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
            <span>
              {{
                t('online.trackedBadgeStatus', {
                  cached: trackedCacheStats.cached,
                  total: trackedCacheStats.total,
                })
              }}
            </span>
          </span>
        </div>
      </template>

      <template v-if="detail" #overflow-menu="{ close }">
        <template v-if="auth.isAdmin && isTrackedSeries">
          <MdMenuItem
            :label="
              refreshingTrackedToc ? t('online.refreshingTrackedToc') : t('online.refreshTrackedToc')
            "
            :icon="LuRefreshCw"
            :disabled="refreshingTrackedToc"
            @select="
              close();
              handleRefreshTrackedToc()
            "
          />
          <MdMenuItem
            v-if="trackedCacheStats.uncached > 0"
            :label="
              cachingSeries ? t('online.cachingAllChapters') : t('online.cacheAllChapters')
            "
            :icon="LuCloud"
            :disabled="cachingSeries"
            @select="
              close();
              handleCacheAllChapters()
            "
          />
        </template>
        <MdMenuItem
          v-if="auth.isAdmin && isManagedSeries"
          :label="t('series.uploadToSeries')"
          :icon="LuUpload"
          @select="
            close();
            chapterUploadRef?.click()
          "
        />
        <MdMenuItem
          :label="allCompleted ? t('series.markAllUnread') : t('series.markAllRead')"
          :icon="LuCheckCheck"
          @select="
            close();
            handleMarkSeriesRead(!allCompleted)
          "
        />
        <MdMenuItem
          v-if="auth.isAdmin"
          :label="t('series.editMetadata')"
          :icon="LuPencil"
          @select="
            close();
            editSeriesOpen = true
          "
        />
        <MdMenuItem
          v-if="auth.isAdmin"
          :label="t('series.changeCover')"
          :icon="LuImage"
          @select="
            close();
            coverInputRef?.click()
          "
        />
        <MdMenuItem
          :label="globalT('common.refresh')"
          :icon="LuRefreshCw"
          @select="
            close();
            refreshPage()
          "
        />
      </template>

      <template v-if="detail && auth.isAdmin" #overflow-danger="{ close }">
        <MdMenuItem
          :label="t('series.deleteSeries')"
          :icon="LuTrash2"
          destructive
          @select="
            close();
            handleDeleteSeries()
          "
        />
      </template>

      <!-- Chapters Section -->
      <section v-if="detail" class="space-y-4">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="flex items-center gap-2.5">
            <h2 class="text-title-large text-on-surface">
              {{ t('series.chaptersHeader') }}
            </h2>
            <span
              class="text-label-medium rounded-corner-full bg-surface-container-high px-2.5 py-0.5 tabular-nums text-on-surface-variant"
            >
              {{ detail.completed_chapters }} / {{ detail.series.chapter_count }}
            </span>
          </div>

          <div class="flex flex-wrap items-center gap-2.5">
            <div v-if="detail.chapters.length > 10" class="w-48">
              <MdTextField
                v-model="chapterQuery"
                type="search"
                size="sm"
                clearable
                :placeholder="t('series.filterChaptersPlaceholder')"
              />
            </div>

            <AppCheckbox
              v-if="hasAnyVolumes"
              v-model="groupByVolume"
              :label="t('series.groupByVolume')"
            />

            <MdButton
              variant="outlined"
              size="sm"
              :icon="LuArrowUpDown"
              @click="orderAsc = !orderAsc"
            >
              {{ orderAsc ? t('series.orderAsc') : t('series.orderDesc') }}
            </MdButton>
          </div>
        </div>

        <MdEmptyState
          v-if="filteredSortedChapters.length === 0"
          compact
          :icon="LuBookOpen"
          :title="t('series.noMatchingChapters')"
        />

        <!-- Volume Grouped Mode -->
        <div v-else-if="hasAnyVolumes && groupByVolume" class="space-y-3">
          <MdCard
            v-for="group in volumeGroups"
            :key="group.key"
            variant="filled"
            padding="none"
            class="overflow-hidden"
          >
            <div
              class="flex cursor-pointer items-center justify-between gap-2 bg-surface-container-high/60 px-4 py-2.5"
              @click="toggleVolumeCollapse(group.key)"
            >
              <div class="flex items-center gap-2 text-title-small text-on-surface">
                <LuChevronRight v-if="collapsedVolumes[group.key]" class="h-4 w-4" aria-hidden="true" />
                <LuChevronDown v-else class="h-4 w-4" aria-hidden="true" />
                <span>{{ group.title }}</span>
                <span class="text-body-small tabular-nums text-on-surface-variant">
                  ({{ group.chapters.length }})
                </span>
              </div>

              <MdButton
                v-if="group.volumeNumber !== null"
                variant="text"
                size="sm"
                @click.stop="handleMarkVolumeRead(group.volumeNumber)"
              >
                {{ t('series.markVolumeRead') }}
              </MdButton>
            </div>

            <div
              v-if="!collapsedVolumes[group.key]"
              class="divide-y divide-outline-variant/50"
            >
              <ComicChapterRow
                v-for="ch in group.chapters"
                :key="ch.id"
                :badge="chapterKindBadge(ch)"
                :title="ch.title || chapterKindBadge(ch)"
                :meta-text="chapterMetaText(ch)"
                :progress-text="
                  ch.current_page > 0 && !ch.is_completed
                    ? t('series.readProgress', { page: ch.current_page, total: ch.page_count })
                    : ''
                "
                :progress-percent="chapterProgressPercent(ch)"
                :completed="ch.is_completed"
                :completed-label="t('series.completedBadge')"
                :missing="ch.is_missing"
                :missing-label="t('series.missingBadge')"
                :active="detail.continue_reading?.chapter_id === ch.id"
                :menu-aria-label="t('series.chapterActions')"
                @open="openChapter(ch)"
              >
                <template #menu="{ close }">
                  <MdMenuItem
                    v-if="auth.isAdmin && ch.online_tracking && !ch.online_tracking.cached"
                    :label="t('online.cacheChapterBtn')"
                    :icon="LuDownload"
                    :disabled="cachingChapterIds.has(ch.id)"
                    @select="
                      close();
                      handleCacheChapter(ch)
                    "
                  />
                  <MdMenuItem
                    :label="ch.is_completed ? t('series.markUnread') : t('series.markRead')"
                    :icon="LuCheck"
                    @select="
                      close();
                      toggleChapterRead(ch)
                    "
                  />
                  <MdMenuItem
                    :label="t('series.markPrevRead')"
                    :icon="LuCheckCheck"
                    @select="
                      close();
                      handleMarkPreviousRead(ch)
                    "
                  />
                  <MdMenuItem
                    v-if="auth.isAdmin"
                    :label="t('series.editChapter')"
                    :icon="LuPencil"
                    @select="
                      close();
                      editingChapter = ch
                    "
                  />
                  <MdMenuItem
                    v-if="auth.isAdmin"
                    :label="t('series.deleteChapter')"
                    :icon="LuTrash2"
                    destructive
                    @select="
                      close();
                      handleDeleteChapter(ch)
                    "
                  />
                </template>
              </ComicChapterRow>
            </div>
          </MdCard>
        </div>

        <!-- Flat Chapter List Mode (reusing exact same ComicChapterRow) -->
        <MdCard v-else variant="filled" padding="none" class="overflow-hidden">
          <div class="divide-y divide-outline-variant/50">
            <ComicChapterRow
              v-for="ch in filteredSortedChapters"
              :key="ch.id"
              :badge="chapterKindBadge(ch)"
              :title="ch.title || chapterKindBadge(ch)"
              :meta-text="chapterMetaText(ch)"
              :progress-text="
                ch.current_page > 0 && !ch.is_completed
                  ? t('series.readProgress', { page: ch.current_page, total: ch.page_count })
                  : ''
              "
              :progress-percent="chapterProgressPercent(ch)"
              :completed="ch.is_completed"
              :completed-label="t('series.completedBadge')"
              :missing="ch.is_missing"
              :missing-label="t('series.missingBadge')"
              :active="detail.continue_reading?.chapter_id === ch.id"
              :menu-aria-label="t('series.chapterActions')"
              @open="openChapter(ch)"
            >
              <template #menu="{ close }">
                <MdMenuItem
                  v-if="auth.isAdmin && ch.online_tracking && !ch.online_tracking.cached"
                  :label="t('online.cacheChapterBtn')"
                  :icon="LuDownload"
                  :disabled="cachingChapterIds.has(ch.id)"
                  @select="
                    close();
                    handleCacheChapter(ch)
                  "
                />
                <MdMenuItem
                  :label="ch.is_completed ? t('series.markUnread') : t('series.markRead')"
                  :icon="LuCheck"
                  @select="
                    close();
                    toggleChapterRead(ch)
                  "
                />
                <MdMenuItem
                  :label="t('series.markPrevRead')"
                  :icon="LuCheckCheck"
                  @select="
                    close();
                    handleMarkPreviousRead(ch)
                  "
                />
                <MdMenuItem
                  v-if="auth.isAdmin"
                  :label="t('series.editChapter')"
                  :icon="LuPencil"
                  @select="
                    close();
                    editingChapter = ch
                  "
                />
                <MdMenuItem
                  v-if="auth.isAdmin"
                  :label="t('series.deleteChapter')"
                  :icon="LuTrash2"
                  destructive
                  @select="
                    close();
                    handleDeleteChapter(ch)
                  "
                />
              </template>
            </ComicChapterRow>
          </div>
        </MdCard>
      </section>
    </MediaDetailTemplate>

    <!-- Modals -->
    <SeriesMetadataModal
      :open="editSeriesOpen"
      :series="detail?.series ?? null"
      @close="editSeriesOpen = false"
      @saved="onSeriesMetadataSaved"
    />

    <ChapterMetadataModal
      :open="Boolean(editingChapter)"
      :chapter="editingChapter"
      @close="editingChapter = null"
      @saved="loadDetail"
    />

    <MdDialog
      :open="shelfModalOpen"
      :title="t('shelves.selectShelvesTitle')"
      size="sm"
      @close="shelfModalOpen = false"
    >
      <div class="space-y-2.5">
        <p v-if="allShelves.length === 0" class="text-body-medium text-on-surface-variant">
          {{ t('shelves.emptyShelves') }}
        </p>
        <div
          v-for="sh in allShelves"
          :key="sh.shelf.id"
          class="rounded-corner-md bg-surface-container-low p-3"
        >
          <AppCheckbox
            :model-value="selectedShelfIds.includes(sh.shelf.id)"
            :label="sh.shelf.name"
            :description="sh.shelf.description || undefined"
            @update:model-value="setShelfChecked(sh.shelf.id, $event)"
          />
        </div>
      </div>

      <template #footer>
        <MdButton variant="text" :disabled="savingShelves" @click="shelfModalOpen = false">
          {{ t('common.cancel') }}
        </MdButton>
        <MdButton
          variant="filled"
          :loading="savingShelves"
          :disabled="savingShelves"
          @click="saveShelvesSelection"
        >
          {{ t('common.save') }}
        </MdButton>
      </template>
    </MdDialog>
  </div>
</template>
