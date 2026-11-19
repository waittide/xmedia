<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  LuArrowDown,
  LuArrowLeft,
  LuArrowUp,
  LuBookOpen,
  LuCheck,
  LuChevronRight,
  LuClapperboard,
  LuColumns3,
  LuDisc3,
  LuFileSignature,
  LuFileText,
  LuFolder,
  LuFolderPlus,
  LuImage,
  LuLayers,
  LuList,
  LuLoader2,
  LuSave,
  LuSearch,
  LuSparkles,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { useDialogStore } from '@/stores/dialog'
import type { MediaKind } from '@/types/api'
import { buildArtworkUrl, resolveScraperCoverUrl, toolApi } from '../api'
import { formatBytes, useToolText } from '../i18n'
import type {
  DirectorySnapshot,
  ManagedRootDto,
  MediaFileDetail,
  MediaFileEntry,
  ScrapedMetadata,
} from '../types'
import InspectorBookForm from './inspector/InspectorBookForm.vue'
import InspectorComicForm from './inspector/InspectorComicForm.vue'
import InspectorMusicForm from './inspector/InspectorMusicForm.vue'
import InspectorVideoForm from './inspector/InspectorVideoForm.vue'
import {
  useMetadataForm,
  type ApplyFillMode,
  type ScrapedFieldKey,
} from './inspector/useMetadataForm'
import WorkbenchCandidatePane from './scrape/WorkbenchCandidatePane.vue'

const props = defineProps<{
  activeDomain: MediaKind
  currentRelPath: string
  domainRoots: ManagedRootDto[]
  domainLabels: {
    emptyRootsTitle: string
    emptyRootsDesc: string
  }
  breadcrumbs: Array<{ label: string; relPath: string }>
  viewMode: 'flat' | 'folders' | 'workbench'
  filteredFiles: MediaFileEntry[]
  selectedPaths: Set<string>
  checkedFiles: MediaFileEntry[]
  selectedComicEntries: MediaFileEntry[]
  selectedMusicEntries: MediaFileEntry[]
  selectedVideoEntries: MediaFileEntry[]
  loadingDir: boolean
  dirError: string | null
  snapshot: DirectorySnapshot | null
  activeDetail: MediaFileDetail | null
  artworkVersion: number
}>()

const emit = defineEmits<{
  openSettingsRoots: []
  openSettingsScrapers: []
  enterFolder: [relPath: string]
  setViewMode: [mode: 'flat' | 'folders' | 'workbench']
  toggleSelectAll: []
  clearSelection: []
  toggleSelectFile: [relPath: string, shiftKey?: boolean]
  inspectFile: [entry: MediaFileEntry]
  updatedDetail: [detail: MediaFileDetail, bumpArtwork: boolean]
  refreshDir: []
  enqueueScrapeSelection: []
  openDryRun: []
  openRename: []
  openAdvancedScrape: [detail: MediaFileDetail]
  openBookContentEditor: [entry: MediaFileEntry]
  openComicPageManager: [entry: MediaFileEntry]
  openComicSeriesTools: []
  openMusicToolbox: [entries: MediaFileEntry[]]
  openVideoToolbox: [entries: MediaFileEntry[]]
}>()

const txt = useToolText()
const dialogStore = useDialogStore()

const {
  bookForm,
  comicForm,
  musicForm,
  videoForm,
  pendingCoverUrl,
  pendingBackdropUrl,
  videoArtworkSlot,
  isDirty,
  hydrateForm,
  buildPatchEnvelope,
  applyScrapedField,
  applyScrapedMetadata,
} = useMetadataForm()

const leftFilterText = ref('')
const leftQuickFilter = ref<'all' | 'incomplete' | 'missing_cover'>('all')
const autoSearchOnSwitch = ref(false)
const autoSearchTrigger = ref(0)
const externalSearchKeyword = ref<string | undefined>(undefined)
const externalSearchArtist = ref<string | undefined>(undefined)

const saving = ref(false)
const polishingBook = ref(false)
const uploadingArtwork = ref(false)
const saveFeedback = ref<string | null>(null)
const errorMsg = ref<string | null>(null)
const coverInputRef = ref<HTMLInputElement | null>(null)
const pendingUploadSlot = ref<'cover' | 'series_cover' | 'poster' | 'fanart' | 'thumb'>('cover')
const skipNextHydrate = ref(false)

function completenessPercent(f: MediaFileEntry): number {
  const c = f.completeness
  const total = 5
  const ok =
    Number(c.has_title) +
    Number(c.has_creator) +
    Number(c.has_cover) +
    Number(c.has_summary_or_lyrics) +
    Number(c.has_nfo_or_tag_block)
  return Math.round((ok / total) * 100)
}

const workbenchFiles = computed<MediaFileEntry[]>(() => {
  const q = leftFilterText.value.trim().toLowerCase()
  return props.filteredFiles.filter((f) => {
    if (leftQuickFilter.value === 'incomplete' && completenessPercent(f) >= 100) return false
    if (leftQuickFilter.value === 'missing_cover' && f.completeness.has_cover) return false
    if (!q) return true
    return (
      f.file_name.toLowerCase().includes(q) ||
      (f.display_title || '').toLowerCase().includes(q) ||
      (f.display_subtitle || '').toLowerCase().includes(q) ||
      f.rel_path.toLowerCase().includes(q)
    )
  })
})

