import { computed, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import type { MediaDetailSpec, MediaDetailTag } from '@/components/domain'
import { api } from '@/domains/comic/api'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { onlineComicApi } from '@/domains/comic/onlineApi'
import { useSidebarStore } from '@/domains/comic/store'
import type { ChapterItemDto, SeriesDetailDto, ShelfDetailDto } from '@/domains/comic/types'
import { libraryRoute, parseItemRef, playRoute } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'

export function useComicSeriesDetail() {
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

  const editSeriesOpen = ref(false)
  const editingChapter = ref<ChapterItemDto | null>(null)
  const shelfModalOpen = ref(false)
  const allShelves = ref<ShelfDetailDto[]>([])
  const selectedShelfIds = ref<string[]>([])
  const savingShelves = ref(false)
  const refreshingTrackedToc = ref(false)
  const cachingSeries = ref(false)
  const cachingChapterIds = ref<Set<string>>(new Set())

  let detailSeq = 0

  const seriesId = computed(() => {
    const parsed = parseItemRef(route, 'comic')
    if (parsed && parsed.scope === 'local') return parsed.id
    return typeof route.params.id === 'string' ? route.params.id : ''
  })

  const isManagedSeries = computed(() =>
    sidebarStore.libraries.some(
      (l) => l.id === detail.value?.series.library_id && l.kind === 'managed_upload',
    ),
  )

  const isTrackedSeries = computed(() =>
    Boolean(
      detail.value?.online_tracking ||
        detail.value?.chapters.some((ch) => Boolean(ch.online_tracking)),
    ),
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

  const allCompleted = computed(() =>
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

  function showError(err: unknown): void {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }

  function chapterLabel(ch: ChapterItemDto): string {
    if (ch.title) return ch.title
    if (ch.kind === 'volume') return t('series.kindVolume', { num: ch.number_label })
    if (ch.kind === 'special') return t('series.kindSpecial', { num: ch.number_label })
    return t('series.kindChapter', { num: ch.number_label })
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
      description: t('series.deleteChapterConfirmDesc', { title: chapterLabel(ch) }),
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

  async function handleUnbindSeries(): Promise<void> {
    if (!detail.value) return
    const s = detail.value.series
    const ok = await dialog.confirm({
      title: globalT('pool.unbindConfirmTitle'),
      message: isTrackedSeries.value
        ? globalT('pool.unbindOnlineConfirmMessage', { title: s.title })
        : globalT('pool.unbindConfirmMessage', { title: s.title }),
      danger: true,
      confirmText: globalT('pool.unbindBtn'),
    })
    if (!ok) return
    try {
      await api.unbindItem(s.id)
      snackbar.show(globalT('pool.unboundToast'))
      upload.notifyCatalogChanged()
      await sidebarStore.refresh()
      await router.replace(libraryRoute('comic'))
    } catch (err) {
      showError(err)
    }
  }

  async function handleCacheChapter(ch: ChapterItemDto): Promise<void> {
    if (cachingChapterIds.value.has(ch.id)) return
    const next = new Set(cachingChapterIds.value)
    next.add(ch.id)
    cachingChapterIds.value = next
    const label = chapterLabel(ch)
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

  return {
    detail,
    loading,
    loadError,
    editSeriesOpen,
    editingChapter,
    shelfModalOpen,
    allShelves,
    selectedShelfIds,
    savingShelves,
    refreshingTrackedToc,
    cachingSeries,
    cachingChapterIds,
    isManagedSeries,
    isTrackedSeries,
    trackedCacheStats,
    heroSubtitle,
    overallProgressPercent,
    allCompleted,
    primaryActionLabel,
    detailTags,
    detailSpecs,
    loadDetail,
    refreshPage,
    primaryReadAction,
    openChapter,
    toggleFavorite,
    openShelfDialog,
    setShelfChecked,
    saveShelvesSelection,
    onSeriesMetadataSaved,
    toggleChapterRead,
    handleMarkPreviousRead,
    handleMarkSeriesRead,
    handleMarkVolumeRead,
    onPickCover,
    onUploadChaptersToSeries,
    handleDeleteSeries,
    handleDeleteChapter,
    handleRefreshTrackedToc,
    handleCacheAllChapters,
    handleUnbindSeries,
    handleCacheChapter,
  }
}
