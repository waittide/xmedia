<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  LuArrowDown,
  LuArrowLeft,
  LuArrowUp,
  LuBookOpen,
  LuCheckCircle2,
  LuChevronDown,
  LuClapperboard,
  LuDisc3,
  LuFileSignature,
  LuFileText,
  LuFilm,
  LuImage,
  LuLayers,
  LuLock,
  LuMusic,
  LuPackageCheck,
  LuPanelLeft,
  LuRefreshCw,
  LuSave,
  LuSearch,
  LuSparkles,
  LuUnlock,
  LuUpload,
  LuWand2,
  LuX,
} from 'vue-icons-plus/lu'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { buildArtworkUrl, resolveScraperCoverUrl, toolApi } from '../../api'
import { formatBytes, formatDuration, useToolText } from '../../i18n'
import type { MediaFileDetail, MediaFileEntry, ScrapedMetadata } from '../../types'
import InspectorBookForm from '../inspector/InspectorBookForm.vue'
import InspectorComicForm from '../inspector/InspectorComicForm.vue'
import InspectorMusicForm from '../inspector/InspectorMusicForm.vue'
import InspectorVideoForm from '../inspector/InspectorVideoForm.vue'
import {
  useMetadataForm,
  type ApplyFillMode,
  type ScrapedFieldKey,
} from '../inspector/useMetadataForm'
import CandidateDetailPreviewPane, {
  type CandidatePreviewState,
} from './CandidateDetailPreviewPane.vue'
import WorkbenchCandidatePane from './WorkbenchCandidatePane.vue'

type DropdownFilter = 'all' | 'incomplete' | 'missing_cover' | 'saved'

const props = defineProps<{
  detail: MediaFileDetail | null
  loadingDetail: boolean
  entries: MediaFileEntry[]
  savedPaths: Set<string>
  artworkVersion: number
  sidebarCollapsed: boolean
  hasPrev: boolean
  hasNext: boolean
  positionLabel: string
}>()

const emit = defineEmits<{
  exit: []
  toggleSidebar: []
  selectEntry: [entry: MediaFileEntry]
  step: [delta: -1 | 1]
  updated: [detail: MediaFileDetail, bumpArtwork: boolean]
  openScrape: [detail: MediaFileDetail]
  openSettingsScrapers: []
  openContentEditor: [entry: MediaFileEntry]
  openNormalizeEpub: [entry: MediaFileEntry]
  openComicPages: [entry: MediaFileEntry]
  openMusicTools: [entry: MediaFileEntry]
  openVideoTools: [entry: MediaFileEntry]
  openHistory: []
}>()

const txt = useToolText()
const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()

const {
  bookForm,
  comicForm,
  musicForm,
  videoForm,
  pendingCoverUrl,
  pendingBackdropUrl,
  videoArtworkSlot,
  isDirty,
  isBookFieldLocked,
  toggleBookFieldLock,
  isVideoFieldLocked,
  toggleVideoFieldLock,
  hydrateForm,
  buildPatchEnvelope,
  applyScrapedField,
  applyScrapedMetadata,
} = useMetadataForm()

const saving = ref(false)
const polishingBook = ref(false)
const uploadingArtwork = ref(false)
const errorMsg = ref<string | null>(null)
const coverInputRef = ref<HTMLInputElement | null>(null)
const pendingSlot = ref<'cover' | 'series_cover' | 'poster' | 'fanart' | 'thumb'>('cover')
const skipNextHydrate = ref(false)

const autoSearchOnSwitch = ref(true)
const autoSearchTrigger = ref(0)
const candidatePreview = ref<CandidatePreviewState | null>(null)

// Plan B: Top dropdown queue picker state
const queueDropdownOpen = ref(false)
const dropdownQuery = ref('')
const dropdownFilter = ref<DropdownFilter>('all')

function isEntryComplete(e: MediaFileEntry): boolean {
  return (
    e.completeness.has_title &&
    e.completeness.has_cover &&
    e.completeness.has_summary_or_lyrics &&
    e.completeness.has_nfo_or_tag_block
  )
}

const dropdownEntries = computed(() => {
  const q = dropdownQuery.value.trim().toLowerCase()
  return props.entries.filter((e) => {
    if (dropdownFilter.value === 'incomplete' && isEntryComplete(e)) return false
    if (dropdownFilter.value === 'missing_cover' && e.completeness.has_cover) return false
    if (dropdownFilter.value === 'saved' && !props.savedPaths.has(e.rel_path)) return false
    if (q) {
      const hay = `${e.display_title} ${e.file_name} ${e.display_subtitle}`.toLowerCase()
      if (!hay.includes(q)) return false
    }
    return true
  })
})