const activeIndex = computed(() => {
  if (!props.activeDetail) return -1
  return workbenchFiles.value.findIndex((f) => f.rel_path === props.activeDetail?.entry.rel_path)
})

const isMultiSelectMode = computed(() => props.selectedPaths.size >= 2)

const allVisibleSelected = computed(
  () =>
    workbenchFiles.value.length > 0 &&
    workbenchFiles.value.every((f) => props.selectedPaths.has(f.rel_path)),
)

const canSaveMetadata = computed(() => {
  if (!props.activeDetail) return false
  if (props.activeDetail.metadata.kind === 'book') {
    return props.activeDetail.metadata.data.writable
  }
  return true
})

const displayArtworkSrc = computed<string | null>(() => {
  if (pendingCoverUrl.value) {
    return resolveScraperCoverUrl(pendingCoverUrl.value)
  }
  if (!props.activeDetail) return null
  if (props.activeDetail.metadata.kind === 'video') {
    const v = props.activeDetail.metadata.data
    const slot = videoArtworkSlot.value
    const exists =
      slot === 'poster' ? v.has_poster : slot === 'fanart' ? v.has_fanart : v.has_thumb
    if (!exists) return null
    return buildArtworkUrl(
      props.activeDetail.entry.root_id,
      props.activeDetail.entry.rel_path,
      props.activeDetail.entry.media_kind,
      props.artworkVersion,
      slot,
    )
  }
  if (!props.activeDetail.entry.completeness.has_cover) return null
  return buildArtworkUrl(
    props.activeDetail.entry.root_id,
    props.activeDetail.entry.rel_path,
    props.activeDetail.entry.media_kind,
    props.artworkVersion,
  )
})

watch(
  () => props.activeDetail,
  (next, prev) => {
    if (!next) return
    if (skipNextHydrate.value) {
      skipNextHydrate.value = false
      return
    }
    hydrateForm(next)
    errorMsg.value = null
    if (autoSearchOnSwitch.value && next.entry.rel_path !== prev?.entry.rel_path) {
      externalSearchKeyword.value = undefined
      externalSearchArtist.value = undefined
      autoSearchTrigger.value += 1
    }
  },
  { immediate: true },
)

// Auto-select first file when entering workbench with no active detail
watch(
  () => [props.filteredFiles, props.activeDetail] as const,
  ([files, detail]) => {
    if (!detail && files.length > 0 && !props.loadingDir) {
      emit('inspectFile', files[0]!)
    }
  },
  { immediate: true },
)

async function confirmDiscardIfDirty(): Promise<boolean> {
  if (!isDirty.value) return true
  return dialogStore.confirm({
    title: '放弃未保存的修改？',
    message: '当前文件存在尚未保存的元数据或封面改动，切换后改动将丢失。',
    confirmText: '放弃并切换',
    danger: true,
  })
}

async function selectFile(entry: MediaFileEntry): Promise<void> {
  if (props.activeDetail?.entry.rel_path === entry.rel_path) return
  if (!(await confirmDiscardIfDirty())) return
  emit('inspectFile', entry)
}

async function stepFile(delta: -1 | 1): Promise<void> {
  if (workbenchFiles.value.length === 0) return
  const nextIdx = activeIndex.value + delta
  if (nextIdx < 0 || nextIdx >= workbenchFiles.value.length) return
  await selectFile(workbenchFiles.value[nextIdx]!)
}

function triggerSearchFromForm(): void {
  if (!props.activeDetail) return
  const kind = props.activeDetail.metadata.kind
  if (kind === 'book') {
    externalSearchKeyword.value = bookForm.title
    externalSearchArtist.value = bookForm.authors.split(/[/；;、]/)[0]?.trim() || ''
  } else if (kind === 'comic') {
    externalSearchKeyword.value = comicForm.series || comicForm.title
    externalSearchArtist.value = comicForm.writers.split(/[/；;、]/)[0]?.trim() || ''
  } else if (kind === 'music') {
    externalSearchKeyword.value = musicForm.title
    externalSearchArtist.value = musicForm.artist.split(/[/；;、]/)[0]?.trim() || ''
  } else {
    externalSearchKeyword.value = videoForm.isEpisode
      ? videoForm.showTitle || videoForm.title
      : videoForm.title
    externalSearchArtist.value = ''
  }
  autoSearchTrigger.value += 1
}

function handleFillAll(meta: ScrapedMetadata, mode: ApplyFillMode): void {
  if (!props.activeDetail) return
  const count = applyScrapedMetadata(props.activeDetail.entry.media_kind, meta, mode)
  saveFeedback.value = `已回填 ${count} 项字段到表单，确认后点击「保存」落盘`
  setTimeout(() => {
    saveFeedback.value = null
  }, 2600)
}

