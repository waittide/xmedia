<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuArrowDownToLine,
  LuArrowUpToLine,
  LuDownload,
  LuFlaskConical,
  LuGripVertical,
  LuKeyRound,
  LuLoader2,
  LuMoreVertical,
  LuPencil,
  LuPlus,
  LuSearch,
  LuTrash2,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
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

const busyId = ref<string | null>(null)
const reordering = ref(false)
const loginModalSource = ref<BookSourceSummaryDto | null>(null)
const editSheetSource = ref<BookSourceSummaryDto | null>(null)
const debugSheetSource = ref<BookSourceSummaryDto | null>(null)

// Local optimistic list so drag-and-drop updates immediately without visual flicker
const localSources = ref<BookSourceSummaryDto[]>([...props.sources])
watch(
  () => props.sources,
  (next) => {
    localSources.value = [...next]
  },
  { deep: false },
)

// Drag-and-drop state
const draggingId = ref<string | null>(null)
const dropTargetId = ref<string | null>(null)
const dropPosition = ref<'before' | 'after'>('before')

const canReorder = computed(
  () =>
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
  if (ev.key === 'Escape' && openMenuSourceId.value) {
    closeMenu()
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
  busyId.value = src.id
  try {
    await onlineBookApi.updateSource(src.id, { enabled: nextVal })
    emit('refresh')
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    busyId.value = null
  }
}

async function handlePinSource(src: BookSourceSummaryDto, position: 'top' | 'bottom'): Promise<void> {
  closeMenu()
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
  if (!ok) return
  busyId.value = src.id
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
    busyId.value = null
  }
}

function handleSourceUpdated(updated: BookSourceSummaryDto): void {
  loginModalSource.value = updated
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
    <!-- Top Toolbar: Search + Import Actions -->
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

      <div class="flex items-center gap-2">
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
          @click="triggerFileImport"
        >
          <LuUpload class="h-3.5 w-3.5" />
          <span>上传 JSON</span>
        </button>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="showImportBox = !showImportBox"
        >
          <LuPlus class="h-3.5 w-3.5" />
          <span>粘贴导入</span>
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
        v-if="localSources.length > 1"
        class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]/80"
      >
        {{ canReorder ? '拖动左侧手柄可调整顺序' : '清除筛选后可拖动排序' }}
      </span>
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

    <!-- Minimal MD3 Draggable Source Cards -->
    <div v-else class="space-y-2">
      <div
        v-for="src in filteredSources"
        :key="src.id"
        :draggable="canReorder"
        class="relative flex items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] px-3.5 py-3.5 transition hover:bg-[var(--md-sys-color-surface-container-high)]/75"
        :class="[
          !src.enabled ? 'opacity-65' : '',
          draggingId === src.id ? 'opacity-40 ring-1 ring-[var(--md-sys-color-primary)]/50' : '',
        ]"
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

        <!-- Left: Drag Handle + Source Name + Status Dot & Compact Meta -->
        <div class="flex min-w-0 flex-1 items-center gap-2.5">
          <div
            v-if="localSources.length > 1"
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

        <!-- Right: Switch + Edit + Overflow Menu -->
        <div class="flex shrink-0 items-center gap-1.5" @mousedown.stop>
          <AppSwitch
            :model-value="src.enabled"
            :disabled="busyId === src.id"
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
            <span>导出 JSON</span>
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
