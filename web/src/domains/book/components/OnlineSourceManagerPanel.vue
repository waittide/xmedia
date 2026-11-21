<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuArrowDownToLine,
  LuArrowUpToLine,
  LuBan,
  LuCheckCircle2,
  LuDownload,
  LuFlaskConical,
  LuFolder,
  LuGripVertical,
  LuKeyRound,
  LuListChecks,
  LuLoader2,
  LuMoreVertical,
  LuPencil,
  LuPlus,
  LuSearch,
  LuTrash2,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { onlineBookApi, type BookSourceSummaryDto } from '../onlineApi'
import OnlineSourceDebugSheet from './OnlineSourceDebugSheet.vue'
import OnlineSourceEditSheet from './OnlineSourceEditSheet.vue'
import OnlineSourceLoginModal from './OnlineSourceLoginModal.vue'

const props = defineProps<{
  sources: BookSourceSummaryDto[]
}>()

const emit = defineEmits<{
  refresh: []
}>()

const router = useRouter()
const snackbar = useSnackbarStore()
const dialogStore = useDialogStore()

const showImportBox = ref(false)
const importJsonText = ref('')
const importing = ref(false)
const importError = ref<string | null>(null)
const fileInputRef = ref<HTMLInputElement | null>(null)

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

// Multi-select state
const selectionMode = ref(false)
const selectedIds = ref<Set<string>>(new Set())
const lastToggledId = ref<string | null>(null)

// Local optimistic list so drag-and-drop updates immediately without visual flicker
const localSources = ref<BookSourceSummaryDto[]>([...props.sources])
watch(
  () => props.sources,
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

// Drag-and-drop state
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

// Floating MD3 overflow menu state
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
      if (visibleSet.has(id)) {
        pruned.add(id)
      } else {
        changed = true
      }
    }
    if (changed) {
      selectedIds.value = pruned
    }
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

function toggleSelectionMode(): void {
  closeMenu()
  selectionMode.value = !selectionMode.value
  if (!selectionMode.value) {
    selectedIds.value = new Set()
    lastToggledId.value = null
  }
}

function exitSelectionMode(): void {
  selectionMode.value = false
  selectedIds.value = new Set()
  lastToggledId.value = null
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
    for (const s of filteredSources.value) {
      next.delete(s.id)
    }
  } else {
    for (const s of filteredSources.value) {
      next.add(s.id)
    }
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

function closeMenu(): void {
  openMenuSourceId.value = null
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
  const top = fitsBelow
    ? rect.bottom + 6
    : Math.max(12, rect.top - 6 - estimatedHeight)
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
  if (openMenuSourceId.value) {
    closeMenu()
  }
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
    emit('refresh')
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
    const midY = rect.top + rect.height / 2
    dropPosition.value = ev.clientY < midY ? 'before' : 'after'
  }
  dropTargetId.value = src.id
}

function handleDragLeave(src: BookSourceSummaryDto, ev: DragEvent): void {
  const el = ev.currentTarget as HTMLElement | null
  if (el && ev.relatedTarget instanceof Node && el.contains(ev.relatedTarget)) {
    return
  }
  if (dropTargetId.value === src.id) {
    dropTargetId.value = null
  }
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

  const unchanged = list.every((s, idx) => s.id === localSources.value[idx]?.id)
  if (unchanged) return

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
    emit('refresh')
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
    emit('refresh')
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
    emit('refresh')
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    batchBusy.value = false
  }
}

function formatSourceHost(rawUrl: string): string {
  try {
    const u = new URL(rawUrl)
    return u.host || rawUrl
  } catch {
    return rawUrl
  }
}

function statusDotClass(src: BookSourceSummaryDto): string {
  if (src.last_test_ok === true) return 'bg-[var(--md-sys-color-success)]'
  if (src.last_test_ok === false) return 'bg-[var(--md-sys-color-error)]'
  return 'bg-[var(--md-sys-color-outline)]/55'
}