const savedSessionCount = computed(() => {
  let n = 0
  for (const e of props.entries) {
    if (props.savedPaths.has(e.rel_path)) n += 1
  }
  return n
})

watch(
  () => props.detail,
  (d) => {
    if (!d) return
    if (skipNextHydrate.value) {
      skipNextHydrate.value = false
      return
    }
    errorMsg.value = null
    candidatePreview.value = null
    hydrateForm(d)
    if (autoSearchOnSwitch.value) {
      autoSearchTrigger.value += 1
    }
  },
  { immediate: true },
)

const isCurrentDirty = computed(() => Boolean(props.detail) && isDirty.value)

const canSaveMetadata = computed(() => {
  if (!props.detail) return false
  if (props.detail.metadata.kind === 'book') {
    return props.detail.metadata.data.writable
  }
  return true
})

const artworkSrc = computed(() => {
  if (!props.detail) return ''
  const kind = props.detail.entry.media_kind
  if (pendingCoverUrl.value && (kind !== 'video' || videoArtworkSlot.value === 'poster')) {
    return resolveScraperCoverUrl(pendingCoverUrl.value)
  }
  if (pendingBackdropUrl.value && kind === 'video' && videoArtworkSlot.value === 'fanart') {
    return resolveScraperCoverUrl(pendingBackdropUrl.value)
  }
  if (props.detail.metadata.kind === 'video') {
    const slot = videoArtworkSlot.value
    const has =
      slot === 'poster'
        ? props.detail.metadata.data.has_poster
        : slot === 'fanart'
          ? props.detail.metadata.data.has_fanart
          : props.detail.metadata.data.has_thumb
    if (!has) return ''
    return buildArtworkUrl(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
      'video',
      props.artworkVersion,
      slot,
    )
  }
  if (!props.detail.entry.completeness.has_cover) return ''
  return buildArtworkUrl(
    props.detail.entry.root_id,
    props.detail.entry.rel_path,
    kind,
    props.artworkVersion,
    'cover',
  )
})

async function confirmDiscardIfDirty(): Promise<boolean> {
  if (!isCurrentDirty.value) return true
  return dialogStore.confirm({
    title: '放弃左侧未保存的更改？',
    message: '当前条目的左侧元数据表单已修改但尚未保存，切换后这些修改将丢失。',
    confirmText: '放弃并继续',
    cancelText: '留下编辑',
    danger: true,
  })
}

async function requestStep(delta: -1 | 1): Promise<void> {
  if (delta < 0 && !props.hasPrev) return
  if (delta > 0 && !props.hasNext) return
  if (!(await confirmDiscardIfDirty())) return
  queueDropdownOpen.value = false
  emit('step', delta)
}

async function requestPickEntry(entry: MediaFileEntry): Promise<void> {
  queueDropdownOpen.value = false
  if (props.detail?.entry.rel_path === entry.rel_path) return
  if (!(await confirmDiscardIfDirty())) return
  emit('selectEntry', entry)
}

async function requestExit(): Promise<void> {
  if (!(await confirmDiscardIfDirty())) return
  queueDropdownOpen.value = false
  emit('exit')
}

function triggerArtworkUpload(
  slot: 'cover' | 'series_cover' | 'poster' | 'fanart' | 'thumb',
): void {
  pendingSlot.value = slot
  coverInputRef.value?.click()
}

async function handleArtworkPicked(ev: Event): Promise<void> {
  if (!props.detail) return
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  uploadingArtwork.value = true
  errorMsg.value = null
  try {
    const updated = await toolApi.uploadArtwork(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
      file,
      pendingSlot.value,
      props.detail.entry.media_kind,
    )
    skipNextHydrate.value = true
    emit('updated', updated, true)
    snackbar.show('封面已更新并写入磁盘')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    uploadingArtwork.value = false
  }
}

async function handlePolishBook(): Promise<void> {
  if (!props.detail || polishingBook.value) return
  polishingBook.value = true
  errorMsg.value = null
  try {
    const report = await toolApi.polishBook(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
    )
    emit('updated', report.detail, true)
    snackbar.show(
      `书籍抛光完成 · ${report.jacket_updated ? '简介页已更新' : '目录已同步'}，清理 ${report.removed_orphan_files} 个冗余资源`,
    )
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    polishingBook.value = false
  }
}

