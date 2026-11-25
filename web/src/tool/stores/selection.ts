import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import {
  LuAlertCircle,
  LuCheckCircle2,
  LuFileText,
  LuImage,
  LuLayers,
  LuSparkles,
} from 'vue-icons-plus/lu'
import { i18n } from '@/i18n'
import {
  completenessScore,
  entryCompositeKey,
  isEntryComplete,
  type ToolCompletenessFilter,
} from '../completeness'
import type { MediaFileEntry } from '../types'
import { useToolDirectoryStore } from './directory'
import { useToolInspectorStore } from './inspector'

export type BrowseViewMode = 'flat' | 'folders'
export type SortField = 'name' | 'title' | 'incomplete' | 'size' | 'modified'
export type SortDir = 'asc' | 'desc'
export type CompletenessFilter = ToolCompletenessFilter

export const PAGE_CHUNK_SIZE = 100

export function loadSavedSortField(): SortField {
  const saved = localStorage.getItem('xmedia_tool.sort_field')
  if (
    saved === 'name' ||
    saved === 'title' ||
    saved === 'incomplete' ||
    saved === 'size' ||
    saved === 'modified'
  ) {
    return saved
  }
  return 'name'
}

export function loadSavedViewMode(): BrowseViewMode {
  return localStorage.getItem('xmedia_tool.view_mode') === 'folders' ? 'folders' : 'flat'
}

export function parentRelOf(relPath: string): string {
  const idx = relPath.lastIndexOf('/')
  return idx === -1 ? '' : relPath.slice(0, idx)
}