function handleFillField(field: ScrapedFieldKey, meta: ScrapedMetadata): void {
  if (!props.activeDetail) return
  applyScrapedField(props.activeDetail.entry.media_kind, field, meta, 'overwrite')
}

async function handleDirectApply(meta: ScrapedMetadata, mode: ApplyFillMode): Promise<void> {
  if (!props.activeDetail || saving.value) return
  saving.value = true
  errorMsg.value = null
  try {
    const updated = await toolApi.applyScrapedMetadata(
      props.activeDetail.entry.root_id,
      props.activeDetail.entry.rel_path,
      props.activeDetail.entry.media_kind,
      meta,
      true,
      props.activeDetail.entry.media_kind === 'music',
      false,
      mode === 'fill_missing',
      false,
    )
    emit('updatedDetail', updated, true)
    hydrateForm(updated)
    saveFeedback.value = '已直接写入元数据与封面'
    setTimeout(() => {
      saveFeedback.value = null
    }, 2400)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    saving.value = false
  }
}

async function saveCurrent(advanceNext = false): Promise<void> {
  if (!props.activeDetail || !canSaveMetadata.value || saving.value) return
  saving.value = true
  errorMsg.value = null
  saveFeedback.value = null
  try {
    const detail = props.activeDetail
    const envelope = buildPatchEnvelope(detail.entry.media_kind)
    let updated = await toolApi.writeMetadata(detail.entry.root_id, detail.entry.rel_path, envelope)
    let bumpedArtwork = false

    if (pendingCoverUrl.value) {
      const slot = detail.entry.media_kind === 'video' ? 'poster' : 'cover'
      updated = await toolApi.uploadArtworkFromUrl(
        updated.entry.root_id,
        updated.entry.rel_path,
        pendingCoverUrl.value,
        slot,
        updated.entry.media_kind,
      )
      bumpedArtwork = true
    }
    if (detail.entry.media_kind === 'video' && pendingBackdropUrl.value) {
      try {
        updated = await toolApi.uploadArtworkFromUrl(
          updated.entry.root_id,
          updated.entry.rel_path,
          pendingBackdropUrl.value,
          'fanart',
          'video',
        )
        bumpedArtwork = true
      } catch {
        // Fanart download is best-effort
      }
    }

    pendingCoverUrl.value = null
    pendingBackdropUrl.value = null
    hydrateForm(updated)
    emit('updatedDetail', updated, bumpedArtwork)
    saveFeedback.value = txt.value.actions.saved
    setTimeout(() => {
      saveFeedback.value = null
    }, 2000)

    if (advanceNext && activeIndex.value >= 0 && activeIndex.value + 1 < workbenchFiles.value.length) {
      emit('inspectFile', workbenchFiles.value[activeIndex.value + 1]!)
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    saving.value = false
  }
}

async function handlePolishBook(): Promise<void> {
  if (!props.activeDetail || polishingBook.value || props.activeDetail.entry.extension !== 'epub') {
    return
  }
  polishingBook.value = true
  errorMsg.value = null
  try {
    if (isDirty.value) {
      await saveCurrent(false)
    }
    const res = await toolApi.polishBook(
      props.activeDetail.entry.root_id,
      props.activeDetail.entry.rel_path,
    )
    emit('updatedDetail', res.detail, false)
    saveFeedback.value = '已更新 EPUB 内封简介页并同步目录'
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    polishingBook.value = false
  }
}

function triggerArtworkUpload(
  slot: 'cover' | 'series_cover' | 'poster' | 'fanart' | 'thumb',
): void {
  pendingUploadSlot.value = slot
  coverInputRef.value?.click()
}

async function handleArtworkPicked(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file || !props.activeDetail) return
  uploadingArtwork.value = true
  errorMsg.value = null
  try {
    const next = await toolApi.uploadArtwork(
      props.activeDetail.entry.root_id,
      props.activeDetail.entry.rel_path,
      file,
      pendingUploadSlot.value,
      props.activeDetail.entry.media_kind,
    )
    pendingCoverUrl.value = null
    skipNextHydrate.value = true
    emit('updatedDetail', next, true)
    saveFeedback.value = '封面已上传'
    setTimeout(() => {
      saveFeedback.value = null
    }, 2000)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    uploadingArtwork.value = false
  }
}

function handleKeydown(ev: KeyboardEvent): void {
  if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === 's') {
    if (props.activeDetail && !isMultiSelectMode.value) {
      ev.preventDefault()
      void saveCurrent(false)
    }
    return
  }
  if ((ev.ctrlKey || ev.metaKey) && ev.key === 'Enter') {
    if (props.activeDetail && !isMultiSelectMode.value) {
      ev.preventDefault()
      void saveCurrent(true)
    }
    return
  }
  if (ev.altKey && ev.key === 'ArrowUp') {
    ev.preventDefault()
    void stepFile(-1)
  } else if (ev.altKey && ev.key === 'ArrowDown') {
    ev.preventDefault()
    void stepFile(1)
  }
}