async function saveMetadata(
  organizeAfterSave = false,
  advanceNext = false,
): Promise<void> {
  if (!props.detail || !canSaveMetadata.value || saving.value) return
  saving.value = true
  errorMsg.value = null
  try {
    const envelope = buildPatchEnvelope(props.detail.entry.media_kind)
    let updated = await toolApi.writeMetadata(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
      envelope,
    )

    let bumpedArt = false
    if (pendingCoverUrl.value) {
      try {
        const slot = updated.entry.media_kind === 'video' ? 'poster' : 'cover'
        updated = await toolApi.uploadArtworkFromUrl(
          updated.entry.root_id,
          updated.entry.rel_path,
          pendingCoverUrl.value,
          slot,
          updated.entry.media_kind,
        )
        bumpedArt = true
      } catch {
        // Keep persisted metadata even if remote cover download fails
      }
      pendingCoverUrl.value = null
    }
    if (pendingBackdropUrl.value && updated.entry.media_kind === 'video') {
      try {
        updated = await toolApi.uploadArtworkFromUrl(
          updated.entry.root_id,
          updated.entry.rel_path,
          pendingBackdropUrl.value,
          'fanart',
          'video',
        )
        bumpedArt = true
      } catch {
        // Ignore backdrop fetch failure
      }
      pendingBackdropUrl.value = null
    }

    if (organizeAfterSave) {
      const plan = await toolApi.previewRename(
        updated.entry.root_id,
        [updated.entry.rel_path],
        null,
        updated.entry.media_kind,
      )
      const actionable = plan.find((p) => !p.unchanged && !p.conflict)
      if (actionable) {
        await toolApi.executeRename(updated.entry.root_id, [actionable])
        updated = await toolApi.inspectFile(
          updated.entry.root_id,
          actionable.to_rel_path,
          updated.entry.media_kind,
        )
      }
    }

    hydrateForm(updated)
    emit('updated', updated, bumpedArt)
    snackbar.show({
      message: organizeAfterSave ? '已保存元数据并完成路径归档' : '已保存元数据更改',
      actionLabel: '变更历史',
      onAction: () => emit('openHistory'),
    })

    if (advanceNext && props.hasNext) {
      emit('step', 1)
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    saving.value = false
  }
}

function handleFillAll(meta: ScrapedMetadata, mode: ApplyFillMode): void {
  if (!props.detail) return
  const count = applyScrapedMetadata(props.detail.entry.media_kind, meta, mode)
  snackbar.show(`已将 ${count} 个字段填入左侧表单（尚未保存）`)
}

function handleFillField(field: ScrapedFieldKey, meta: ScrapedMetadata): void {
  if (!props.detail) return
  const ok = applyScrapedField(props.detail.entry.media_kind, field, meta, 'overwrite')
  if (ok) {
    snackbar.show('已填入左侧表单，按 ⌘S 保存落盘')
  }
}

function handlePreviewFillField(
  field: ScrapedFieldKey,
  meta: ScrapedMetadata,
  label: string,
): void {
  if (!props.detail) return
  const ok = applyScrapedField(props.detail.entry.media_kind, field, meta, 'overwrite')
  if (ok) {
    snackbar.show(`已将「${label}」填入左侧表单，按 ⌘S 保存落盘`)
  }
}

async function handleDirectApply(
  meta: ScrapedMetadata,
  mode: ApplyFillMode,
  advanceNext = false,
): Promise<void> {
  if (!props.detail) return
  applyScrapedMetadata(props.detail.entry.media_kind, meta, mode)
  await saveMetadata(false, advanceNext)
}

function handleKeydown(ev: KeyboardEvent): void {
  if (ev.key === 'Escape' && queueDropdownOpen.value) {
    ev.preventDefault()
    queueDropdownOpen.value = false
    return
  }
  if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === 's') {
    ev.preventDefault()
    void saveMetadata(false, false)
    return
  }
  if ((ev.ctrlKey || ev.metaKey) && ev.key === 'Enter') {
    ev.preventDefault()
    void saveMetadata(false, true)
    return
  }
  if (ev.altKey && ev.key === 'ArrowUp') {
    ev.preventDefault()
    void requestStep(-1)
    return
  }
  if (ev.altKey && ev.key === 'ArrowDown') {
    ev.preventDefault()
    void requestStep(1)
  }
}