export const useToolSelectionStore = defineStore('toolSelection', () => {
  const directoryStore = useToolDirectoryStore()
  const inspectorStore = useToolInspectorStore()
  const t = (key: string, params?: Record<string, unknown>): string =>
    params ? String(i18n.global.t(key, params)) : String(i18n.global.t(key))

  const viewMode = ref<BrowseViewMode>(loadSavedViewMode())
  const sortField = ref<SortField>(loadSavedSortField())
  const sortDir = ref<SortDir>(
    localStorage.getItem('xmedia_tool.sort_dir') === 'desc' ? 'desc' : 'asc',
  )
  const visibleLimit = ref(PAGE_CHUNK_SIZE)
  const completenessFilter = ref<CompletenessFilter>('all')
  const searchQuery = ref('')
  const workbenchFocusPaths = ref<Set<string> | null>(null)
  const selectedPaths = ref<Set<string>>(new Set())
  const lastSelectedRelPath = ref<string | null>(null)

  const stats = computed(() => {
    const files = directoryStore.snapshot?.files ?? []
    let complete = 0
    let missingCover = 0
    let missingSummary = 0
    let missingTag = 0
    for (const f of files) {
      if (isEntryComplete(f)) complete += 1
      if (!f.completeness.has_cover) missingCover += 1
      if (!f.completeness.has_summary_or_lyrics) missingSummary += 1
      if (!f.completeness.has_nfo_or_tag_block) missingTag += 1
    }
    return {
      total: files.length,
      complete,
      incomplete: files.length - complete,
      missingCover,
      missingSummary,
      missingTag,
    }
  })

  const filterItems = computed(() => [
    {
      id: 'all' as CompletenessFilter,
      label: t('tool.filters.all'),
      count: stats.value.total,
      icon: LuLayers,
    },
    {
      id: 'complete' as CompletenessFilter,
      label: t('tool.filters.complete'),
      count: stats.value.complete,
      icon: LuCheckCircle2,
    },
    {
      id: 'incomplete' as CompletenessFilter,
      label: t('tool.filters.incomplete'),
      count: stats.value.incomplete,
      icon: LuAlertCircle,
    },
    {
      id: 'missing_cover' as CompletenessFilter,
      label: directoryStore.domainLabels.missingCover,
      count: stats.value.missingCover,
      icon: LuImage,
    },
    {
      id: 'missing_summary' as CompletenessFilter,
      label: directoryStore.domainLabels.missingSummary,
      count: stats.value.missingSummary,
      icon: LuFileText,
    },
    {
      id: 'missing_tag' as CompletenessFilter,
      label: directoryStore.domainLabels.missingTagBlock,
      count: stats.value.missingTag,
      icon: LuSparkles,
    },
  ])

  const hasActiveSearchOrFilter = computed<boolean>(
    () => Boolean(searchQuery.value.trim()) || completenessFilter.value !== 'all',
  )

  const sortOptions = computed(() => [
    { value: 'name', label: t('tool.sort.name') },
    { value: 'title', label: t('tool.sort.title') },
    { value: 'incomplete', label: t('tool.sort.incomplete') },
    { value: 'size', label: t('tool.sort.size') },
    { value: 'modified', label: t('tool.sort.modified') },
  ])

  const filteredFiles = computed<MediaFileEntry[]>(() => {
    const base = directoryStore.snapshot?.files ?? []
    const q = searchQuery.value.trim().toLowerCase()
    const focus = workbenchFocusPaths.value
    const showRecursive =
      viewMode.value === 'flat' ||
      hasActiveSearchOrFilter.value ||
      Boolean(focus && focus.size > 0)

    const list = base.filter((f) => {
      if (focus && focus.size > 0 && !focus.has(entryCompositeKey(f))) return false
      if (!showRecursive && parentRelOf(f.rel_path) !== directoryStore.currentRelPath) return false
      if (completenessFilter.value === 'complete' && !isEntryComplete(f)) return false
      if (completenessFilter.value === 'incomplete' && isEntryComplete(f)) return false
      if (completenessFilter.value === 'missing_cover' && f.completeness.has_cover) return false
      if (completenessFilter.value === 'missing_summary' && f.completeness.has_summary_or_lyrics)
        return false
      if (completenessFilter.value === 'missing_tag' && f.completeness.has_nfo_or_tag_block)
        return false
      if (q) {
        const hay =
          `${f.rel_path} ${f.file_name} ${f.display_title} ${f.display_subtitle}`.toLowerCase()
        if (!hay.includes(q)) return false
      }
      return true
    })

    const dir = sortDir.value === 'asc' ? 1 : -1
    return [...list].sort((a, b) => {
      let cmp = 0
      if (sortField.value === 'title') {
        cmp = a.display_title.localeCompare(b.display_title, undefined, { numeric: true })
      } else if (sortField.value === 'incomplete') {
        cmp = completenessScore(a) - completenessScore(b)
      } else if (sortField.value === 'size') {
        cmp = a.file_size - b.file_size
      } else if (sortField.value === 'modified') {
        cmp = a.modified_unix_secs - b.modified_unix_secs
      } else {
        cmp = a.file_name.localeCompare(b.file_name, undefined, { numeric: true })
      }
      if (cmp === 0) {
        cmp = a.rel_path.localeCompare(b.rel_path, undefined, { numeric: true })
      }
      return cmp * dir
    })
  })

  const visibleFiles = computed<MediaFileEntry[]>(() =>
    filteredFiles.value.slice(0, visibleLimit.value),
  )

  watch(
    [
      searchQuery,
      completenessFilter,
      viewMode,
      sortField,
      sortDir,
      () => directoryStore.activeRootId,
      () => directoryStore.currentRelPath,
    ],
    () => {
      visibleLimit.value = PAGE_CHUNK_SIZE
    },
  )

  watch(searchQuery, (q) => {
    if (q.trim() && directoryStore.activeNav !== 'items') {
      directoryStore.activeNav = 'items'
      directoryStore.previousWorkspaceNav = 'items'
      localStorage.setItem('xmedia_tool.active_nav', 'items')
      directoryStore.notifyRouteSync(false)
    } else if (directoryStore.activeNav === 'items') {
      directoryStore.notifyRouteSync(true)
    }
  })

  const isFolderManagerMode = computed<boolean>(
    () =>
      viewMode.value === 'folders' &&
      !hasActiveSearchOrFilter.value &&
      !(workbenchFocusPaths.value && workbenchFocusPaths.value.size > 0),
  )

  const checkedFiles = computed<MediaFileEntry[]>(() => {
    if (selectedPaths.value.size === 0) return []
    if (isFolderManagerMode.value && !directoryStore.isMultiRootSummary) {
      const folderPrefixes: string[] = []
      for (const p of selectedPaths.value) {
        if (directoryStore.currentFolderMap.has(p)) {
          folderPrefixes.push(`${p}/`)
        }
      }
      const allFiles = directoryStore.snapshot?.files ?? []
      return allFiles.filter(
        (f) =>
          selectedPaths.value.has(f.rel_path) ||
          folderPrefixes.some((prefix) => f.rel_path.startsWith(prefix)),
      )
    }
    return filteredFiles.value.filter((f) =>
      selectedPaths.value.has(directoryStore.entrySelectionKey(f)),
    )
  })

  const selectedEntriesForRename = computed<MediaFileEntry[]>(() => {
    if (checkedFiles.value.length > 0) return checkedFiles.value
    if (inspectorStore.activeDetail) {
      const match = filteredFiles.value.find(
        (f) =>
          f.root_id === inspectorStore.activeDetail?.entry.root_id &&
          f.rel_path === inspectorStore.activeDetail?.entry.rel_path,
      )
      if (match) return [match]
    }
    return []
  })

  const selectedBookEntries = computed(() =>
    checkedFiles.value.filter(
      (f) =>
        f.media_kind === 'book' &&
        (f.extension === 'txt' || f.extension === 'epub' || f.extension === 'pdf'),
    ),
  )
  const selectedComicEntries = computed(() =>
    selectedEntriesForRename.value.filter((e) => e.media_kind === 'comic'),
  )
  const selectedMusicEntries = computed(() =>
    selectedEntriesForRename.value.filter((e) => e.media_kind === 'music'),
  )
  const selectedVideoEntries = computed(() =>
    selectedEntriesForRename.value.filter((e) => e.media_kind === 'video'),
  )

  const allFilteredSelected = computed(
    () =>
      filteredFiles.value.length > 0 &&
      filteredFiles.value.every((f) =>
        selectedPaths.value.has(directoryStore.entrySelectionKey(f)),
      ),
  )

  function clearSelection(): void {
    selectedPaths.value = new Set()
    lastSelectedRelPath.value = null
  }

  function pruneSelection(surviving: Set<string>): void {
    if (selectedPaths.value.size === 0) return
    selectedPaths.value = new Set([...selectedPaths.value].filter((p) => surviving.has(p)))
  }

  function setViewMode(next: BrowseViewMode): void {
    if (viewMode.value !== next) clearSelection()
    viewMode.value = next
    if (next === 'folders') {
      completenessFilter.value = 'all'
      workbenchFocusPaths.value = null
    }
    localStorage.setItem('xmedia_tool.view_mode', next)
  }

  function setSortField(field: SortField): void {
    sortField.value = field
    localStorage.setItem('xmedia_tool.sort_field', field)
  }

  function toggleSortDir(): void {
    sortDir.value = sortDir.value === 'asc' ? 'desc' : 'asc'
    localStorage.setItem('xmedia_tool.sort_dir', sortDir.value)
  }

  function selectCompletenessFilter(filter: CompletenessFilter): void {
    completenessFilter.value = filter
    if (filter !== 'all') setViewMode('flat')
    if (directoryStore.activeNav !== 'items') {
      directoryStore.activeNav = 'items'
      directoryStore.previousWorkspaceNav = 'items'
      localStorage.setItem('xmedia_tool.active_nav', 'items')
    }
    directoryStore.mobileMenuOpen = false
    directoryStore.notifyRouteSync(false)
  }

  function toggleSelectFile(relPath: string, shiftKey = false, orderedPaths?: string[]): void {
    const next = new Set(selectedPaths.value)
    if (shiftKey && lastSelectedRelPath.value) {
      const listPaths: string[] =
        orderedPaths && orderedPaths.length > 0
          ? orderedPaths
          : isFolderManagerMode.value
            ? [
                ...(directoryStore.snapshot?.folders ?? []).map((d) => d.rel_path),
                ...(directoryStore.snapshot?.raw_files ?? []).map((f) => f.rel_path),
              ]
            : filteredFiles.value.map((f) => directoryStore.entrySelectionKey(f))
      const fromIdx = listPaths.indexOf(lastSelectedRelPath.value)
      const toIdx = listPaths.indexOf(relPath)
      if (fromIdx !== -1 && toIdx !== -1) {
        const start = Math.min(fromIdx, toIdx)
        const end = Math.max(fromIdx, toIdx)
        const shouldSelect = !next.has(relPath)
        for (let i = start; i <= end; i += 1) {
          const p = listPaths[i]!
          if (shouldSelect) next.add(p)
          else next.delete(p)
        }
        selectedPaths.value = next
        lastSelectedRelPath.value = relPath
        return
      }
    }
    if (next.has(relPath)) next.delete(relPath)
    else next.add(relPath)
    selectedPaths.value = next
    lastSelectedRelPath.value = relPath
  }

  function toggleSelectAllFiltered(): void {
    if (allFilteredSelected.value) {
      selectedPaths.value = new Set()
    } else {
      selectedPaths.value = new Set(
        filteredFiles.value.map((f) => directoryStore.entrySelectionKey(f)),
      )
    }
  }

  function toggleSelectAllRaw(paths: string[]): void {
    const allSelected = paths.length > 0 && paths.every((p) => selectedPaths.value.has(p))
    selectedPaths.value = allSelected ? new Set() : new Set(paths)
  }

  return {
    viewMode,
    sortField,
    sortDir,
    visibleLimit,
    completenessFilter,
    searchQuery,
    workbenchFocusPaths,
    selectedPaths,
    lastSelectedRelPath,
    stats,
    filterItems,
    hasActiveSearchOrFilter,
    sortOptions,
    filteredFiles,
    visibleFiles,
    isFolderManagerMode,
    checkedFiles,
    selectedEntriesForRename,
    selectedBookEntries,
    selectedComicEntries,
    selectedMusicEntries,
    selectedVideoEntries,
    allFilteredSelected,
    setViewMode,
    setSortField,
    toggleSortDir,
    selectCompletenessFilter,
    toggleSelectFile,
    toggleSelectAllFiltered,
    toggleSelectAllRaw,
    clearSelection,
    pruneSelection,
  }
})
