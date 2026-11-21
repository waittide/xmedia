import { computed, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  extractApiError,
  type OnlineSourceManagerAdapter,
  type OnlineSourceSummaryDto,
} from './types'

export function useOnlineSourceManager<S extends OnlineSourceSummaryDto>(
  sourcesProp: Ref<S[]>,
  adapterProp: Ref<OnlineSourceManagerAdapter<S>>,
  onRefresh: () => void,
) {
  const { t, locale } = useI18n()
  const router = useRouter()
  const snackbar = useSnackbarStore()
  const dialogStore = useDialogStore()

  const showImportBox = ref(false)
  const importJsonText = ref('')
  const importing = ref(false)
  const seeding = ref(false)
  const importError = ref<string | null>(null)

  const searchQuery = ref('')
  const statusFilter = ref<'all' | 'enabled' | 'disabled'>('all')
  const groupFilter = ref<string>('__all__')

  const busyIds = ref<Set<string>>(new Set())
  const reordering = ref(false)
  const batchBusy = ref(false)
  const loginModalSource = ref<S | null>(null)
  const editSheetSource = ref<S | null>(null)
  const debugSheetSource = ref<S | null>(null)

  const domainLabel = computed(() =>
    t(`onlineSources.domainLabel.${adapterProp.value.domain}`),
  )

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

  const localSources = ref<S[]>([...sourcesProp.value]) as Ref<S[]>
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

  const openMenuSource = computed<S | null>(
    () => localSources.value.find((s) => s.id === openMenuSourceId.value) ?? null,
  )

  const availableGroups = computed(() => {
    const set = new Set<string>()
    for (const s of localSources.value) {
      const g = s.group_name.trim()
      if (g) set.add(g)
    }
    return Array.from(set).sort((a, b) => a.localeCompare(b, locale.value))
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

  function toggleSourceMenu(src: S, ev: MouseEvent): void {
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

  async function persistOrder(nextList: S[], toastMsg?: string): Promise<void> {
    const prev = [...localSources.value]
    localSources.value = nextList
    reordering.value = true
    try {
      const saved = await adapterProp.value.reorderSources(nextList.map((s) => s.id))
      localSources.value = saved
      if (toastMsg) snackbar.show({ message: toastMsg })
      onRefresh()
    } catch (err) {
      localSources.value = prev
      snackbar.show({ message: extractApiError(err), tone: 'error' })
    } finally {
      reordering.value = false
    }
  }

  function handleDragStart(src: S, ev: DragEvent): void {
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

  function handleDragOver(src: S, ev: DragEvent): void {
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

  function handleDragLeave(src: S, ev: DragEvent): void {
    const el = ev.currentTarget as HTMLElement | null
    if (el && ev.relatedTarget instanceof Node && el.contains(ev.relatedTarget)) return
    if (dropTargetId.value === src.id) dropTargetId.value = null
  }

  async function handleDrop(targetSrc: S, ev: DragEvent): Promise<void> {
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
      const rep = await adapterProp.value.batchSources({
        ids,
        action: enabled ? 'enable' : 'disable',
      })
      snackbar.show({
        message: t(enabled ? 'onlineSources.messages.batchEnabled' : 'onlineSources.messages.batchDisabled', {
          count: rep.affected_count,
          domain: domainLabel.value,
        }),
      })
      onRefresh()
    } catch (err) {
      snackbar.show({ message: extractApiError(err), tone: 'error' })
    } finally {
      batchBusy.value = false
    }
  }

  async function handleBatchSetGroup(): Promise<void> {
    const ids = Array.from(selectedIds.value)
    if (ids.length === 0 || batchBusy.value) return
    const groupInput = await dialogStore.prompt({
      title: t('onlineSources.messages.batchGroupTitle'),
      message: t('onlineSources.messages.batchGroupMsg', {
        count: ids.length,
        domain: domainLabel.value,
      }),
      placeholder: t('onlineSources.messages.batchGroupPlaceholder'),
      confirmText: t('onlineSources.messages.batchGroupConfirm'),
      cancelText: t('common.cancel'),
    })
    if (groupInput === null) return
    batchBusy.value = true
    try {
      const trimmed = groupInput.trim()
      const rep = await adapterProp.value.batchSources({
        ids,
        action: 'set_group',
        group_name: trimmed,
      })
      snackbar.show({
        message: trimmed
          ? t('onlineSources.messages.batchGroupApplied', {
              count: rep.affected_count,
              domain: domainLabel.value,
              group: trimmed,
            })
          : t('onlineSources.messages.batchGroupCleared', {
              count: rep.affected_count,
              domain: domainLabel.value,
            }),
      })
      onRefresh()
    } catch (err) {
      snackbar.show({ message: extractApiError(err), tone: 'error' })
    } finally {
      batchBusy.value = false
    }
  }

  async function handleBatchExport(): Promise<void> {
    const ids = Array.from(selectedIds.value)
    if (ids.length === 0 || batchBusy.value) return
    batchBusy.value = true
    try {
      const arr = await adapterProp.value.exportSourcesBatch(ids)
      const blob = new Blob([JSON.stringify(arr, null, 2)], {
        type: 'application/json;charset=utf-8',
      })
      const url = URL.createObjectURL(blob)
      const link = document.createElement('a')
      try {
        link.href = url
        link.download = `${adapterProp.value.exportFilePrefix}-${arr.length}.json`
        document.body.appendChild(link)
        link.click()
      } finally {
        link.remove()
        setTimeout(() => {
          URL.revokeObjectURL(url)
        }, 1000)
      }
      snackbar.show({
        message: t('onlineSources.messages.batchExported', {
          count: arr.length,
          domain: domainLabel.value,
        }),
      })
    } catch (err) {
      snackbar.show({ message: extractApiError(err), tone: 'error' })
    } finally {
      batchBusy.value = false
    }
  }

  async function handleBatchDelete(): Promise<void> {
    const ids = Array.from(selectedIds.value)
    if (ids.length === 0 || batchBusy.value) return
    const ok = await dialogStore.confirm({
      title: t('onlineSources.messages.batchDeleteTitle', { domain: domainLabel.value }),
      message: t('onlineSources.messages.batchDeleteMsg', {
        count: ids.length,
        domain: domainLabel.value,
      }),
      confirmText: t('onlineSources.messages.batchDeleteConfirm', { count: ids.length }),
      cancelText: t('common.cancel'),
      danger: true,
    })
    if (!ok) return
    batchBusy.value = true
    try {
      const rep = await adapterProp.value.batchSources({ ids, action: 'delete' })
      snackbar.show({
        message: t('onlineSources.messages.batchDeleted', {
          count: rep.affected_count,
          domain: domainLabel.value,
        }),
      })
      exitSelectionMode()
      onRefresh()
    } catch (err) {
      snackbar.show({ message: extractApiError(err), tone: 'error' })
    } finally {
      batchBusy.value = false
    }
  }

  async function handleImportSubmit(): Promise<void> {
    if (!importJsonText.value.trim() || importing.value) return
    importing.value = true
    importError.value = null
    try {
      const report = await adapterProp.value.importSources(importJsonText.value)
      snackbar.show({
        message: t('onlineSources.messages.importSuccess', {
          domain: domainLabel.value,
          imported: report.imported_count,
          updated: report.updated_count,
        }),
      })
      importJsonText.value = ''
      showImportBox.value = false
      onRefresh()
    } catch (err) {
      importError.value = extractApiError(err)
    } finally {
      importing.value = false
    }
  }

  async function handleImportSeeds(): Promise<void> {
    const fn = adapterProp.value.importSeeds
    if (!fn || seeding.value) return
    seeding.value = true
    importError.value = null
    try {
      const report = await fn()
      snackbar.show({
        message: t('onlineSources.messages.seedsImported', {
          domain: domainLabel.value,
          imported: report.imported_count,
          total: report.sources.length,
        }),
      })
      onRefresh()
    } catch (err) {
      snackbar.show({ message: extractApiError(err), tone: 'error' })
    } finally {
      seeding.value = false
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
      importError.value = extractApiError(err)
    }
  }

  async function handleToggleEnabled(src: S, nextVal: boolean): Promise<void> {
    if (isSourceBusy(src.id)) return
    setSourceBusy(src.id, true)
    try {
      await adapterProp.value.updateSource(src.id, { enabled: nextVal })
      onRefresh()
    } catch (err) {
      snackbar.show({ message: extractApiError(err), tone: 'error' })
    } finally {
      setSourceBusy(src.id, false)
    }
  }

  async function handlePinSource(src: S, position: 'top' | 'bottom'): Promise<void> {
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
      t(position === 'top' ? 'onlineSources.messages.pinnedTop' : 'onlineSources.messages.pinnedBottom', {
        name: src.name,
      }),
    )
  }

  function handleOpenLogin(src: S): void {
    closeMenu()
    loginModalSource.value = src
  }

  function handleSearchWithSource(src: S): void {
    closeMenu()
    void router.push({
      path: adapterProp.value.searchRoutePath,
      query: { scope: 'online', source: src.id },
    })
  }

  function handleOpenDebug(src: S): void {
    closeMenu()
    debugSheetSource.value = src
  }

  async function handleDeleteSource(src: S): Promise<void> {
    closeMenu()
    const ok = await dialogStore.confirm({
      title: t('onlineSources.messages.deleteTitle', { domain: domainLabel.value }),
      message: t('onlineSources.messages.deleteMsg', {
        domain: domainLabel.value,
        name: src.name,
      }),
      confirmText: t('common.delete'),
      cancelText: t('common.cancel'),
      danger: true,
    })
    if (!ok || isSourceBusy(src.id)) return
    setSourceBusy(src.id, true)
    try {
      await adapterProp.value.deleteSource(src.id)
      snackbar.show({
        message: t('onlineSources.messages.deleted', {
          domain: domainLabel.value,
          name: src.name,
        }),
      })
      onRefresh()
    } catch (err) {
      snackbar.show({ message: extractApiError(err), tone: 'error' })
    } finally {
      setSourceBusy(src.id, false)
    }
  }

  function handleSourceUpdated(updated: S): void {
    if (loginModalSource.value?.id === updated.id) {
      loginModalSource.value = updated
    }
    onRefresh()
  }

  function handleSavedAndDebug(updated: S): void {
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
    domainLabel,
    showImportBox,
    importJsonText,
    importing,
    seeding,
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
    handleImportSeeds,
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