onMounted(() => window.addEventListener('keydown', handleKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', handleKeydown))

defineExpose({
  isDirty: isCurrentDirty,
  confirmDiscardIfDirty,
})
</script>

<template>
  <div class="relative flex h-full w-full flex-col overflow-hidden bg-[var(--md-sys-color-surface)]">
    <input
      ref="coverInputRef"
      type="file"
      accept="image/jpeg,image/png,image/webp"
      class="hidden"
      @change="handleArtworkPicked"
    />

    <!-- Plan B: Compact Top Queue Switcher Bar (Always Accessible Even When Sidebar Collapsed) -->
    <div
      class="relative z-20 flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container-low)] px-3.5 py-2"
    >
      <!-- Left: Exit & Sidebar Queue Toggle -->
      <div class="flex shrink-0 items-center gap-1.5">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="requestExit"
        >
          <LuArrowLeft class="h-3.5 w-3.5" />
          <span>退出刮削</span>
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state hidden md:inline-flex"
          :title="sidebarCollapsed ? '展开左侧队列列表' : '收起左侧队列列表（最大化三栏宽度）'"
          @click="emit('toggleSidebar')"
        >
          <LuPanelLeft class="h-4 w-4" />
        </button>
      </div>

      <!-- Center: Stepper + Clickable Dropdown Queue Picker -->
      <div class="relative flex min-w-0 flex-1 items-center justify-center gap-1.5">
        <button
          type="button"
          :disabled="!hasPrev"
          class="md-icon-btn md-icon-btn-sm md-state !h-8 !w-8 shrink-0 disabled:opacity-35"
          title="上一项 (Alt+↑)"
          @click="requestStep(-1)"
        >
          <LuArrowUp class="h-4 w-4" />
        </button>

        <button
          type="button"
          class="md-state flex h-8 max-w-[min(420px,55vw)] min-w-0 items-center gap-2 rounded-full border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] px-3 text-xs font-semibold text-[var(--md-sys-color-on-surface)] transition hover:border-[var(--md-sys-color-primary)]"
          title="点击展开完整队列并快速跳转"
          @click="queueDropdownOpen = !queueDropdownOpen"
        >
          <span
            v-if="positionLabel"
            class="shrink-0 rounded-full bg-[var(--md-sys-color-primary-container)] px-1.5 py-0.5 font-mono text-[10px] font-bold text-[var(--md-sys-color-on-primary-container)]"
          >
            {{ positionLabel }}
          </span>
          <span class="min-w-0 flex-1 truncate">
            {{ detail?.entry.file_name || '未选择条目' }}
          </span>
          <span
            v-if="isCurrentDirty"
            class="shrink-0 rounded-full bg-[var(--md-sys-color-warning-container)] px-1.5 py-0.5 text-[9px] font-semibold text-[var(--md-sys-color-on-warning-container)]"
          >
            未保存
          </span>
          <LuChevronDown
            class="h-3.5 w-3.5 shrink-0 opacity-70 transition"
            :class="queueDropdownOpen ? 'rotate-180' : ''"
          />
        </button>

        <button
          type="button"
          :disabled="!hasNext"
          class="md-icon-btn md-icon-btn-sm md-state !h-8 !w-8 shrink-0 disabled:opacity-35"
          title="下一项 (Alt+↓)"
          @click="requestStep(1)"
        >
          <LuArrowDown class="h-4 w-4" />
        </button>

        <!-- Dropdown Queue Popover -->
        <template v-if="queueDropdownOpen">
          <div
            class="fixed inset-0 z-30"
            @click="queueDropdownOpen = false"
          />
          <div
            class="absolute top-10 left-1/2 z-40 flex max-h-[68vh] w-[min(440px,92vw)] -translate-x-1/2 flex-col overflow-hidden rounded-2xl border border-[var(--md-sys-color-outline-variant)]/75 bg-[var(--md-sys-color-surface-container-low)] shadow-[var(--md-sys-elevation-3)]"
          >
            <div class="shrink-0 space-y-2 border-b border-[var(--md-sys-color-outline-variant)]/45 p-3">
              <div class="flex items-center justify-between gap-2">
                <span class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
                  快速切换队列条目（{{ entries.length }} 项）
                </span>
                <button
                  type="button"
                  class="md-icon-btn md-icon-btn-sm md-state !h-6 !w-6"
                  @click="queueDropdownOpen = false"
                >
                  <LuX class="h-3.5 w-3.5" />
                </button>
              </div>

              <div
                class="flex h-8 items-center gap-1.5 rounded-full bg-[var(--md-sys-color-surface)] px-2.5"
              >
                <LuSearch class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-on-surface-variant)]" />
                <input
                  v-model="dropdownQuery"
                  type="text"
                  placeholder="搜索标题或文件名…"
                  class="min-w-0 flex-1 bg-transparent text-xs text-[var(--md-sys-color-on-surface)] outline-none"
                />
              </div>

              <div class="flex flex-wrap gap-1">
                <button
                  v-for="tab in [
                    { id: 'all' as const, label: '全部' },
                    { id: 'incomplete' as const, label: '未完善' },
                    { id: 'missing_cover' as const, label: '缺封面' },
                    { id: 'saved' as const, label: '本次已存' },
                  ]"
                  :key="tab.id"
                  type="button"
                  class="rounded-full px-2.5 py-0.5 text-[10px] font-semibold transition"
                  :class="
                    dropdownFilter === tab.id
                      ? 'bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]'
                      : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
                  "
                  @click="dropdownFilter = tab.id"
                >
                  {{ tab.label }}
                </button>
              </div>
            </div>

            <div class="min-h-0 flex-1 space-y-1 overflow-y-auto p-2">
              <div
                v-if="dropdownEntries.length === 0"
                class="py-8 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
              >
                无匹配条目
              </div>
              <button
                v-for="entry in dropdownEntries"
                :key="entry.rel_path"
                type="button"
                class="md-state flex w-full items-center gap-2.5 rounded-xl px-2.5 py-2 text-left transition"
                :class="
                  detail?.entry.rel_path === entry.rel_path
                    ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                    : 'text-[var(--md-sys-color-on-surface)] hover:bg-[var(--md-sys-color-surface-container-high)]'
                "
                @click="requestPickEntry(entry)"
              >
                <div
                  class="shrink-0 overflow-hidden rounded-md bg-[var(--md-sys-color-surface-container-high)]"
                  :class="entry.media_kind === 'music' ? 'h-9 w-9' : 'h-10 w-8'"
                >
                  <img
                    v-if="entry.completeness.has_cover"
                    :src="
                      buildArtworkUrl(
                        entry.root_id,
                        entry.rel_path,
                        entry.media_kind,
                        artworkVersion,
                        entry.media_kind === 'video' ? 'poster' : 'cover',
                      )
                    "
                    alt=""
                    loading="lazy"
                    class="h-full w-full object-cover"
                  />
                  <div
                    v-else
                    class="flex h-full w-full items-center justify-center text-[var(--md-sys-color-on-surface-variant)]/50"
                  >
                    <LuBookOpen v-if="entry.media_kind === 'book'" class="h-3.5 w-3.5" />
                    <LuLayers v-else-if="entry.media_kind === 'comic'" class="h-3.5 w-3.5" />
                    <LuDisc3 v-else-if="entry.media_kind === 'music'" class="h-3.5 w-3.5" />
                    <LuClapperboard v-else class="h-3.5 w-3.5" />
                  </div>
                </div>
                <div class="min-w-0 flex-1">
                  <div class="truncate text-xs font-semibold">
                    {{ entry.display_title || entry.file_name }}
                  </div>
                  <div class="truncate font-mono text-[10px] opacity-70">
                    {{ entry.file_name }}
                  </div>
                </div>
                <LuCheckCircle2
                  v-if="savedPaths.has(entry.rel_path)"
                  class="h-3.5 w-3.5 shrink-0 text-emerald-500"
                />
                <span
                  v-else-if="!entry.completeness.has_cover"
                  class="h-2 w-2 shrink-0 rounded-full bg-amber-400"
                />
              </button>
            </div>
          </div>
        </template>
      </div>

      <!-- Right: Progress, Auto Search, Advanced Modal -->
      <div class="flex shrink-0 items-center gap-2">
        <span
          class="hidden rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-1 font-mono text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)] sm:inline-flex"
        >
          已存 {{ savedSessionCount }} / {{ entries.length }}
        </span>

        <label class="hidden cursor-pointer items-center gap-1.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)] lg:inline-flex">
          <input
            v-model="autoSearchOnSwitch"
            type="checkbox"
            class="h-3.5 w-3.5 accent-[var(--md-sys-color-primary)]"
          />
          <span>切项自动搜</span>
        </label>

        <button
          v-if="detail"
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="多源高级刮削向导"
          @click="emit('openScrape', detail)"
        >
          <LuSparkles class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
        </button>
      </div>
    </div>

    <!-- Loading Overlay when Switching Items -->
    <div
      v-if="loadingDetail && !detail"
      class="flex flex-1 items-center justify-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      正在加载当前条目元数据…
    </div>

    <!-- Empty Queue Fallback -->
    <div
      v-else-if="!detail"
      class="flex flex-1 flex-col items-center justify-center gap-2 text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      <p>当前队列没有可编辑的条目</p>
      <button type="button" class="md-btn md-btn-sm md-btn-tonal md-state" @click="requestExit">
        返回媒体列表
      </button>
    </div>

    <!-- MAIN 3-COLUMN WORKBENCH: [Left: Current Item] | [Middle: Scraper Sources] | [Right: Editable Scraped Result] -->
    <div
      v-else
      class="grid min-h-0 flex-1 grid-cols-1 overflow-hidden lg:grid-cols-[minmax(0,1fr)_330px_minmax(0,1fr)] xl:grid-cols-[minmax(0,1.05fr)_370px_minmax(0,1.05fr)]"
    >
      <!-- COLUMN 1 (LEFT): Current Local Entry Metadata (Editable) -->
      <div class="flex min-h-0 flex-col overflow-hidden bg-[var(--md-sys-color-surface-container-low)]">
        <div
          class="flex shrink-0 items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)]/45 px-4 py-2.5"
        >
          <div class="flex min-w-0 items-center gap-1.5">
            <span
              class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-surface)]"
            >
              当前本地条目
            </span>
            <span class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ formatBytes(detail.entry.file_size) }}
            </span>
            <span
              v-if="isCurrentDirty"
              class="shrink-0 rounded-full bg-[var(--md-sys-color-warning-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-warning-container)]"
            >
              未保存
            </span>
          </div>
          <span class="shrink-0 font-mono text-[10px] uppercase text-[var(--md-sys-color-on-surface-variant)]">
            {{ detail.entry.extension }}
          </span>
        </div>

        <div class="flex-1 space-y-4 overflow-y-auto px-4 pt-3 pb-4">
          <!-- Local Cover & Quick Specs Card -->
          <div class="flex gap-3.5 rounded-2xl bg-[var(--md-sys-color-surface)] p-3.5">
            <div class="flex shrink-0 flex-col items-center gap-1.5">
              <div
                class="relative overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-high)]"
                :class="
                  detail.entry.media_kind === 'music'
                    ? 'h-24 w-24 album-shadow'
                    : 'h-32 w-24 book-shadow'
                "
              >
                <img
                  v-if="artworkSrc"
                  :src="artworkSrc"
                  alt="artwork"
                  referrerpolicy="no-referrer"
                  class="h-full w-full object-cover"
                />
                <div
                  v-else
                  class="flex h-full w-full flex-col items-center justify-center gap-1 text-[var(--md-sys-color-on-surface-variant)]"
                >
                  <LuBookOpen v-if="detail.entry.media_kind === 'book'" class="h-6 w-6 opacity-55" />
                  <LuLayers v-else-if="detail.entry.media_kind === 'comic'" class="h-6 w-6 opacity-55" />
                  <LuDisc3 v-else-if="detail.entry.media_kind === 'music'" class="h-6 w-6 opacity-55" />
                  <LuClapperboard v-else class="h-6 w-6 opacity-55" />
                  <span class="text-[10px]">无封面</span>
                </div>
                <span
                  v-if="pendingCoverUrl"
                  class="absolute inset-x-0 bottom-0 bg-[var(--md-sys-color-primary)]/90 py-0.5 text-center text-[9px] font-semibold text-[var(--md-sys-color-on-primary)]"
                >
                  待保存封面
                </span>
              </div>

              <div
                v-if="detail.metadata.kind === 'video'"
                class="flex items-center gap-0.5 rounded-full bg-[var(--md-sys-color-surface-container)] p-0.5 text-[10px]"
              >
                <button
                  v-for="slotTab in [
                    { id: 'poster' as const, label: '海报', ok: detail.metadata.data.has_poster },
                    { id: 'fanart' as const, label: '背景', ok: detail.metadata.data.has_fanart },
                    { id: 'thumb' as const, label: '剧照', ok: detail.metadata.data.has_thumb },
                  ]"
                  :key="slotTab.id"
                  type="button"
                  class="rounded-full px-2 py-0.5 font-medium transition"
                  :class="
                    videoArtworkSlot === slotTab.id
                      ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                      : 'text-[var(--md-sys-color-on-surface-variant)]'
                  "
                  @click="videoArtworkSlot = slotTab.id"
                >
                  {{ slotTab.label }}
                  <span v-if="slotTab.ok" class="text-emerald-500">·</span>
                </button>
              </div>
            </div>

            <div class="flex min-w-0 flex-1 flex-col justify-between gap-2.5">
              <div>
                <div class="flex flex-wrap items-center gap-1.5">
                  <span class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 font-mono text-[10px] font-semibold uppercase text-[var(--md-sys-color-on-primary-container)]">
                    {{ detail.entry.extension }}
                  </span>

                  <template v-if="detail.metadata.kind === 'music'">
                    <span
                      v-if="detail.metadata.data.duration_secs"
                      class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
                    >
                      {{ formatDuration(detail.metadata.data.duration_secs) }}
                    </span>
                    <span
                      v-if="detail.metadata.data.bit_rate_kbps"
                      class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
                    >
                      {{ detail.metadata.data.bit_rate_kbps }} kbps
                    </span>
                  </template>

                  <template v-else-if="detail.metadata.kind === 'comic'">
                    <span class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                      {{ detail.metadata.data.page_count }} 页
                    </span>
                  </template>

                  <template v-else-if="detail.metadata.kind === 'video'">
                    <span
                      v-if="detail.metadata.data.width && detail.metadata.data.height"
                      class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
                    >
                      {{ detail.metadata.data.width }}×{{ detail.metadata.data.height }}
                    </span>
                  </template>

                  <template v-else-if="detail.metadata.kind === 'book'">
                    <span class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                      {{
                        detail.metadata.data.format === 'epub'
                          ? 'EPUB 内封'
                          : `${detail.metadata.data.format.toUpperCase()} 旁路 .opf`
                      }}
                    </span>
                  </template>
                </div>
              </div>

              <div class="flex flex-wrap gap-1.5">
                <template v-if="detail.entry.media_kind !== 'video'">
                  <button
                    type="button"
                    :disabled="uploadingArtwork"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="triggerArtworkUpload('cover')"
                  >
                    <LuUpload class="h-3 w-3" />
                    <span>{{ detail.entry.media_kind === 'comic' ? '单话封面' : txt.actions.uploadCover }}</span>
                  </button>

                  <button
                    v-if="detail.metadata.kind === 'book' && detail.metadata.data.writable"
                    type="button"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    :class="isBookFieldLocked('cover') ? 'md-chip-selected' : ''"
                    @click="toggleBookFieldLock('cover')"
                  >
                    <LuLock v-if="isBookFieldLocked('cover')" class="h-3 w-3" />
                    <LuUnlock v-else class="h-3 w-3" />
                    <span>{{ isBookFieldLocked('cover') ? '封面已锁' : '锁封面' }}</span>
                  </button>

                  <button
                    v-if="
                      detail.entry.media_kind === 'book' &&
                      (detail.entry.extension === 'txt' || detail.entry.extension === 'epub')
                    "
                    type="button"
                    class="md-chip md-chip-selected md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="emit('openContentEditor', detail.entry)"
                  >
                    <LuFileText class="h-3 w-3" />
                    <span>{{ detail.entry.extension === 'txt' ? '正文与转 EPUB' : '编辑正文' }}</span>
                  </button>

                  <button
                    v-if="
                      detail.entry.media_kind === 'book' &&
                      (detail.entry.extension === 'txt' ||
                        detail.entry.extension === 'epub' ||
                        detail.entry.extension === 'pdf')
                    "
                    type="button"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    title="转为规范化压缩 EPUB 3.0（默认保留源文件）"
                    @click="emit('openNormalizeEpub', detail.entry)"
                  >
                    <LuPackageCheck class="h-3 w-3" />
                    <span>转规范化 EPUB</span>
                  </button>

                  <button
                    v-if="detail.entry.media_kind === 'comic'"
                    type="button"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="emit('openComicPages', detail.entry)"
                  >
                    <LuWand2 class="h-3 w-3" />
                    <span>页面瘦身</span>
                  </button>

                  <button
                    v-if="detail.entry.media_kind === 'music'"
                    type="button"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="emit('openMusicTools', detail.entry)"
                  >
                    <LuMusic class="h-3 w-3" />
                    <span>音频工具</span>
                  </button>
                </template>

                <template v-else>
                  <button
                    type="button"
                    :disabled="uploadingArtwork"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="triggerArtworkUpload(videoArtworkSlot)"
                  >
                    <LuUpload class="h-3 w-3" />
                    <span>上传{{ videoArtworkSlot === 'poster' ? '海报' : videoArtworkSlot === 'fanart' ? '背景' : '剧照' }}</span>
                  </button>
                  <button
                    type="button"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    :class="isVideoFieldLocked(videoArtworkSlot) ? 'md-chip-selected' : ''"
                    @click="toggleVideoFieldLock(videoArtworkSlot)"
                  >
                    <LuLock v-if="isVideoFieldLocked(videoArtworkSlot)" class="h-3 w-3" />
                    <LuUnlock v-else class="h-3 w-3" />
                    <span>锁图</span>
                  </button>
                  <button
                    type="button"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="emit('openVideoTools', detail.entry)"
                  >
                    <LuFilm class="h-3 w-3" />
                    <span>字幕/剧集</span>
                  </button>
                </template>
              </div>
            </div>
          </div>

          <div
            v-if="errorMsg"
            class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-3.5 py-2.5 text-xs text-[var(--md-sys-color-on-error-container)]"
          >
            {{ errorMsg }}
          </div>

          <!-- Left Editable Domain Metadata Form -->
          <InspectorMusicForm
            v-if="detail.metadata.kind === 'music'"
            :form="musicForm"
            :sidecar-lrc-filename="detail.metadata.data.sidecar_lrc_filename"
            :file-key="`${detail.entry.root_id}:${detail.entry.rel_path}`"
          />

          <InspectorVideoForm
            v-else-if="detail.metadata.kind === 'video'"
            :form="videoForm"
          />

          <InspectorComicForm
            v-else-if="detail.metadata.kind === 'comic'"
            :form="comicForm"
          />

          <InspectorBookForm
            v-else-if="detail.metadata.kind === 'book'"
            :form="bookForm"
            :book-format="detail.metadata.data.format"
            :writable="detail.metadata.data.writable"
            :is-epub="detail.entry.extension === 'epub'"
            :has-cover="detail.metadata.data.has_cover"
            :polishing-book="polishingBook"
            @polish="handlePolishBook"
          />
        </div>

        <!-- Left Bottom Action Bar -->
        <div
          class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container)]/75 px-4 py-2.5"
        >
          <span class="hidden text-[10px] text-[var(--md-sys-color-on-surface-variant)] xl:inline">
            ⌘S 保存 · ⌘↵ 下一项
          </span>

          <div class="ml-auto flex flex-wrap items-center gap-1.5">
            <button
              type="button"
              :disabled="!canSaveMetadata || saving"
              class="md-btn md-btn-sm md-btn-text md-state"
              title="保存左侧元数据并按默认模板重命名归档"
              @click="saveMetadata(true, false)"
            >
              <LuFileSignature class="h-3.5 w-3.5" />
              <span>保存并归档</span>
            </button>

            <button
              v-if="hasNext"
              type="button"
              :disabled="!canSaveMetadata || saving"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              @click="saveMetadata(false, true)"
            >
              <span>保存并下一项</span>
            </button>

            <button
              type="button"
              :disabled="!canSaveMetadata || saving"
              class="md-btn md-btn-sm md-btn-filled md-state"
              @click="saveMetadata(false, false)"
            >
              <LuRefreshCw v-if="saving" class="h-3.5 w-3.5 animate-spin" />
              <LuSave v-else class="h-3.5 w-3.5" />
              <span>{{ saving ? txt.actions.saving : '保存左侧' }}</span>
            </button>
          </div>
        </div>
      </div>

      <!-- COLUMN 2 (MIDDLE): Scraper Sources & Candidate List -->
      <div
        class="flex min-h-0 flex-col border-t border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container)]/40 lg:border-t-0 lg:border-x"
      >
        <div class="min-h-0 flex-1 overflow-hidden">
          <WorkbenchCandidatePane
            :detail="detail"
            :auto-search-trigger="autoSearchTrigger"
            :selected-preview-key="candidatePreview?.key ?? null"
            @fill-all="handleFillAll"
            @fill-field="handleFillField"
            @direct-apply="(m, mode) => handleDirectApply(m, mode, false)"
            @select-preview="candidatePreview = $event"
            @open-settings="emit('openSettingsScrapers')"
          />
        </div>
      </div>

      <!-- COLUMN 3 (RIGHT): Editable Selected Scraped Result -->
      <div class="flex min-h-0 flex-col overflow-hidden bg-[var(--md-sys-color-surface-container-low)]">
        <CandidateDetailPreviewPane
          v-if="candidatePreview"
          :media-kind="detail.entry.media_kind"
          :preview="candidatePreview"
          :has-next="hasNext"
          :closable="false"
          :saving="saving"
          @fill-all="handleFillAll"
          @fill-field="handlePreviewFillField"
          @direct-apply="(m, mode) => handleDirectApply(m, mode, false)"
          @direct-apply-next="(m, mode) => handleDirectApply(m, mode, true)"
        />
        <div
          v-else
          class="flex flex-1 flex-col items-center justify-center gap-2.5 px-8 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          <div
            class="flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-surface-container-high)]"
          >
            <LuImage class="h-5 w-5 opacity-55" />
          </div>
          <p class="font-semibold text-[var(--md-sys-color-on-surface)]">
            等待选择刮削源结果
          </p>
          <p class="max-w-[240px] text-[11px] leading-relaxed opacity-80">
            在中间搜索并点击「多源瀑布聚合结果」或任意候选源条目，此处会以与左侧一致的结构展示并允许你直接编辑后保存。
          </p>
        </div>
      </div>
    </div>
  </div>
</template>
