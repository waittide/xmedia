import { computed, watch, type ComputedRef, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuCheckCircle2,
  LuFileText,
  LuImage,
  LuLayers,
  LuSparkles,
} from 'vue-icons-plus/lu'
import { completenessScore, entryCompositeKey, isEntryComplete } from '../../completeness'
import type { DirectorySnapshot, ManagedRootDto, MediaFileDetail, MediaFileEntry } from '../../types'
import {
  PAGE_CHUNK_SIZE,
  parentRelOf,
  type BrowseViewMode,
  type CompletenessFilter,
  type SortDir,
  type SortField,
} from './hashState'

export interface UseWorkspaceFilteringOptions {
  snapshot: Ref<DirectorySnapshot | null>
  domainRoots: ComputedRef<ManagedRootDto[]>
  activeRootId: Ref<string>
  currentRelPath: Ref<string>
  viewMode: Ref<BrowseViewMode>
  sortField: Ref<SortField>
  sortDir: Ref<SortDir>
  completenessFilter: Ref<CompletenessFilter>
  searchQuery: Ref<string>
  workbenchFocusPaths: Ref<Set<string> | null>
  selectedPaths: Ref<Set<string>>
  lastSelectedRelPath: Ref<string | null>
  visibleLimit: Ref<number>
  activeDetail: Ref<MediaFileDetail | null>
  domainLabels: ComputedRef<{
    missingCover: string
    missingSummary: string
    missingTagBlock: string
    searchPlaceholder: string
  }>
}