onMounted(() => window.addEventListener('keydown', handleKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', handleKeydown))
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <!-- Empty Roots Guard -->
    <div
      v-if="domainRoots.length === 0"
      class="mx-auto mt-12 max-w-lg rounded-3xl border border-dashed border-[var(--ctp-surface1)] bg-[var(--ctp-mantle)]/45 p-8 text-center"
    >
      <div
        class="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
      >
        <LuFolderPlus class="h-6 w-6" />
      </div>
      <h2 class="text-base font-bold text-[var(--ctp-text)]">{{ domainLabels.emptyRootsTitle }}</h2>
      <p class="mt-2 text-xs leading-relaxed text-[var(--ctp-subtext0)]">
        {{ domainLabels.emptyRootsDesc }}
      </p>
      <button
        type="button"
        class="mt-5 inline-flex items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-5 py-2.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95"
        @click="emit('openSettingsRoots')"
      >
        <LuFolderPlus class="h-4 w-4" />
        <span>{{ txt.roots.add }}</span>
      </button>
    </div>

    <template v-else>
      <!-- Compact Top Bar: Breadcrumbs + View Switcher + Keyboard Hints -->
      <div
        class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/45 px-4 py-2"
      >
        <div class="flex min-w-0 flex-wrap items-center gap-1 text-xs">
          <template v-for="(bc, idx) in breadcrumbs" :key="bc.relPath || '__root__'">
            <LuChevronRight
              v-if="idx > 0"
              class="h-3.5 w-3.5 shrink-0 text-[var(--ctp-subtext0)]"
            />
            <button
              type="button"
              class="rounded-lg px-2 py-0.5 font-medium transition hover:bg-[var(--ctp-surface0)]/65"
              :class="
                idx === breadcrumbs.length - 1
                  ? 'font-bold text-[var(--ctp-text)]'
                  : 'text-[var(--ctp-subtext1)]'
              "
              @click="emit('enterFolder', bc.relPath)"
            >
              {{ bc.label }}
            </button>
          </template>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <!-- Auto-search on switch toggle -->
          <label
            class="hidden cursor-pointer items-center gap-1.5 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1 text-[11px] text-[var(--ctp-subtext1)] xl:inline-flex"
            title="切换文件时自动在右栏发起在线搜索"
          >
            <input
              v-model="autoSearchOnSwitch"
              type="checkbox"
              class="h-3.5 w-3.5 accent-[var(--ctp-accent)]"
            />
            <span>切项自动搜</span>
          </label>

          <!-- View Mode Switcher -->
          <div
            class="flex items-center gap-0.5 rounded-xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-canvas)] p-0.5"
          >
            <button
              type="button"
              class="inline-flex items-center gap-1 rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
              :class="
                viewMode === 'flat'
                  ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
              "
              @click="emit('setViewMode', 'flat')"
            >
              <LuList class="h-3.5 w-3.5" />
              <span>全部媒体</span>
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-1 rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
              :class="
                viewMode === 'folders'
                  ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
              "
              @click="emit('setViewMode', 'folders')"
            >
              <LuFolder class="h-3.5 w-3.5" />
              <span>文件夹</span>
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-1 rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
              :class="
                viewMode === 'workbench'
                  ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
              "
              title="三栏同屏刮削工作台：左栏选文件，中栏精修元数据，右栏在线候选单字段回填"
              @click="emit('setViewMode', 'workbench')"
            >
              <LuColumns3 class="h-3.5 w-3.5" />
              <span>三栏刮削台</span>
            </button>
          </div>

          <RefreshButton :action="() => emit('refreshDir')" />
        </div>
      </div>

      <!-- Error Banner -->
      <div
        v-if="dirError"
        class="mx-4 mt-2 rounded-xl bg-red-500/12 px-3 py-2 text-xs text-red-400"
      >
        {{ dirError }}
      </div>

      <!-- 3-Pane Workbench Body -->
      <div class="grid min-h-0 flex-1 grid-cols-1 lg:grid-cols-[270px_minmax(0,1fr)_340px] xl:grid-cols-[300px_minmax(0,1fr)_390px]">
        <!-- PANE 1 (LEFT): Directory + Compact File List -->
        <section
          class="flex min-h-0 flex-col border-b border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/25 lg:border-r lg:border-b-0"
        >
          <div class="shrink-0 space-y-2 border-b border-[var(--ctp-surface0)]/75 p-2.5">
            <div class="relative">
              <LuSearch
                class="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-[var(--ctp-subtext0)]"
              />
              <input
                v-model="leftFilterText"
                type="text"
                placeholder="筛选文件名 / 标题…"
                class="h-8 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] pr-7 pl-8 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
              <button
                v-if="leftFilterText"
                type="button"
                class="absolute top-1/2 right-2 -translate-y-1/2 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]"
                @click="leftFilterText = ''"
              >
                <LuX class="h-3.5 w-3.5" />
              </button>
            </div>

            <div class="flex items-center justify-between gap-1">
              <div class="flex items-center gap-1">
                <button
                  type="button"
                  class="rounded-lg px-2 py-0.5 text-[10px] font-semibold transition"
                  :class="
                    leftQuickFilter === 'all'
                      ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                      : 'bg-[var(--ctp-surface0)]/60 text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                  "
                  @click="leftQuickFilter = 'all'"
                >
                  全部
                </button>
                <button
                  type="button"
                  class="rounded-lg px-2 py-0.5 text-[10px] font-semibold transition"
                  :class="
                    leftQuickFilter === 'incomplete'
                      ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                      : 'bg-[var(--ctp-surface0)]/60 text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                  "
                  @click="leftQuickFilter = 'incomplete'"
                >
                  待补全
                </button>
                <button
                  type="button"
                  class="rounded-lg px-2 py-0.5 text-[10px] font-semibold transition"
                  :class="
                    leftQuickFilter === 'missing_cover'
                      ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                      : 'bg-[var(--ctp-surface0)]/60 text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                  "
                  @click="leftQuickFilter = 'missing_cover'"
                >
                  缺封面
                </button>
              </div>

              <div class="flex items-center gap-1.5">
                <AppCheckbox
                  v-if="workbenchFiles.length > 0"
                  :model-value="allVisibleSelected"
                  :label="selectedPaths.size > 0 ? `已选 ${selectedPaths.size}` : '全选'"
                  @change="emit('toggleSelectAll')"
                />
                <button
                  v-if="selectedPaths.size > 0"
                  type="button"
                  class="text-[10px] text-[var(--ctp-subtext0)] hover:text-[var(--ctp-accent)]"
                  @click="emit('clearSelection')"
                >
                  清空
                </button>
              </div>
            </div>
          </div>

          <!-- Folders & Files Scrollable List -->
          <div class="min-h-0 flex-1 overflow-y-auto p-2 space-y-1">
            <!-- Parent folder link -->
            <button
              v-if="currentRelPath"
              type="button"
              class="flex w-full items-center gap-2 rounded-xl px-2.5 py-1.5 text-left text-xs text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
              @click="emit('enterFolder', currentRelPath.split('/').slice(0, -1).join('/'))"
            >
              <LuArrowLeft class="h-3.5 w-3.5 shrink-0" />
              <span class="truncate font-medium">返回上级目录</span>
            </button>

            <!-- Subfolders -->
            <button
              v-for="folder in snapshot?.folders ?? []"
              :key="folder.rel_path"
              type="button"
              class="flex w-full items-center gap-2 rounded-xl px-2.5 py-1.5 text-left text-xs text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
              @click="emit('enterFolder', folder.rel_path)"
            >
              <LuFolder class="h-3.5 w-3.5 shrink-0 text-[var(--ctp-accent)]" />
              <span class="min-w-0 flex-1 truncate font-medium">{{ folder.name }}</span>
              <span class="text-[10px] text-[var(--ctp-subtext0)]">{{ folder.media_file_count }}</span>
            </button>

            <div
              v-if="loadingDir && workbenchFiles.length === 0"
              class="flex items-center justify-center gap-2 py-10 text-xs text-[var(--ctp-subtext0)]"
            >
              <LuLoader2 class="h-4 w-4 animate-spin text-[var(--ctp-accent)]" />
              <span>扫描目录中…</span>
            </div>

            <div
              v-else-if="workbenchFiles.length === 0"
              class="py-10 text-center text-xs text-[var(--ctp-subtext0)]"
            >
              当前筛选下无媒体文件
            </div>

            <!-- File Rows -->
            <div
              v-for="file in workbenchFiles"
              :key="file.rel_path"
              class="group flex cursor-pointer items-center gap-2 rounded-xl border px-2 py-1.5 transition"
              :class="
                activeDetail?.entry.rel_path === file.rel_path
                  ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)]/55'
                  : 'border-transparent hover:border-[var(--ctp-surface0)] hover:bg-[var(--ctp-surface0)]/40'
              "
              @click="selectFile(file)"
            >
              <div class="shrink-0" @click.stop>
                <AppCheckbox
                  :model-value="selectedPaths.has(file.rel_path)"
                  @change="emit('toggleSelectFile', file.rel_path, false)"
                />
              </div>

              <!-- Mini Thumbnail -->
              <div
                class="h-9 w-7 shrink-0 overflow-hidden rounded-md border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]"
              >
                <img
                  v-if="file.completeness.has_cover"
                  :src="buildArtworkUrl(file.root_id, file.rel_path, file.media_kind, artworkVersion)"
                  alt=""
                  loading="lazy"
                  class="h-full w-full object-cover"
                />
                <div
                  v-else
                  class="flex h-full w-full items-center justify-center text-[var(--ctp-subtext0)]/45"
                >
                  <LuImage class="h-3.5 w-3.5" />
                </div>
              </div>

              <div class="min-w-0 flex-1">
                <div class="truncate text-xs font-semibold text-[var(--ctp-text)]">
                  {{ file.display_title || file.file_name }}
                </div>
                <div class="flex items-center gap-1.5 text-[10px] text-[var(--ctp-subtext0)]">
                  <span class="truncate">{{ file.display_subtitle || file.file_name }}</span>
                </div>
              </div>

              <!-- Completeness Dot -->
              <span
                class="h-2 w-2 shrink-0 rounded-full"
                :class="
                  completenessPercent(file) >= 100
                    ? 'bg-emerald-500'
                    : completenessPercent(file) >= 60
                      ? 'bg-amber-400'
                      : 'bg-rose-400'
                "
                :title="`完整度 ${completenessPercent(file)}%`"
              />
            </div>
          </div>

          <div
            class="shrink-0 border-t border-[var(--ctp-surface0)]/75 px-3 py-1.5 text-[10px] text-[var(--ctp-subtext0)]"
          >
            共 {{ workbenchFiles.length }} 项 · Alt+↑/↓ 切换 · Ctrl+Enter 保存并下一项
          </div>
        </section>

        <!-- PANE 2 (CENTER): Persistent Metadata Editor OR Multi-Select Batch Panel -->
        <section class="flex min-h-0 flex-col bg-[var(--ctp-canvas)]">
          <!-- Multi-Select Batch Mode -->
          <div
            v-if="isMultiSelectMode"
            class="flex min-h-0 flex-1 flex-col overflow-y-auto p-5 space-y-4"
          >
            <div
              class="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--ctp-accent)]/40 bg-[var(--ctp-accent-soft)]/30 p-4"
            >
              <div>
                <h3 class="text-sm font-bold text-[var(--ctp-text)]">
                  已勾选 {{ checkedFiles.length }} 个文件 · 批量整理模式
                </h3>
                <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
                  可直接发起批量自动刮削、先预演置信度，或批量按模板重命名归档。取消勾选回到单文件精修。
                </p>
              </div>
              <button
                type="button"
                class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
                @click="emit('clearSelection')"
              >
                取消多选
              </button>
            </div>

            <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
              <button
                type="button"
                class="flex items-center gap-3 rounded-2xl border border-[var(--ctp-accent)]/50 bg-[var(--ctp-accent)] px-4 py-3 text-left text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95"
                @click="emit('enqueueScrapeSelection')"
              >
                <LuSparkles class="h-5 w-5 shrink-0" />
                <div>
                  <div class="text-xs font-bold">批量自动刮削</div>
                  <div class="text-[11px] opacity-85">按策略配置自动匹配元数据与封面</div>
                </div>
              </button>

              <button
                type="button"
                class="flex items-center gap-3 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/60 px-4 py-3 text-left text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
                @click="emit('openDryRun')"
              >
                <LuSearch class="h-5 w-5 shrink-0 text-[var(--ctp-accent)]" />
                <div>
                  <div class="text-xs font-bold">刮削置信度预演</div>
                  <div class="text-[11px] text-[var(--ctp-subtext0)]">先试跑匹配分数，确认后再写入</div>
                </div>
              </button>

              <button
                type="button"
                class="flex items-center gap-3 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/60 px-4 py-3 text-left text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
                @click="emit('openRename')"
              >
                <LuFileSignature class="h-5 w-5 shrink-0 text-[var(--ctp-accent)]" />
                <div>
                  <div class="text-xs font-bold">批量重命名与归档</div>
                  <div class="text-[11px] text-[var(--ctp-subtext0)]">按当前领域模板规范目录与文件名</div>
                </div>
              </button>

              <button
                v-if="activeDomain === 'comic' && selectedComicEntries.length > 0"
                type="button"
                class="flex items-center gap-3 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/60 px-4 py-3 text-left text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
                @click="emit('openComicSeriesTools')"
              >
                <LuLayers class="h-5 w-5 shrink-0 text-[var(--ctp-accent)]" />
                <div>
                  <div class="text-xs font-bold">漫画系列工具箱</div>
                  <div class="text-[11px] text-[var(--ctp-subtext0)]">同步系列信息、合并卷册与压图</div>
                </div>
              </button>

              <button
                v-if="activeDomain === 'music' && selectedMusicEntries.length > 0"
                type="button"
                class="flex items-center gap-3 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/60 px-4 py-3 text-left text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
                @click="emit('openMusicToolbox', selectedMusicEntries)"
              >
                <LuDisc3 class="h-5 w-5 shrink-0 text-[var(--ctp-accent)]" />
                <div>
                  <div class="text-xs font-bold">音乐专辑与歌词工具箱</div>
                  <div class="text-[11px] text-[var(--ctp-subtext0)]">统一专辑标签、歌词处理与 ReplayGain</div>
                </div>
              </button>

              <button
                v-if="activeDomain === 'video' && selectedVideoEntries.length > 0"
                type="button"
                class="flex items-center gap-3 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/60 px-4 py-3 text-left text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
                @click="emit('openVideoToolbox', selectedVideoEntries)"
              >
                <LuClapperboard class="h-5 w-5 shrink-0 text-[var(--ctp-accent)]" />
                <div>
                  <div class="text-xs font-bold">影视字幕与集数工具箱</div>
                  <div class="text-[11px] text-[var(--ctp-subtext0)]">批量重映射季集号与字幕处理</div>
                </div>
              </button>
            </div>

            <!-- Selected Files Preview -->
            <div class="rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/40 p-3">
              <div class="mb-2 text-xs font-bold text-[var(--ctp-subtext1)]">已选文件列表</div>
              <div class="max-h-60 space-y-1 overflow-y-auto text-xs">
                <div
                  v-for="item in checkedFiles"
                  :key="item.rel_path"
                  class="flex items-center justify-between gap-2 rounded-lg px-2 py-1 hover:bg-[var(--ctp-surface0)]/40"
                >
                  <span class="truncate text-[var(--ctp-text)]">{{ item.display_title || item.file_name }}</span>
                  <span class="shrink-0 font-mono text-[10px] text-[var(--ctp-subtext0)]">
                    {{ completenessPercent(item) }}%
                  </span>
                </div>
              </div>
            </div>
          </div>

          <!-- Empty Selection -->
          <div
            v-else-if="!activeDetail"
            class="flex flex-1 flex-col items-center justify-center p-8 text-center text-xs text-[var(--ctp-subtext0)]"
          >
            <LuColumns3 class="mb-2 h-8 w-8 opacity-35" />
            <p class="font-semibold text-[var(--ctp-text)]">三栏刮削工作台</p>
            <p class="mt-1">在左栏点击任意文件即可在此直接编辑元数据，并在右栏对照在线候选单字段回填</p>
          </div>

          <!-- Single-File Persistent Editor -->
          <template v-else>
            <!-- Editor Sticky Action Bar -->
            <div
              class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/35 px-4 py-2.5"
            >
              <div class="flex items-center gap-1.5">
                <button
                  type="button"
                  :disabled="saving || !canSaveMetadata"
                  class="inline-flex h-8 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-3.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-45"
                  title="保存当前元数据与待写入封面 (Ctrl+S)"
                  @click="saveCurrent(false)"
                >
                  <LuLoader2 v-if="saving" class="h-3.5 w-3.5 animate-spin" />
                  <LuSave v-else class="h-3.5 w-3.5" />
                  <span>保存</span>
                  <span
                    v-if="isDirty"
                    class="h-1.5 w-1.5 rounded-full bg-amber-300"
                    title="有未保存改动"
                  />
                </button>

                <button
                  type="button"
                  :disabled="saving || !canSaveMetadata || activeIndex < 0 || activeIndex >= workbenchFiles.length - 1"
                  class="inline-flex h-8 items-center gap-1 rounded-xl border border-[var(--ctp-accent)]/50 bg-[var(--ctp-accent-soft)]/50 px-2.5 text-xs font-semibold text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent)] hover:text-[var(--ctp-on-accent)] disabled:opacity-40"
                  title="保存并自动跳到下一条 (Ctrl+Enter)"
                  @click="saveCurrent(true)"
                >
                  <LuCheck class="h-3.5 w-3.5" />
                  <span>保存并下一项</span>
                </button>

                <button
                  type="button"
                  class="inline-flex h-8 items-center gap-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
                  title="用当前表单中的标题与作者在右栏搜索候选"
                  @click="triggerSearchFromForm"
                >
                  <LuSearch class="h-3.5 w-3.5" />
                  <span>搜右栏</span>
                </button>
              </div>

              <div class="flex items-center gap-1">
                <button
                  type="button"
                  :disabled="activeIndex <= 0"
                  class="flex h-7 w-7 items-center justify-center rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)] disabled:opacity-35"
                  title="上一项 (Alt+↑)"
                  @click="stepFile(-1)"
                >
                  <LuArrowUp class="h-3.5 w-3.5" />
                </button>
                <span class="px-1 font-mono text-[10px] text-[var(--ctp-subtext0)]">
                  {{ activeIndex >= 0 ? activeIndex + 1 : '-' }}/{{ workbenchFiles.length }}
                </span>
                <button
                  type="button"
                  :disabled="activeIndex < 0 || activeIndex >= workbenchFiles.length - 1"
                  class="flex h-7 w-7 items-center justify-center rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)] disabled:opacity-35"
                  title="下一项 (Alt+↓)"
                  @click="stepFile(1)"
                >
                  <LuArrowDown class="h-3.5 w-3.5" />
                </button>

                <button
                  type="button"
                  class="ml-1 inline-flex h-7 items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
                  title="打开完整多源聚合对比弹窗"
                  @click="emit('openAdvancedScrape', activeDetail)"
                >
                  <LuSparkles class="h-3 w-3" />
                  <span>高级多源</span>
                </button>
              </div>
            </div>

            <!-- Feedback Banners -->
            <div v-if="saveFeedback || errorMsg" class="shrink-0 px-4 pt-2">
              <div
                v-if="saveFeedback"
                class="rounded-xl bg-emerald-500/12 px-3 py-1.5 text-xs font-medium text-emerald-500"
              >
                {{ saveFeedback }}
              </div>
              <div
                v-if="errorMsg"
                class="rounded-xl bg-red-500/12 px-3 py-1.5 text-xs text-red-400"
              >
                {{ errorMsg }}
              </div>
            </div>

            <!-- Scrollable Metadata Form Body -->
            <div class="min-h-0 flex-1 overflow-y-auto p-4 space-y-4">
              <!-- Compact File Header + Cover Card -->
              <div
                class="flex flex-wrap items-start gap-3.5 rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/40 p-3"
              >
                <div class="relative shrink-0">
                  <div
                    class="h-24 w-[72px] overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] shadow-sm"
                  >
                    <img
                      v-if="displayArtworkSrc"
                      :src="displayArtworkSrc"
                      alt=""
                      class="h-full w-full object-cover"
                    />
                    <div
                      v-else
                      class="flex h-full w-full items-center justify-center text-[var(--ctp-subtext0)]/45"
                    >
                      <LuImage class="h-6 w-6" />
                    </div>
                  </div>
                  <span
                    v-if="pendingCoverUrl"
                    class="absolute -top-1.5 -right-1.5 rounded-full bg-amber-500 px-1.5 py-0.5 text-[9px] font-bold text-white shadow"
                  >
                    待保存
                  </span>
                </div>

                <div class="min-w-0 flex-1 space-y-1.5">
                  <div class="truncate text-xs font-bold text-[var(--ctp-text)]">
                    {{ activeDetail.entry.file_name }}
                  </div>
                  <div class="truncate font-mono text-[10px] text-[var(--ctp-subtext0)]">
                    {{ activeDetail.entry.rel_path }} · {{ formatBytes(activeDetail.entry.file_size) }}
                  </div>

                  <div class="flex flex-wrap items-center gap-1.5 pt-1">
                    <input
                      ref="coverInputRef"
                      type="file"
                      accept="image/*"
                      class="hidden"
                      @change="handleArtworkPicked"
                    />
                    <button
                      type="button"
                      :disabled="uploadingArtwork"
                      class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
                      @click="triggerArtworkUpload(activeDetail.entry.media_kind === 'video' ? 'poster' : 'cover')"
                    >
                      <LuUpload class="h-3 w-3" />
                      <span>上传封面</span>
                    </button>

                    <button
                      v-if="pendingCoverUrl"
                      type="button"
                      class="inline-flex items-center gap-1 rounded-lg border border-amber-500/50 bg-amber-500/10 px-2 py-1 text-[11px] font-medium text-amber-500 transition hover:bg-amber-500/20"
                      @click="pendingCoverUrl = null"
                    >
                      <LuX class="h-3 w-3" />
                      <span>撤销候选封面</span>
                    </button>

                    <button
                      v-if="activeDetail.entry.media_kind === 'book' && (activeDetail.entry.extension === 'epub' || activeDetail.entry.extension === 'txt')"
                      type="button"
                      class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
                      @click="emit('openBookContentEditor', activeDetail.entry)"
                    >
                      <LuBookOpen class="h-3 w-3" />
                      <span>正文编辑</span>
                    </button>

                    <button
                      v-if="activeDetail.entry.media_kind === 'comic'"
                      type="button"
                      class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
                      @click="emit('openComicPageManager', activeDetail.entry)"
                    >
                      <LuFileText class="h-3 w-3" />
                      <span>页面管理</span>
                    </button>

                    <button
                      v-if="activeDetail.entry.media_kind === 'music'"
                      type="button"
                      class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
                      @click="emit('openMusicToolbox', [activeDetail.entry])"
                    >
                      <LuDisc3 class="h-3 w-3" />
                      <span>音乐工具</span>
                    </button>

                    <button
                      v-if="activeDetail.entry.media_kind === 'video'"
                      type="button"
                      class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
                      @click="emit('openVideoToolbox', [activeDetail.entry])"
                    >
                      <LuClapperboard class="h-3 w-3" />
                      <span>影视工具</span>
                    </button>
                  </div>
                </div>
              </div>

              <!-- Domain-Specific Form -->
              <InspectorBookForm
                v-if="activeDetail.metadata.kind === 'book'"
                :form="bookForm"
                :book-format="activeDetail.metadata.data.format"
                :writable="activeDetail.metadata.data.writable"
                :is-epub="activeDetail.entry.extension === 'epub'"
                :has-cover="activeDetail.entry.completeness.has_cover"
                :polishing-book="polishingBook"
                @polish="handlePolishBook"
              />
              <InspectorComicForm
                v-else-if="activeDetail.metadata.kind === 'comic'"
                :form="comicForm"
              />
              <InspectorMusicForm
                v-else-if="activeDetail.metadata.kind === 'music'"
                :form="musicForm"
                :sidecar-lrc-filename="activeDetail.entry.file_name.replace(/\.[^.]+$/, '.lrc')"
                :file-key="activeDetail.entry.rel_path"
              />
              <InspectorVideoForm
                v-else
                :form="videoForm"
              />
            </div>
          </template>
        </section>

        <!-- PANE 3 (RIGHT): Live Online Candidates & Single-Field Fill -->
        <WorkbenchCandidatePane
          :detail="isMultiSelectMode ? null : activeDetail"
          :auto-search-trigger="autoSearchTrigger"
          :external-keyword="externalSearchKeyword"
          :external-artist="externalSearchArtist"
          @fill-all="handleFillAll"
          @fill-field="handleFillField"
          @direct-apply="handleDirectApply"
          @open-settings="emit('openSettingsScrapers')"
        />
      </div>
    </template>
  </div>
</template>