function statusDotTitle(src: BookSourceSummaryDto): string {
  if (src.last_test_ok === true) return src.last_test_msg || '最近测试通过'
  if (src.last_test_ok === false) return src.last_test_msg || '最近测试失败'
  return '尚未调试测试'
}

function triggerFileImport(): void {
  fileInputRef.value?.click()
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
    emit('refresh')
  } catch (err) {
    importError.value = err instanceof Error ? err.message : String(err)
  } finally {
    importing.value = false
  }
}

async function handleToggleEnabled(src: BookSourceSummaryDto, nextVal: boolean): Promise<void> {
  if (isSourceBusy(src.id)) return
  setSourceBusy(src.id, true)
  try {
    await onlineBookApi.updateSource(src.id, { enabled: nextVal })
    emit('refresh')
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    setSourceBusy(src.id, false)
  }
}

async function handlePinSource(src: BookSourceSummaryDto, position: 'top' | 'bottom'): Promise<void> {
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
    emit('refresh')
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
  emit('refresh')
}

function handleSavedAndDebug(updated: BookSourceSummaryDto): void {
  editSheetSource.value = null
  debugSheetSource.value = updated
  emit('refresh')
}
</script>

<template>
  <div class="space-y-4">
    <!-- Top Toolbar: Search + Multi-select + Import Actions -->
    <div class="flex flex-wrap items-center justify-between gap-2.5">
      <div class="relative min-w-[13rem] flex-1 sm:max-w-xs">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          v-model="searchQuery"
          type="text"
          placeholder="搜索书源名称、分组或网址…"
          class="md-input w-full py-1.5 pr-8 pl-9 text-xs"
        />
        <button
          v-if="searchQuery"
          type="button"
          class="absolute top-1/2 right-2 -translate-y-1/2 rounded-full p-0.5 text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-highest)]"
          @click="searchQuery = ''"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <button
          v-if="localSources.length > 0"
          type="button"
          class="md-btn md-btn-sm md-state"
          :class="selectionMode ? 'md-btn-tonal' : 'md-btn-outlined'"
          @click="toggleSelectionMode"
        >
          <LuListChecks class="h-3.5 w-3.5" />
          <span>{{ selectionMode ? '退出多选' : '多选' }}</span>
        </button>

        <input
          ref="fileInputRef"
          type="file"
          accept=".json,application/json"
          class="hidden"
          @change="handleFileSelected"
        />
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          title="从本地 JSON 文件导入书源"
          @click="triggerFileImport"
        >
          <LuUpload class="h-3.5 w-3.5" />
          <span>导入</span>
        </button>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state"
          title="粘贴书源 JSON 文本导入"
          @click="showImportBox = !showImportBox"
        >
          <LuPlus class="h-3.5 w-3.5" />
          <span>粘贴</span>
        </button>
      </div>
    </div>

    <!-- Filter Chips (Status + Groups + Reorder Hint) -->
    <div v-if="localSources.length > 0" class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-3 !text-xs"
          :class="statusFilter === 'all' && groupFilter === '__all__' ? 'md-chip-selected' : ''"
          @click="
            statusFilter = 'all';
            groupFilter = '__all__'
          "
        >
          全部 ({{ localSources.length }})
        </button>
        <button
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-3 !text-xs"
          :class="statusFilter === 'enabled' ? 'md-chip-selected' : ''"
          @click="statusFilter = statusFilter === 'enabled' ? 'all' : 'enabled'"
        >
          已启用 ({{ localSources.filter((s) => s.enabled).length }})
        </button>
        <button
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-3 !text-xs"
          :class="statusFilter === 'disabled' ? 'md-chip-selected' : ''"
          @click="statusFilter = statusFilter === 'disabled' ? 'all' : 'disabled'"
        >
          已禁用 ({{ localSources.filter((s) => !s.enabled).length }})
        </button>

        <span
          v-if="availableGroups.length > 0"
          class="mx-1 h-3.5 w-px bg-[var(--md-sys-color-outline-variant)]"
        />

        <button
          v-for="grp in availableGroups"
          :key="grp"
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-3 !text-xs"
          :class="groupFilter === grp ? 'md-chip-selected' : ''"
          @click="groupFilter = groupFilter === grp ? '__all__' : grp"
        >
          {{ grp }}
        </button>
      </div>

      <span
        v-if="!selectionMode && localSources.length > 1"
        class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]/80"
      >
        {{ canReorder ? '拖动左侧手柄可调整顺序' : '清除筛选后可拖动排序' }}
      </span>
      <span
        v-else-if="selectionMode"
        class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]/80"
      >
        点击条目勾选，按住 Shift 可连续区间选择
      </span>
    </div>

    <!-- Multi-Select Bulk Action Bar -->
    <div
      v-if="selectionMode"
      class="flex flex-wrap items-center justify-between gap-2.5 rounded-2xl bg-[var(--md-sys-color-secondary-container)] px-4 py-2.5 text-xs text-[var(--md-sys-color-on-secondary-container)]"
    >
      <div class="flex flex-wrap items-center gap-2">
        <span class="font-semibold">
          已选 {{ selectedCount }} / {{ filteredSources.length }} 项
        </span>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state !px-2 !text-[var(--md-sys-color-on-secondary-container)]"
          @click="toggleSelectAllFiltered"
        >
          {{ allFilteredSelected ? '取消全选' : '全选当前' }}
        </button>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state !px-2 !text-[var(--md-sys-color-on-secondary-container)]"
          @click="invertSelectFiltered"
        >
          反选
        </button>
      </div>

      <div class="flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm disabled:opacity-45"
          @click="handleBatchEnable(true)"
        >
          <LuCheckCircle2 class="h-3.5 w-3.5 text-[var(--md-sys-color-success)]" />
          <span>启用</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm disabled:opacity-45"
          @click="handleBatchEnable(false)"
        >
          <LuBan class="h-3.5 w-3.5 text-[var(--md-sys-color-on-surface-variant)]" />
          <span>禁用</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm disabled:opacity-45"
          @click="handleBatchSetGroup"
        >
          <LuFolder class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
          <span>分组</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] shadow-sm disabled:opacity-45"
          @click="handleBatchExport"
        >
          <LuDownload class="h-3.5 w-3.5" />
          <span>导出</span>
        </button>

        <button
          type="button"
          :disabled="selectedCount === 0 || batchBusy"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)] shadow-sm disabled:opacity-45"
          @click="handleBatchDelete"
        >
          <LuLoader2 v-if="batchBusy" class="h-3.5 w-3.5 animate-spin" />
          <LuTrash2 v-else class="h-3.5 w-3.5" />
          <span>删除</span>
        </button>
      </div>
    </div>

    <!-- Paste Import Box -->
    <div
      v-if="showImportBox"
      class="md-card-filled space-y-3 p-4"
    >
      <div
        v-if="importError"
        class="rounded-xl bg-[var(--md-sys-color-error-container)] px-3 py-2 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        {{ importError }}
      </div>
      <textarea
        v-model="importJsonText"
        rows="7"
        placeholder="粘贴单条或多条 Legado 书源 JSON（[...] 或 {...}）…"
        class="md-input w-full py-2.5 font-mono text-xs"
      />
      <div class="flex items-center justify-end gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="showImportBox = false"
        >
          取消
        </button>
        <button
          type="button"
          :disabled="!importJsonText.trim() || importing"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="handleImportSubmit"
        >
          <LuLoader2 v-if="importing" class="h-3.5 w-3.5 animate-spin" />
          <span>{{ importing ? '导入中…' : '确认导入' }}</span>
        </button>
      </div>
    </div>

    <!-- Empty State -->
    <div
      v-if="localSources.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center"
    >
      <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">尚未导入任何书源</p>
      <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
        上传或粘贴 Legado JSON 导入书源
      </p>
    </div>

    <!-- Empty Filter Result -->
    <div
      v-else-if="filteredSources.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-12 text-center"
    >
      <p class="text-sm font-medium text-[var(--md-sys-color-on-surface)]">没有匹配的书源</p>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-text md-state mt-2"
        @click="
          searchQuery = '';
          statusFilter = 'all';
          groupFilter = '__all__'
        "
      >
        清除筛选条件
      </button>
    </div>

    <!-- Minimal MD3 Draggable / Selectable Source Cards -->
    <div v-else class="space-y-2">
      <div
        v-for="src in filteredSources"
        :key="src.id"
        :draggable="canReorder"
        class="relative flex items-center justify-between gap-3 rounded-2xl px-3.5 py-3.5 transition"
        :class="[
          selectionMode
            ? selectedIds.has(src.id)
              ? 'cursor-pointer select-none bg-[var(--md-sys-color-secondary-container)]/35 ring-1 ring-[var(--md-sys-color-primary)]/55'
              : 'cursor-pointer select-none bg-[var(--md-sys-color-surface-container)] hover:bg-[var(--md-sys-color-surface-container-high)]/75'
            : 'bg-[var(--md-sys-color-surface-container)] hover:bg-[var(--md-sys-color-surface-container-high)]/75',
          !src.enabled && !selectedIds.has(src.id) ? 'opacity-65' : '',
          draggingId === src.id ? 'opacity-40 ring-1 ring-[var(--md-sys-color-primary)]/50' : '',
        ]"
        @click="selectionMode ? toggleSelectItem(src.id, $event.shiftKey) : undefined"
        @dragstart="handleDragStart(src, $event)"
        @dragover="handleDragOver(src, $event)"
        @dragleave="handleDragLeave(src, $event)"
        @drop="handleDrop(src, $event)"
        @dragend="handleDragEnd"
      >
        <!-- Drop Insertion Indicator -->
        <div
          v-if="dropTargetId === src.id && dropPosition === 'before'"
          class="pointer-events-none absolute -top-1.5 right-3 left-3 h-0.5 rounded-full bg-[var(--md-sys-color-primary)]"
        />
        <div
          v-if="dropTargetId === src.id && dropPosition === 'after'"
          class="pointer-events-none absolute right-3 -bottom-1.5 left-3 h-0.5 rounded-full bg-[var(--md-sys-color-primary)]"
        />

        <!-- Left: Checkbox (Selection Mode) OR Drag Handle + Source Name & Status Dot -->
        <div class="flex min-w-0 flex-1 items-center gap-2.5">
          <div
            v-if="selectionMode"
            class="flex shrink-0 items-center"
          >
            <AppCheckbox
              :model-value="selectedIds.has(src.id)"
            />
          </div>
          <div
            v-else-if="localSources.length > 1"
            class="flex h-7 w-5 shrink-0 items-center justify-center rounded-md text-[var(--md-sys-color-on-surface-variant)]/60 transition"
            :class="
              canReorder
                ? 'cursor-grab hover:text-[var(--md-sys-color-on-surface)] active:cursor-grabbing'
                : 'cursor-not-allowed opacity-35'
            "
            :title="canReorder ? '拖动调整书源顺序' : '清除筛选后可拖动排序'"
          >
            <LuGripVertical class="h-4 w-4" />
          </div>

          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                {{ src.name }}
              </span>
              <span
                v-if="src.group_name"
                class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ src.group_name }}
              </span>
            </div>

            <div class="mt-1.5 flex items-center gap-2">
              <span
                class="h-2.5 w-2.5 shrink-0 rounded-full"
                :class="statusDotClass(src)"
                :title="statusDotTitle(src)"
              />
              <span class="truncate font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                {{ formatSourceHost(src.source_url) }}
              </span>
              <span
                v-if="src.cookie_hosts.length > 0"
                class="shrink-0 text-[10px] text-[var(--md-sys-color-on-surface-variant)]/85"
              >
                · Cookie({{ src.cookie_hosts.length }})
              </span>
            </div>
          </div>
        </div>

        <!-- Right: Status Badge in Selection Mode, OR Switch + Edit + Overflow Menu -->
        <div
          v-if="selectionMode"
          class="shrink-0"
        >
          <span
            class="rounded-full px-2.5 py-0.5 text-[11px] font-medium"
            :class="
              src.enabled
                ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                : 'bg-[var(--md-sys-color-surface-container-highest)] text-[var(--md-sys-color-on-surface-variant)]'
            "
          >
            {{ src.enabled ? '已启用' : '已禁用' }}
          </span>
        </div>

        <div v-else class="flex shrink-0 items-center gap-1.5" @mousedown.stop>
          <AppSwitch
            :model-value="src.enabled"
            :disabled="isSourceBusy(src.id)"
            :title="src.enabled ? '禁用书源' : '启用书源'"
            @update:model-value="(val) => handleToggleEnabled(src, val)"
          />

          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            title="编辑书源"
            @click="editSheetSource = src"
          >
            <LuPencil class="h-4 w-4" />
          </button>

          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            :aria-expanded="openMenuSourceId === src.id"
            aria-haspopup="menu"
            title="更多操作"
            @click.stop="toggleSourceMenu(src, $event)"
          >
            <LuMoreVertical class="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>

    <!-- Teleported MD3 Overflow Menu -->
    <Teleport to="body">
      <div
        v-if="openMenuSource"
        class="fixed inset-0 z-[var(--z-popover,60)]"
        @mousedown="closeMenu"
      >
        <div
          role="menu"
          class="fixed w-44 space-y-0.5 rounded-2xl bg-[var(--md-sys-color-surface-container-high)] p-1.5 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)] ring-1 ring-[var(--md-sys-color-outline-variant)]/40"
          :style="{ top: `${menuPos.top}px`, left: `${menuPos.left}px` }"
          @mousedown.stop
        >
          <button
            type="button"
            role="menuitem"
            class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium"
            @click="handlePinSource(openMenuSource, 'top')"
          >
            <LuArrowUpToLine class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
            <span>置顶</span>
          </button>

          <button
            type="button"
            role="menuitem"
            class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium"
            @click="handlePinSource(openMenuSource, 'bottom')"
          >
            <LuArrowDownToLine class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
            <span>置底</span>
          </button>

          <button
            v-if="openMenuSource.has_login_url || openMenuSource.login_ui.length > 0"
            type="button"
            role="menuitem"
            class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium"
            @click="handleOpenLogin(openMenuSource)"
          >
            <LuKeyRound class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
            <span>登录</span>
          </button>

          <button
            type="button"
            role="menuitem"
            class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium"
            @click="handleSearchWithSource(openMenuSource)"
          >
            <LuSearch class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
            <span>搜索</span>
          </button>

          <button
            type="button"
            role="menuitem"
            class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium"
            @click="handleOpenDebug(openMenuSource)"
          >
            <LuFlaskConical class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
            <span>调试</span>
          </button>

          <a
            role="menuitem"
            :href="onlineBookApi.exportSourceUrl(openMenuSource.id)"
            :download="`${openMenuSource.name}.json`"
            class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium"
            @click="closeMenu"
          >
            <LuDownload class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
            <span>导出</span>
          </a>

          <div class="my-1 h-px bg-[var(--md-sys-color-outline-variant)]/50" />

          <button
            type="button"
            role="menuitem"
            class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium text-[var(--md-sys-color-error)]"
            @click="handleDeleteSource(openMenuSource)"
          >
            <LuTrash2 class="h-4 w-4" />
            <span>删除</span>
          </button>
        </div>
      </div>
    </Teleport>

    <OnlineSourceLoginModal
      :open="loginModalSource !== null"
      :source="loginModalSource"
      @close="loginModalSource = null"
      @updated="handleSourceUpdated"
    />

    <OnlineSourceEditSheet
      :open="editSheetSource !== null"
      :source="editSheetSource"
      @close="editSheetSource = null"
      @saved="emit('refresh')"
      @saved-and-debug="handleSavedAndDebug"
    />

    <OnlineSourceDebugSheet
      :open="debugSheetSource !== null"
      :source="debugSheetSource"
      @close="debugSheetSource = null"
      @tested="emit('refresh')"
    />
  </div>
</template>