export function useWorkspaceFiltering(opts: UseWorkspaceFilteringOptions) {
  const { t } = useI18n()

  const stats = computed(() => {
    const files = opts.snapshot.value?.files ?? []
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
    { id: 'all' as CompletenessFilter, label: t('tool.filters.all'), count: stats.value.total, icon: LuLayers },
    { id: 'complete' as CompletenessFilter, label: t('tool.filters.complete'), count: stats.value.complete, icon: LuCheckCircle2 },
    { id: 'incomplete' as CompletenessFilter, label: t('tool.filters.incomplete'), count: stats.value.incomplete, icon: LuAlertCircle },
    { id: 'missing_cover' as CompletenessFilter, label: opts.domainLabels.value.missingCover, count: stats.value.missingCover, icon: LuImage },
    { id: 'missing_summary' as CompletenessFilter, label: opts.domainLabels.value.missingSummary, count: stats.value.missingSummary, icon: LuFileText },
    { id: 'missing_tag' as CompletenessFilter, label: opts.domainLabels.value.missingTagBlock, count: stats.value.missingTag, icon: LuSparkles },
  ])

  const hasActiveSearchOrFilter = computed<boolean>(
    () => Boolean(opts.searchQuery.value.trim()) || opts.completenessFilter.value !== 'all',
  )

  const sortOptions = computed(() => [
    { value: 'name', label: t('tool.sort.name') },
    { value: 'title', label: t('tool.sort.title') },
    { value: 'incomplete', label: t('tool.sort.incomplete') },
    { value: 'size', label: t('tool.sort.size') },
    { value: 'modified', label: t('tool.sort.modified') },
  ])

  function setSortField(field: SortField): void {
    opts.sortField.value = field
    localStorage.setItem('xmedia_tool.sort_field', field)
  }

  function toggleSortDir(): void {
    opts.sortDir.value = opts.sortDir.value === 'asc' ? 'desc' : 'asc'
    localStorage.setItem('xmedia_tool.sort_dir', opts.sortDir.value)
  }

  const filteredFiles = computed<MediaFileEntry[]>(() => {
    const base = opts.snapshot.value?.files ?? []
    const q = opts.searchQuery.value.trim().toLowerCase()
    const focus = opts.workbenchFocusPaths.value
    const showRecursive =
      opts.viewMode.value === 'flat' ||
      hasActiveSearchOrFilter.value ||
      Boolean(focus && focus.size > 0)

    const list = base.filter((f) => {
      if (focus && focus.size > 0 && !focus.has(entryCompositeKey(f))) {
        return false
      }
      if (!showRecursive && parentRelOf(f.rel_path) !== opts.currentRelPath.value) {
        return false
      }
      if (opts.completenessFilter.value === 'complete' && !isEntryComplete(f)) return false
      if (opts.completenessFilter.value === 'incomplete' && isEntryComplete(f)) return false
      if (opts.completenessFilter.value === 'missing_cover' && f.completeness.has_cover)
        return false
      if (
        opts.completenessFilter.value === 'missing_summary' &&
        f.completeness.has_summary_or_lyrics
      )
        return false
      if (opts.completenessFilter.value === 'missing_tag' && f.completeness.has_nfo_or_tag_block)
        return false

      if (q) {
        const hay =
          `${f.rel_path} ${f.file_name} ${f.display_title} ${f.display_subtitle}`.toLowerCase()
        if (!hay.includes(q)) return false
      }
      return true
    })

    const dir = opts.sortDir.value === 'asc' ? 1 : -1
    return [...list].sort((a, b) => {
      let cmp = 0
      if (opts.sortField.value === 'title') {
        cmp = a.display_title.localeCompare(b.display_title, undefined, { numeric: true })
      } else if (opts.sortField.value === 'incomplete') {
        cmp = completenessScore(a) - completenessScore(b)
      } else if (opts.sortField.value === 'size') {
        cmp = a.file_size - b.file_size
      } else if (opts.sortField.value === 'modified') {
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
    filteredFiles.value.slice(0, opts.visibleLimit.value),
  )

  watch(
    [
      opts.searchQuery,
      opts.completenessFilter,
      opts.viewMode,
      opts.sortField,
      opts.sortDir,
      opts.activeRootId,
      opts.currentRelPath,
    ],
    () => {
      opts.visibleLimit.value = PAGE_CHUNK_SIZE
    },
  )

  const isMultiRootSummary = computed<boolean>(
    () => !opts.activeRootId.value && opts.domainRoots.value.length > 1,
  )

  function entrySelectionKey(entry: Pick<MediaFileEntry, 'root_id' | 'rel_path'>): string {
    return isMultiRootSummary.value ? `${entry.root_id}::${entry.rel_path}` : entry.rel_path
  }

  function parseSelectionKey(key: string): { rootId: string; relPath: string } {
    const sep = key.indexOf('::')
    if (sep !== -1) {
      return { rootId: key.slice(0, sep), relPath: key.slice(sep + 2) }
    }
    const fallbackRootId =
      opts.activeRootId.value || opts.snapshot.value?.root_id || opts.domainRoots.value[0]?.id || ''
    return { rootId: fallbackRootId, relPath: key }
  }

  function resolveModalRootId(entries: MediaFileEntry[]): string | null {
    const uniqueRoots = new Set(entries.map((e) => e.root_id).filter(Boolean))
    if (uniqueRoots.size > 1) return null
    if (uniqueRoots.size === 1) return Array.from(uniqueRoots)[0]!
    return (
      opts.activeRootId.value || opts.snapshot.value?.root_id || opts.domainRoots.value[0]?.id || ''
    )
  }

  const breadcrumbs = computed<Array<{ label: string; relPath: string }>>(() => {
    if (!opts.snapshot.value) return []
    const items: Array<{ label: string; relPath: string }> = [
      { label: opts.snapshot.value.root_name, relPath: '' },
    ]
    if (!opts.snapshot.value.current_rel_path) return items
    const segments = opts.snapshot.value.current_rel_path.split('/').filter(Boolean)
    let acc = ''
    for (const seg of segments) {
      acc = acc ? `${acc}/${seg}` : seg
      items.push({ label: seg, relPath: acc })
    }
    return items
  })

  const isFolderManagerMode = computed<boolean>(
    () =>
      opts.viewMode.value === 'folders' &&
      !hasActiveSearchOrFilter.value &&
      !(opts.workbenchFocusPaths.value && opts.workbenchFocusPaths.value.size > 0),
  )

  const currentFolderMap = computed(() => {
    const m = new Map<string, { name: string; media_file_count: number; child_dir_count: number }>()
    for (const f of opts.snapshot.value?.folders ?? []) {
      m.set(f.rel_path, {
        name: f.name,
        media_file_count: f.media_file_count,
        child_dir_count: f.child_dir_count,
      })
    }
    return m
  })

  const checkedFiles = computed<MediaFileEntry[]>(() => {
    if (opts.selectedPaths.value.size === 0) return []
    if (isFolderManagerMode.value && !isMultiRootSummary.value) {
      const folderPrefixes: string[] = []
      for (const p of opts.selectedPaths.value) {
        if (currentFolderMap.value.has(p)) {
          folderPrefixes.push(`${p}/`)
        }
      }
      const allFiles = opts.snapshot.value?.files ?? []
      return allFiles.filter(
        (f) =>
          opts.selectedPaths.value.has(f.rel_path) ||
          folderPrefixes.some((prefix) => f.rel_path.startsWith(prefix)),
      )
    }
    return filteredFiles.value.filter((f) => opts.selectedPaths.value.has(entrySelectionKey(f)))
  })

  const selectedEntriesForRename = computed<MediaFileEntry[]>(() => {
    if (checkedFiles.value.length > 0) {
      return checkedFiles.value
    }
    if (opts.activeDetail.value) {
      const match = filteredFiles.value.find(
        (f) =>
          f.root_id === opts.activeDetail.value?.entry.root_id &&
          f.rel_path === opts.activeDetail.value?.entry.rel_path,
      )
      if (match) return [match]
    }
    return []
  })

  const allFilteredSelected = computed(
    () =>
      filteredFiles.value.length > 0 &&
      filteredFiles.value.every((f) => opts.selectedPaths.value.has(entrySelectionKey(f))),
  )

  function toggleSelectFile(relPath: string, shiftKey = false, orderedPaths?: string[]): void {
    const next = new Set(opts.selectedPaths.value)
    if (shiftKey && opts.lastSelectedRelPath.value) {
      const listPaths: string[] =
        orderedPaths && orderedPaths.length > 0
          ? orderedPaths
          : isFolderManagerMode.value
            ? [
                ...(opts.snapshot.value?.folders ?? []).map((d) => d.rel_path),
                ...(opts.snapshot.value?.raw_files ?? []).map((f) => f.rel_path),
              ]
            : filteredFiles.value.map((f) => entrySelectionKey(f))
      const fromIdx = listPaths.indexOf(opts.lastSelectedRelPath.value)
      const toIdx = listPaths.indexOf(relPath)
      if (fromIdx !== -1 && toIdx !== -1) {
        const start = Math.min(fromIdx, toIdx)
        const end = Math.max(fromIdx, toIdx)
        const shouldSelect = !next.has(relPath)
        for (let i = start; i <= end; i += 1) {
          const p = listPaths[i]!
          if (shouldSelect) {
            next.add(p)
          } else {
            next.delete(p)
          }
        }
        opts.selectedPaths.value = next
        opts.lastSelectedRelPath.value = relPath
        return
      }
    }
    if (next.has(relPath)) {
      next.delete(relPath)
    } else {
      next.add(relPath)
    }
    opts.selectedPaths.value = next
    opts.lastSelectedRelPath.value = relPath
  }

  function toggleSelectAllFiltered(): void {
    if (allFilteredSelected.value) {
      opts.selectedPaths.value = new Set()
    } else {
      opts.selectedPaths.value = new Set(filteredFiles.value.map((f) => entrySelectionKey(f)))
    }
  }

  function toggleSelectAllRaw(paths: string[]): void {
    const allSelected = paths.length > 0 && paths.every((p) => opts.selectedPaths.value.has(p))
    if (allSelected) {
      opts.selectedPaths.value = new Set()
    } else {
      opts.selectedPaths.value = new Set(paths)
    }
  }

  return {
    stats,
    filterItems,
    hasActiveSearchOrFilter,
    sortOptions,
    setSortField,
    toggleSortDir,
    filteredFiles,
    visibleFiles,
    isMultiRootSummary,
    entrySelectionKey,
    parseSelectionKey,
    resolveModalRootId,
    breadcrumbs,
    isFolderManagerMode,
    currentFolderMap,
    checkedFiles,
    selectedEntriesForRename,
    allFilteredSelected,
    toggleSelectFile,
    toggleSelectAllFiltered,
    toggleSelectAllRaw,
  }
}
