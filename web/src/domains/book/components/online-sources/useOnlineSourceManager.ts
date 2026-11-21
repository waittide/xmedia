import { computed, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue'
import { useRouter } from 'vue-router'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { onlineBookApi, type BookSourceSummaryDto } from '../../onlineApi'

export function useOnlineSourceManager(
  sourcesProp: Ref<BookSourceSummaryDto[]>,
  onRefresh: () => void,
) {
  const router = useRouter()
  const snackbar = useSnackbarStore()
  const dialogStore = useDialogStore()

  const showImportBox = ref(false)
  const importJsonText = ref('')
  const importing = ref(false)
  const importError = ref<string | null>(null)

  const searchQuery = ref('')
  const statusFilter = ref<'all' | 'enabled' | 'disabled'>('all')
  const groupFilter = ref<string>('__all__')

  const busyIds = ref<Set<string>>(new Set())
  const reordering = ref(false)
  const batchBusy = ref(false)
  const loginModalSource = ref<BookSourceSummaryDto | null>(null)
  const editSheetSource = ref<BookSourceSummaryDto | null>(null)
  const debugSheetSource = ref<BookSourceSummaryDto | null>(null)

  function isSourceBusy(id: string): boolean {
    return busyIds.value.has(id)
  }

  function setSourceBusy(id: string, busy: boolean): void {
    const next = new Set(busyIds.value)
    if (busy) next.add(id)
    else next.delete(id)
    busyIds.value = next
  }

  const selectionMode = ref(false)
  const selectedIds = ref<Set<string>>(new Set())
  const lastToggledId = ref<string | null>(null)

  const localSources = ref<BookSourceSummaryDto[]>([...sourcesProp.value])
  watch(
    sourcesProp,
    (next) => {
      if (reordering.value) return
      localSources.value = [...next]
      if (selectedIds.value.size > 0) {
        const valid = new Set(next.map((s) => s.id))
        const pruned = new Set<string>()
        for (const id of selectedIds.value) {
          if (valid.has(id)) pruned.add(id)
        }
        selectedIds.value = pruned
        if (lastToggledId.value && !valid.has(lastToggledId.value)) {
          lastToggledId.value = null
        }
      }
      if (next.length === 0) {
        selectionMode.value = false
      }
    },
    { deep: false },
  )

  const draggingId = ref<string | null>(null)
  const dropTargetId = ref<string | null>(null)
  const dropPosition = ref<'before' | 'after'>('before')

  const canReorder = computed(
    () =>
      !selectionMode.value &&
      !searchQuery.value.trim() &&
      statusFilter.value === 'all' &&
      groupFilter.value === '__all__' &&
      localSources.value.length > 1 &&
      !reordering.value,
  )

  const openMenuSourceId = ref<string | null>(null)
  const menuPos = ref<{ top: number; left: number }>({ top: 0, left: 0 })

  const openMenuSource = computed(
    () => localSources.value.find((s) => s.id === openMenuSourceId.value) ?? null,
  )

  const availableGroups = computed(() => {
    const set = new Set<string>()
    for (const s of localSources.value) {
      const g = s.group_name.trim()
      if (g) set.add(g)
    }
    return Array.from(set).sort((a, b) => a.localeCompare(b, 'zh-CN'))
  })

  watch(availableGroups, (groups) => {
    if (groupFilter.value !== '__all__' && !groups.includes(groupFilter.value)) {
      groupFilter.value = '__all__'
    }
  })

  const enabledCount = computed(() => localSources.value.filter((s) => s.enabled).length)
  const disabledCount = computed(() => localSources.value.length - enabledCount.value)

  const filteredSources = computed(() => {
    const q = searchQuery.value.trim().toLowerCase()
    return localSources.value.filter((s) => {
      if (statusFilter.value === 'enabled' && !s.enabled) return false
      if (statusFilter.value === 'disabled' && s.enabled) return false
      if (groupFilter.value !== '__all__' && s.group_name.trim() !== groupFilter.value) return false
      if (!q) return true
      return (
        s.name.toLowerCase().includes(q) ||
        s.source_url.toLowerCase().includes(q) ||
        s.group_name.toLowerCase().includes(q) ||
        (s.comment ?? '').toLowerCase().includes(q)
      )
    })
  })

  watch(filteredSources, (visible) => {
    const visibleSet = new Set(visible.map((s) => s.id))
    if (selectedIds.value.size > 0) {
      let changed = false
      const pruned = new Set<string>()
      for (const id of selectedIds.value) {
        if (visibleSet.has(id)) pruned.add(id)
        else changed = true
      }
      if (changed) selectedIds.value = pruned
    }
    if (lastToggledId.value && !visibleSet.has(lastToggledId.value)) {
      lastToggledId.value = null
    }
  })

  const selectedCount = computed(() => selectedIds.value.size)

  const allFilteredSelected = computed(
    () =>
      filteredSources.value.length > 0 &&
      filteredSources.value.every((s) => selectedIds.value.has(s.id)),
  )

  function closeMenu(): void {
    openMenuSourceId.value = null
  }

  function exitSelectionMode(): void {
    selectionMode.value = false
    selectedIds.value = new Set()
    lastToggledId.value = null
  }

  function toggleSelectionMode(): void {
    closeMenu()
    selectionMode.value = !selectionMode.value
    if (!selectionMode.value) {
      selectedIds.value = new Set()
      lastToggledId.value = null
    }
  }

  function toggleSelectItem(srcId: string, shiftKey = false): void {
    const next = new Set(selectedIds.value)
    if (shiftKey && lastToggledId.value && lastToggledId.value !== srcId) {
      const ids = filteredSources.value.map((s) => s.id)
      const startIdx = ids.indexOf(lastToggledId.value)
      const endIdx = ids.indexOf(srcId)
      if (startIdx >= 0 && endIdx >= 0) {
        const [lo, hi] = startIdx < endIdx ? [startIdx, endIdx] : [endIdx, startIdx]
        const shouldSelect = !next.has(srcId)
        for (let i = lo; i <= hi; i += 1) {
          const id = ids[i]
          if (!id) continue
          if (shouldSelect) next.add(id)
          else next.delete(id)
        }
        selectedIds.value = next
        lastToggledId.value = srcId
        return
      }
    }
    if (next.has(srcId)) next.delete(srcId)
    else next.add(srcId)
    selectedIds.value = next
    lastToggledId.value = srcId
  }

  function toggleSelectAllFiltered(): void {
    const next = new Set(selectedIds.value)
    if (allFilteredSelected.value) {
      for (const s of filteredSources.value) next.delete(s.id)
    } else {
      for (const s of filteredSources.value) next.add(s.id)
    }
    selectedIds.value = next
    lastToggledId.value = null
  }

  function invertSelectFiltered(): void {
    const next = new Set(selectedIds.value)
    for (const s of filteredSources.value) {
      if (next.has(s.id)) next.delete(s.id)
      else next.add(s.id)
    }
    selectedIds.value = next
    lastToggledId.value = null
  }

  function toggleSourceMenu(src: BookSourceSummaryDto, ev: MouseEvent): void {
    if (openMenuSourceId.value === src.id) {
      closeMenu()
      return
    }
    const btn = ev.currentTarget as HTMLElement | null
    if (!btn) return
    const rect = btn.getBoundingClientRect()
    const menuWidth = 176
    const hasLogin = src.has_login_url || src.login_ui.length > 0
    const estimatedHeight = hasLogin ? 320 : 280
    const left = Math.max(12, Math.min(rect.right - menuWidth, window.innerWidth - menuWidth - 12))
    const fitsBelow = rect.bottom + 6 + estimatedHeight <= window.innerHeight - 12
    const top = fitsBelow ? rect.bottom + 6 : Math.max(12, rect.top - 6 - estimatedHeight)
    menuPos.value = { top, left }
    openMenuSourceId.value = src.id
  }

  function onGlobalKeydown(ev: KeyboardEvent): void {
    if (ev.key === 'Escape') {
      if (openMenuSourceId.value) {
        closeMenu()
      } else if (
        selectionMode.value &&
        !editSheetSource.value &&
        !debugSheetSource.value &&
        !loginModalSource.value
      ) {
        exitSelectionMode()
      }
    }
  }

  function onGlobalScrollOrResize(): void {
    if (openMenuSourceId.value) closeMenu()
  }

  onMounted(() => {
    window.addEventListener('keydown', onGlobalKeydown)
    window.addEventListener('resize', onGlobalScrollOrResize)
    window.addEventListener('scroll', onGlobalScrollOrResize, true)
  })

  onBeforeUnmount(() => {
    window.removeEventListener('keydown', onGlobalKeydown)
    window.removeEventListener('resize', onGlobalScrollOrResize)
    window.removeEventListener('scroll', onGlobalScrollOrResize, true)
  })

  async function persistOrder(nextList: BookSourceSummaryDto[], toastMsg?: string): Promise<void> {
    const prev = [...localSources.value]
    localSources.value = nextList
    reordering.value = true
    try {
      const saved = await onlineBookApi.reorderSources(nextList.map((s) => s.id))
      localSources.value = saved
      if (toastMsg) snackbar.show({ message: toastMsg })
      onRefresh()
    } catch (err) {
      localSources.value = prev
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      reordering.value = false
    }
  }

  function handleDragStart(src: BookSourceSummaryDto, ev: DragEvent): void {
    if (!canReorder.value) {
      ev.preventDefault()
      return
    }
    closeMenu()
    draggingId.value = src.id
    dropTargetId.value = null
    if (ev.dataTransfer) {
      ev.dataTransfer.effectAllowed = 'move'
      ev.dataTransfer.setData('text/plain', src.id)
    }
  }

  function handleDragOver(src: BookSourceSummaryDto, ev: DragEvent): void {
    if (!draggingId.value || draggingId.value === src.id || !canReorder.value) return
    ev.preventDefault()
    if (ev.dataTransfer) ev.dataTransfer.dropEffect = 'move'
    const el = ev.currentTarget as HTMLElement | null
    if (el) {
      const rect = el.getBoundingClientRect()
      dropPosition.value = ev.clientY < rect.top + rect.height / 2 ? 'before' : 'after'
    }
    dropTargetId.value = src.id
  }

  function handleDragLeave(src: BookSourceSummaryDto, ev: DragEvent): void {
    const el = ev.currentTarget as HTMLElement | null
    if (el && ev.relatedTarget instanceof Node && el.contains(ev.relatedTarget)) return
    if (dropTargetId.value === src.id) dropTargetId.value = null
  }

  async function handleDrop(targetSrc: BookSourceSummaryDto, ev: DragEvent): Promise<void> {
    ev.preventDefault()
    const fromId = draggingId.value || ev.dataTransfer?.getData('text/plain') || null
    const pos = dropPosition.value
    draggingId.value = null
    dropTargetId.value = null

    if (!fromId || fromId === targetSrc.id || !canReorder.value) return
    const list = [...localSources.value]
    const fromIdx = list.findIndex((s) => s.id === fromId)
    if (fromIdx < 0) return

    const [moved] = list.splice(fromIdx, 1)
    if (!moved) return
    let targetIdx = list.findIndex((s) => s.id === targetSrc.id)
    if (targetIdx < 0) return
    if (pos === 'after') targetIdx += 1
    list.splice(targetIdx, 0, moved)

    if (list.every((s, idx) => s.id === localSources.value[idx]?.id)) return
    await persistOrder(list)
  }

  function handleDragEnd(): void {
    draggingId.value = null
    dropTargetId.value = null
  }

  async function handleBatchEnable(enabled: boolean): Promise<void> {
    const ids = Array.from(selectedIds.value)
    if (ids.length === 0 || batchBusy.value) return
    batchBusy.value = true
    try {
      const rep = await onlineBookApi.batchSources({
        ids,
        action: enabled ? 'enable' : 'disable',
      })
      snackbar.show({
        message: enabled
          ? `已启用 ${rep.affected_count} 条书源`
          : `已禁用 ${rep.affected_count} 条书源`,
      })
      onRefresh()
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      batchBusy.value = false
    }
  }

  async function handleBatchSetGroup(): Promise<void> {
    const ids = Array.from(selectedIds.value)
    if (ids.length === 0 || batchBusy.value) return
    const groupInput = await dialogStore.prompt({
      title: '批量设置分组',
      message: `为已选中的 ${ids.length} 条书源设置分组名称（留空则清除分组）：`,
      placeholder: '例如：精品、轻小说',
      confirmText: '应用分组',
      cancelText: '取消',
    })
    if (groupInput === null) return
    batchBusy.value = true
    try {
      const trimmed = groupInput.trim()
      const rep = await onlineBookApi.batchSources({
        ids,
        action: 'set_group',
        group_name: trimmed,
      })
      snackbar.show({
        message: trimmed
          ? `已将 ${rep.affected_count} 条书源归入「${trimmed}」`
          : `已清除 ${rep.affected_count} 条书源的分组`,
      })
      onRefresh()
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      batchBusy.value = false
    }
  }

  async function handleBatchExport(): Promise<void> {
    const ids = Array.from(selectedIds.value)
    if (ids.length === 0 || batchBusy.value) return
    batchBusy.value = true
    try {
      const arr = await onlineBookApi.exportSourcesBatch(ids)
      const blob = new Blob([JSON.stringify(arr, null, 2)], {
        type: 'application/json;charset=utf-8',
      })
      const url = URL.createObjectURL(blob)
      const link = document.createElement('a')
      try {
        link.href = url
        link.download = `xmedia-book-sources-${arr.length}.json`
        document.body.appendChild(link)
        link.click()
      } finally {
        link.remove()
        setTimeout(() => {
          URL.revokeObjectURL(url)
        }, 1000)
      }
      snackbar.show({ message: `已导出 ${arr.length} 条书源 JSON` })
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      batchBusy.value = false
    }
  }

  async function handleBatchDelete(): Promise<void> {
    const ids = Array.from(selectedIds.value)
    if (ids.length === 0 || batchBusy.value) return
    const ok = await dialogStore.confirm({
      title: '批量删除书源',
      message: `确定删除已选中的 ${ids.length} 条书源吗？这些书源的本地章节缓存也将一并清理。`,
      confirmText: `删除 (${ids.length})`,
      cancelText: '取消',
      danger: true,
    })
    if (!ok) return
    batchBusy.value = true
    try {
      const rep = await onlineBookApi.batchSources({ ids, action: 'delete' })
      snackbar.show({ message: `已删除 ${rep.affected_count} 条书源` })
      exitSelectionMode()
      onRefresh()
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      batchBusy.value = false
    }
  }

  async function handleImportSubmit(): Promise<void> {
    if (!importJsonText.value.trim() || importing.value) return
    importing.value = true
    importError.value = null
    try {
      const report = await onlineBookApi.importSources(importJsonText.value)
      snackbar.show({
        message: `书源导入完成：新增 ${report.imported_count} 条，更新 ${report.updated_count} 条`,
      })
      importJsonText.value = ''
      showImportBox.value = false
      onRefresh()
    } catch (err) {
      importError.value = err instanceof Error ? err.message : String(err)
    } finally {
      importing.value = false
    }
  }

  async function handleFileSelected(ev: Event): Promise<void> {
    const input = ev.target as HTMLInputElement
    const file = input.files?.[0]
    input.value = ''
    if (!file) return
    try {
      const text = await file.text()
      importJsonText.value = text
      showImportBox.value = true
      await handleImportSubmit()
    } catch (err) {
      importError.value = err instanceof Error ? err.message : String(err)
    }
  }

  async function handleToggleEnabled(src: BookSourceSummaryDto, nextVal: boolean): Promise<void> {
    if (isSourceBusy(src.id)) return
    setSourceBusy(src.id, true)
    try {
      await onlineBookApi.updateSource(src.id, { enabled: nextVal })
      onRefresh()
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      setSourceBusy(src.id, false)
    }
  }

  async function handlePinSource(
    src: BookSourceSummaryDto,
    position: 'top' | 'bottom',
  ): Promise<void> {
    closeMenu()
    if (reordering.value || localSources.value.length <= 1) return
    const currentIdx = localSources.value.findIndex((s) => s.id === src.id)
    if (currentIdx < 0) return
    if (position === 'top' && currentIdx === 0) return
    if (position === 'bottom' && currentIdx === localSources.value.length - 1) return
    const rest = localSources.value.filter((s) => s.id !== src.id)
    const nextList = position === 'top' ? [src, ...rest] : [...rest, src]
    await persistOrder(
      nextList,
      position === 'top' ? `已置顶「${src.name}」` : `已置底「${src.name}」`,
    )
  }

  function handleOpenLogin(src: BookSourceSummaryDto): void {
    closeMenu()
    loginModalSource.value = src
  }

  function handleSearchWithSource(src: BookSourceSummaryDto): void {
    closeMenu()
    void router.push({
      path: '/book/catalog',
      query: { scope: 'online', source: src.id },
    })
  }

  function handleOpenDebug(src: BookSourceSummaryDto): void {
    closeMenu()
    debugSheetSource.value = src
  }

  async function handleDeleteSource(src: BookSourceSummaryDto): Promise<void> {
    closeMenu()
    const ok = await dialogStore.confirm({
      title: '删除书源',
      message: `确定删除书源「${src.name}」吗？该书源的本地章节缓存也将一并清理。`,
      confirmText: '删除',
      cancelText: '取消',
      danger: true,
    })
    if (!ok || isSourceBusy(src.id)) return
    setSourceBusy(src.id, true)
    try {
      await onlineBookApi.deleteSource(src.id)
      snackbar.show({ message: `已删除书源「${src.name}」` })
      onRefresh()
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      setSourceBusy(src.id, false)
    }
  }

  function handleSourceUpdated(updated: BookSourceSummaryDto): void {
    if (loginModalSource.value?.id === updated.id) {
      loginModalSource.value = updated
    }
    onRefresh()
  }

  function handleSavedAndDebug(updated: BookSourceSummaryDto): void {
    editSheetSource.value = null
    debugSheetSource.value = updated
    onRefresh()
  }

  function clearFilters(): void {
    searchQuery.value = ''
    statusFilter.value = 'all'
    groupFilter.value = '__all__'
  }

  return {
    showImportBox,
    importJsonText,
    importing,
    importError,
    searchQuery,
    statusFilter,
    groupFilter,
    batchBusy,
    loginModalSource,
    editSheetSource,
    debugSheetSource,
    selectionMode,
    selectedIds,
    localSources,
    draggingId,
    dropTargetId,
    dropPosition,
    canReorder,
    openMenuSourceId,
    menuPos,
    openMenuSource,
    availableGroups,
    enabledCount,
    disabledCount,
    filteredSources,
    selectedCount,
    allFilteredSelected,
    isSourceBusy,
    closeMenu,
    toggleSelectionMode,
    toggleSelectItem,
    toggleSelectAllFiltered,
    invertSelectFiltered,
    toggleSourceMenu,
    handleDragStart,
    handleDragOver,
    handleDragLeave,
    handleDrop,
    handleDragEnd,
    handleBatchEnable,
    handleBatchSetGroup,
    handleBatchExport,
    handleBatchDelete,
    handleFileSelected,
    handleImportSubmit,
    handleToggleEnabled,
    handlePinSource,
    handleOpenLogin,
    handleSearchWithSource,
    handleOpenDebug,
    handleDeleteSource,
    handleSourceUpdated,
    handleSavedAndDebug,
    clearFilters,
  }
}
