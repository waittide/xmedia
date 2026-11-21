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
  LuEye,
  LuFileSignature,
  LuImage,
  LuLayers,
  LuLoader2,
  LuPanelLeft,
  LuPencil,
  LuSave,
  LuSearch,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { buildArtworkUrl, toolApi } from '../../api'
import { entryCompositeKey, isEntryComplete } from '../../completeness'
import { formatBytes, formatDuration } from '../../i18n'
import type {
  MediaFileDetail,
  MediaFileEntry,
  PublicSettingsDto,
  ScrapedMetadata,
} from '../../types'
import InspectorBookForm from '../inspector/InspectorBookForm.vue'
import InspectorComicForm from '../inspector/InspectorComicForm.vue'
import InspectorMusicForm from '../inspector/InspectorMusicForm.vue'
import InspectorVideoForm from '../inspector/InspectorVideoForm.vue'
import {
  useMetadataForm,
  type ApplyFillMode,
} from '../inspector/useMetadataForm'
import CandidateDetailPreviewPane, {
  type CandidateApplyOptions,
  type CandidatePreviewState,
} from './CandidateDetailPreviewPane.vue'
import WorkbenchCandidatePane from './WorkbenchCandidatePane.vue'

type DropdownFilter = 'all' | 'incomplete' | 'missing_cover' | 'saved'

const props = defineProps<{
  detail: MediaFileDetail | null
  loadingDetail: boolean
  entries: MediaFileEntry[]
  savedKeys: Set<string>
  artworkVersion: number
  artworkVersions?: Record<string, number>
  sidebarCollapsed: boolean
  hasPrev: boolean
  hasNext: boolean
  positionLabel: string
  settings?: PublicSettingsDto | null
}>()

const emit = defineEmits<{
  exit: []
  toggleSidebar: []
  selectEntry: [entry: MediaFileEntry]
  step: [delta: -1 | 1]
  updated: [detail: MediaFileDetail, bumpArtwork: boolean]
  openSettingsScrapers: []
  settingsUpdated: [settings: PublicSettingsDto]
  openHistory: []
}>()

const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()

// Dedicated form instance for direct left-column inline editing
const localForm = useMetadataForm()
// Separate patch builder for committing right-column scraped results
const applyForm = useMetadataForm()

const localEditMode = ref(false)
const saving = ref(false)
const savingLocal = ref(false)
const polishingBook = ref(false)
const uploadingArtwork = ref(false)
const errorMsg = ref<string | null>(null)
const videoArtworkSlot = ref<'poster' | 'fanart' | 'thumb'>('poster')
const coverInputRef = ref<HTMLInputElement | null>(null)
const pendingUploadSlot = ref<'cover' | 'poster' | 'fanart' | 'thumb'>('cover')
const previewPaneRef = ref<InstanceType<typeof CandidateDetailPreviewPane> | null>(null)

const autoSearchOnSwitch = ref(true)
const autoSearchTrigger = ref(0)
const candidatePreview = ref<CandidatePreviewState | null>(null)
let lastEntryKey = ''

// Compact queue dropdown (shown only on mobile or when desktop sidebar is collapsed)
const isMobileViewport = ref(typeof window !== 'undefined' ? window.innerWidth < 768 : false)
const showCompactQueueDropdown = computed(() => props.sidebarCollapsed || isMobileViewport.value)
const queueDropdownOpen = ref(false)
const dropdownQuery = ref('')
const dropdownFilter = ref<DropdownFilter>('all')

const dropdownFilterOptions = [
  { value: 'all' as const, label: '全部' },
  { value: 'incomplete' as const, label: '未完善' },
  { value: 'missing_cover' as const, label: '缺封面' },
  { value: 'saved' as const, label: '已存' },
]

watch(showCompactQueueDropdown, (visible) => {
  if (!visible) queueDropdownOpen.value = false
})

function handleResize(): void {
  isMobileViewport.value = window.innerWidth < 768
}

const dropdownEntries = computed(() => {
  const q = dropdownQuery.value.trim().toLowerCase()
  return props.entries.filter((e) => {
    const key = entryCompositeKey(e)
    if (dropdownFilter.value === 'incomplete' && isEntryComplete(e)) return false
    if (dropdownFilter.value === 'missing_cover' && e.completeness.has_cover) return false
    if (dropdownFilter.value === 'saved' && !props.savedKeys.has(key)) return false
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
    if (props.savedKeys.has(entryCompositeKey(e))) n += 1
  }
  return n
})

watch(
  () => (props.detail ? entryCompositeKey(props.detail.entry) : ''),
  (key) => {
    if (!key) {
      lastEntryKey = ''
      candidatePreview.value = null
      localEditMode.value = false
      errorMsg.value = null
      return
    }
    if (props.detail) {
      localForm.hydrateForm(props.detail)
    }
    if (key === lastEntryKey) return
    lastEntryKey = key
    errorMsg.value = null
    candidatePreview.value = null
    localEditMode.value = false
    videoArtworkSlot.value = 'poster'
    if (autoSearchOnSwitch.value) {
      autoSearchTrigger.value += 1
    }
  },
  { immediate: true },
)

const isLocalFormDirty = computed(() => localEditMode.value && localForm.isDirty.value)
const isCurrentDirty = computed(
  () => isLocalFormDirty.value || Boolean(previewPaneRef.value?.isEdited),
)

const canSaveMetadata = computed(() => {
  if (!props.detail) return false
  if (props.detail.metadata.kind === 'book') {
    return props.detail.metadata.data.writable
  }
  return true
})

const videoSlotOptions = computed(() => {
  if (props.detail?.metadata.kind !== 'video') return []
  const d = props.detail.metadata.data
  return [
    { value: 'poster' as const, label: d.has_poster ? '海报·' : '海报' },
    { value: 'fanart' as const, label: d.has_fanart ? '背景·' : '背景' },
    { value: 'thumb' as const, label: d.has_thumb ? '剧照·' : '剧照' },
  ]
})

function resolveArtworkVersion(rootId: string, relPath: string): number {
  return props.artworkVersion + (props.artworkVersions?.[`${rootId}::${relPath}`] ?? 0)
}

const artworkSrc = computed(() => {
  if (!props.detail) return ''
  const kind = props.detail.entry.media_kind
  const ver = resolveArtworkVersion(props.detail.entry.root_id, props.detail.entry.rel_path)
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
      ver,
      slot,
    )
  }
  if (!props.detail.entry.completeness.has_cover) return ''
  return buildArtworkUrl(
    props.detail.entry.root_id,
    props.detail.entry.rel_path,
    kind,
    ver,
    'cover',
  )
})

interface ReadOnlyFieldItem {
  id: string
  label: string
  value: string
  half?: boolean
  multiline?: boolean
}

const localReadOnlyFields = computed<ReadOnlyFieldItem[]>(() => {
  const d = props.detail
  if (!d) return []
  const m = d.metadata
  if (m.kind === 'book') {
    const b = m.data
    const seriesVal = [b.series, b.series_index ? `#${b.series_index}` : '']
      .filter(Boolean)
      .join(' ')
    return [
      { id: 'title', label: '书名 (dc:title)', value: b.title },
      { id: 'authors', label: '作者 (dc:creator)', value: b.authors.join(' / ') },
      { id: 'series', label: '系列 / 丛书 (Series)', value: seriesVal },
      { id: 'isbn', label: 'ISBN', value: b.isbn ?? '', half: true },
      { id: 'publisher', label: '出版社 (dc:publisher)', value: b.publisher ?? '', half: true },
      { id: 'published_at', label: '出版日期 (dc:date)', value: b.published_at ?? '', half: true },
      { id: 'language', label: '语言 (dc:language)', value: b.language ?? '', half: true },
      { id: 'tags', label: '标签 / 主题 (dc:subject)', value: b.tags.join(' / ') },
      {
        id: 'description',
        label: '内容简介 (dc:description)',
        value: b.description ?? '',
        multiline: true,
      },
    ]
  }
  if (m.kind === 'comic') {
    const c = m.data
    const seriesVal = [
      c.series,
      c.volume !== null ? `Vol.${c.volume}` : '',
      c.number ? `#${c.number}` : '',
    ]
      .filter(Boolean)
      .join(' ')
    return [
      { id: 'title', label: '标题', value: c.title ?? '' },
      { id: 'series', label: '系列 / 丛书', value: seriesVal },
      { id: 'writers', label: '作者 / 编剧', value: c.writers.join(' / '), half: true },
      { id: 'pencillers', label: '画师 / 作画', value: c.pencillers.join(' / '), half: true },
      { id: 'publisher', label: '出版社', value: c.publisher ?? '', half: true },
      { id: 'year', label: '年份', value: c.year ? String(c.year) : '', half: true },
      { id: 'language', label: '语言', value: c.language_iso ?? '', half: true },
      { id: 'genres', label: '流派', value: c.genres.join(' / '), half: true },
      { id: 'tags', label: '标签', value: c.tags.join(' / ') },
      { id: 'summary', label: '简介', value: c.summary ?? '', multiline: true },
    ]
  }
  if (m.kind === 'music') {
    const mu = m.data
    return [
      { id: 'title', label: '曲目标题', value: mu.title },
      { id: 'artist', label: '歌手 / 艺人', value: mu.artist, half: true },
      { id: 'album_artist', label: '专辑艺术家', value: mu.album_artist ?? '', half: true },
      { id: 'album', label: '专辑名称', value: mu.album },
      {
        id: 'disc',
        label: '碟号',
        value: mu.disc_number ? String(mu.disc_number) : '',
        half: true,
      },
      {
        id: 'track',
        label: '音轨号',
        value: mu.track_number ? String(mu.track_number) : '',
        half: true,
      },
      { id: 'year', label: '年份', value: mu.year ? String(mu.year) : '', half: true },
      { id: 'genre', label: '流派', value: mu.genre ?? '', half: true },
      {
        id: 'lyrics',
        label: mu.sidecar_lrc ? `歌词（含 ${mu.sidecar_lrc_filename}）` : '内嵌歌词',
        value: mu.embedded_lyrics || mu.sidecar_lrc || '',
        multiline: true,
      },
    ]
  }
  const v = m.data
  return [
    { id: 'title', label: '标题', value: v.title },
    { id: 'original_title', label: '原作名', value: v.original_title ?? '' },
    { id: 'show_title', label: '所属剧集 / 系列', value: v.show_title ?? '' },
    {
      id: 'season',
      label: '季号',
      value: v.season_number !== null ? String(v.season_number) : '',
      half: true,
    },
    {
      id: 'episode',
      label: '集号',
      value: v.episode_number !== null ? String(v.episode_number) : '',
      half: true,
    },
    { id: 'year', label: '首播 / 年份', value: v.year ? String(v.year) : '', half: true },
    { id: 'rating', label: '评分', value: v.rating ? String(v.rating) : '', half: true },
    { id: 'studio', label: '制片 / 发行商', value: v.studio ?? '' },
    { id: 'directors', label: '导演', value: v.directors.join(' / ') },
    { id: 'actors', label: '主演', value: v.actors.join(' / ') },
    { id: 'genres', label: '流派', value: v.genres.join(' / ') },
    { id: 'tags', label: '标签', value: v.tags.join(' / ') },
    { id: 'summary', label: '剧情简介', value: v.summary ?? '', multiline: true },
  ]
})

function enterLocalEditMode(): void {
  if (!props.detail || !canSaveMetadata.value) return
  localForm.hydrateForm(props.detail)
  localEditMode.value = true
}

async function exitLocalEditMode(): Promise<void> {
  if (localForm.isDirty.value) {
    const ok = await dialogStore.confirm({
      title: '放弃左栏未保存的修改？',
      message: '退出编辑模式将丢弃当前左栏未保存的更改。',
      confirmText: '放弃修改',
      cancelText: '继续编辑',
      danger: true,
    })
    if (!ok) return
  }
  if (props.detail) {
    localForm.hydrateForm(props.detail)
  }
  localEditMode.value = false
}

function triggerLocalArtworkUpload(): void {
  if (!props.detail) return
  pendingUploadSlot.value =
    props.detail.metadata.kind === 'video' ? videoArtworkSlot.value : 'cover'
  coverInputRef.value?.click()
}

async function handleLocalArtworkPicked(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file || !props.detail) return
  uploadingArtwork.value = true
  errorMsg.value = null
  try {
    const updated = await toolApi.uploadArtwork(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
      file,
      pendingUploadSlot.value,
      props.detail.entry.media_kind,
    )
    lastEntryKey = entryCompositeKey(updated.entry)
    emit('updated', updated, true)
    snackbar.show('封面已更新并写入磁盘')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    uploadingArtwork.value = false
  }
}

async function handlePolishLocalBook(): Promise<void> {
  if (polishingBook.value || !props.detail || props.detail.entry.media_kind !== 'book') return
  polishingBook.value = true
  errorMsg.value = null
  try {
    const report = await toolApi.polishBook(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
    )
    lastEntryKey = entryCompositeKey(report.detail.entry)
    localForm.hydrateForm(report.detail)
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

async function saveLocalEdit(organizeAfterSave = false, advanceNext = false): Promise<void> {
  if (!props.detail || !canSaveMetadata.value || savingLocal.value) return
  savingLocal.value = true
  errorMsg.value = null
  try {
    const envelope = localForm.buildPatchEnvelope(props.detail.entry.media_kind)
    let updated = await toolApi.writeMetadata(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
      envelope,
    )
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
    lastEntryKey = entryCompositeKey(updated.entry)
    localForm.hydrateForm(updated)
    localEditMode.value = false
    emit('updated', updated, false)
    snackbar.show({
      message: organizeAfterSave ? '已保存元数据并完成路径归档' : '已保存左栏元数据修改',
      actionLabel: '变更历史',
      onAction: () => emit('openHistory'),
    })
    if (advanceNext && props.hasNext) {
      emit('step', 1)
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    savingLocal.value = false
  }
}

async function confirmDiscardIfDirty(): Promise<boolean> {
  if (!isCurrentDirty.value) return true
  return dialogStore.confirm({
    title: '放弃未保存的修改？',
    message: '当前工作台存在尚未写入原文件的修改，切换后这些修改将丢失。',
    confirmText: '放弃并继续',
    cancelText: '留下编辑',
    danger: true,
  })
}

async function requestStep(delta: -1 | 1): Promise<void> {
  if (saving.value || savingLocal.value) return
  if (delta < 0 && !props.hasPrev) return
  if (delta > 0 && !props.hasNext) return
  if (!(await confirmDiscardIfDirty())) return
  queueDropdownOpen.value = false
  emit('step', delta)
}

async function requestPickEntry(entry: MediaFileEntry): Promise<void> {
  if (saving.value || savingLocal.value) return
  queueDropdownOpen.value = false
  if (props.detail && entryCompositeKey(props.detail.entry) === entryCompositeKey(entry)) return
  if (!(await confirmDiscardIfDirty())) return
  emit('selectEntry', entry)
}

async function requestExit(): Promise<void> {
  if (saving.value || savingLocal.value) return
  if (!(await confirmDiscardIfDirty())) return
  queueDropdownOpen.value = false
  emit('exit')
}

async function handleApplyAndSave(
  meta: ScrapedMetadata,
  mode: ApplyFillMode,
  advanceNext = false,
  opts?: CandidateApplyOptions,
): Promise<void> {
  if (!props.detail || !canSaveMetadata.value || saving.value) return
  saving.value = true
  errorMsg.value = null
  try {
    applyForm.hydrateForm(props.detail)
    applyForm.applyScrapedMetadata(props.detail.entry.media_kind, meta, mode)

    const envelope = applyForm.buildPatchEnvelope(props.detail.entry.media_kind)
    let updated = await toolApi.writeMetadata(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
      envelope,
    )

    let bumpedArt = false
    const coverToUpload = applyForm.pendingCoverUrl.value
    if (coverToUpload) {
      try {
        const slot = updated.entry.media_kind === 'video' ? 'poster' : 'cover'
        updated = await toolApi.uploadArtworkFromUrl(
          updated.entry.root_id,
          updated.entry.rel_path,
          coverToUpload,
          slot,
          updated.entry.media_kind,
        )
        bumpedArt = true
        if (updated.entry.media_kind === 'music' && opts?.writeAlbumCover) {
          try {
            updated = await toolApi.uploadArtworkFromUrl(
              updated.entry.root_id,
              updated.entry.rel_path,
              coverToUpload,
              'album_cover',
              'music',
            )
          } catch {
            // Non-fatal sidecar album cover failure
          }
        }
      } catch {
        // Keep persisted metadata even if remote cover download fails
      }
      applyForm.pendingCoverUrl.value = null
    }
    if (applyForm.pendingBackdropUrl.value && updated.entry.media_kind === 'video') {
      try {
        updated = await toolApi.uploadArtworkFromUrl(
          updated.entry.root_id,
          updated.entry.rel_path,
          applyForm.pendingBackdropUrl.value,
          'fanart',
          'video',
        )
        bumpedArt = true
      } catch {
        // Ignore backdrop fetch failure
      }
      applyForm.pendingBackdropUrl.value = null
    }

    const organizeAfterSave = Boolean(opts?.organizeAfterSave)
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

    lastEntryKey = entryCompositeKey(updated.entry)
    localForm.hydrateForm(updated)
    emit('updated', updated, bumpedArt)
    snackbar.show({
      message: organizeAfterSave ? '已保存元数据并完成路径归档' : '已写入原文件并更新左侧信息',
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

function handleKeydown(ev: KeyboardEvent): void {
  if (ev.key === 'Escape' && queueDropdownOpen.value) {
    ev.preventDefault()
    queueDropdownOpen.value = false
    return
  }
  if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === 's') {
    ev.preventDefault()
    if (localEditMode.value && localForm.isDirty.value) {
      void saveLocalEdit(false, false)
    } else if (previewPaneRef.value) {
      previewPaneRef.value.triggerSave(false)
    } else if (localEditMode.value) {
      void saveLocalEdit(false, false)
    }
    return
  }
  if ((ev.ctrlKey || ev.metaKey) && ev.key === 'Enter') {
    ev.preventDefault()
    if (localEditMode.value && localForm.isDirty.value) {
      void saveLocalEdit(false, true)
    } else if (previewPaneRef.value) {
      previewPaneRef.value.triggerSave(true)
    } else if (localEditMode.value) {
      void saveLocalEdit(false, true)
    }
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

onMounted(() => {
  window.addEventListener('keydown', handleKeydown)
  window.addEventListener('resize', handleResize)
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
  window.removeEventListener('resize', handleResize)
})

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
      @change="handleLocalArtworkPicked"
    />

    <!-- Top Workbench Bar -->
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

      <!-- Center: Stepper + Current Entry Indicator (Dropdown ONLY when mobile or sidebar collapsed) -->
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
          v-if="showCompactQueueDropdown"
          type="button"
          class="md-state flex h-8 max-w-[min(420px,55vw)] min-w-0 items-center gap-2 rounded-full border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] px-3 text-xs font-semibold text-[var(--md-sys-color-on-surface)] transition hover:border-[var(--md-sys-color-primary)]"
          title="点击展开队列并快速跳转"
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

        <div
          v-else
          class="flex h-8 max-w-[min(440px,48vw)] min-w-0 items-center gap-2 rounded-full bg-[var(--md-sys-color-surface-container)] px-3 text-xs font-semibold text-[var(--md-sys-color-on-surface)]"
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
        </div>

        <button
          type="button"
          :disabled="!hasNext"
          class="md-icon-btn md-icon-btn-sm md-state !h-8 !w-8 shrink-0 disabled:opacity-35"
          title="下一项 (Alt+↓)"
          @click="requestStep(1)"
        >
          <LuArrowDown class="h-4 w-4" />
        </button>

        <!-- Compact Dropdown Queue Popover (mobile or collapsed sidebar only) -->
        <template v-if="showCompactQueueDropdown && queueDropdownOpen">
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

              <MdSegmentedButton
                v-model="dropdownFilter"
                :options="dropdownFilterOptions"
                size="sm"
              />
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
                :key="entryCompositeKey(entry)"
                type="button"
                class="md-state flex w-full items-center gap-2.5 rounded-xl px-2.5 py-2 text-left transition"
                :class="
                  detail && entryCompositeKey(detail.entry) === entryCompositeKey(entry)
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
                        resolveArtworkVersion(entry.root_id, entry.rel_path),
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
                  v-if="savedKeys.has(entryCompositeKey(entry))"
                  class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-success)]"
                />
                <span
                  v-else-if="!entry.completeness.has_cover"
                  class="h-2 w-2 shrink-0 rounded-full bg-[var(--md-sys-color-warning)]"
                />
              </button>
            </div>
          </div>
        </template>
      </div>

      <!-- Right: Session Saved Counter & Auto Search Toggle -->
      <div class="flex shrink-0 items-center gap-2.5">
        <span
          class="hidden rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-1 font-mono text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)] sm:inline-flex"
        >
          已存 {{ savedSessionCount }} / {{ entries.length }}
        </span>

        <div class="hidden lg:inline-flex">
          <AppCheckbox v-model="autoSearchOnSwitch" label="切项自动搜" />
        </div>
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

    <!-- MAIN 3-COLUMN WORKBENCH: [Left: Current Item (Read/Inline Edit)] | [Middle: Scraper Sources] | [Right: Editable Result & Save] -->
    <div
      v-else
      class="grid min-h-0 flex-1 grid-cols-1 overflow-hidden lg:grid-cols-[minmax(0,1fr)_330px_minmax(0,1fr)] xl:grid-cols-[minmax(0,1.05fr)_370px_minmax(0,1.05fr)]"
    >
      <!-- COLUMN 1 (LEFT): Current Persisted Entry with Direct Inline Edit Toggle -->
      <div class="flex min-h-0 flex-col overflow-hidden bg-[var(--md-sys-color-surface-container-low)]">
        <div
          class="flex shrink-0 items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)]/45 px-4 py-2"
        >
          <div class="flex min-w-0 items-center gap-1.5">
            <span
              class="rounded-full px-2 py-0.5 text-[10px] font-semibold"
              :class="
                localEditMode
                  ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                  : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface)]'
              "
            >
              {{ localEditMode ? '直接编辑中' : '当前条目信息' }}
            </span>
            <span class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ formatBytes(detail.entry.file_size) }}
            </span>
            <span
              v-if="savedKeys.has(entryCompositeKey(detail.entry))"
              class="shrink-0 rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-success-container)]"
            >
              已更新
            </span>
          </div>

          <div class="flex shrink-0 items-center gap-1.5">
            <button
              v-if="canSaveMetadata && !localEditMode"
              type="button"
              class="md-btn md-btn-sm md-btn-tonal md-state !h-7 !px-2.5 !text-[11px]"
              title="直接在左栏内联编辑当前条目字段"
              @click="enterLocalEditMode"
            >
              <LuPencil class="h-3 w-3" />
              <span>直接编辑</span>
            </button>
            <button
              v-else-if="localEditMode"
              type="button"
              class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2.5 !text-[11px]"
              @click="exitLocalEditMode"
            >
              <LuEye class="h-3 w-3" />
              <span>退出编辑</span>
            </button>
          </div>
        </div>

        <div class="flex-1 space-y-3.5 overflow-y-auto px-4 pt-3 pb-5">
          <!-- Local Persisted Cover & Summary Card -->
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
              </div>

              <MdSegmentedButton
                v-if="detail.metadata.kind === 'video' && videoSlotOptions.length > 0"
                v-model="videoArtworkSlot"
                :options="videoSlotOptions"
                size="sm"
              />
            </div>

            <div class="flex min-w-0 flex-1 flex-col justify-between gap-2">
              <div class="space-y-1">
                <div class="line-clamp-2 text-[13px] font-semibold text-[var(--md-sys-color-on-surface)]">
                  {{ detail.entry.display_title || detail.entry.file_name }}
                </div>
                <div
                  class="truncate font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
                  :title="detail.entry.rel_path"
                >
                  {{ detail.entry.rel_path }}
                </div>
              </div>

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

              <!-- Completeness Badges + Inline Artwork Upload -->
              <div class="flex flex-wrap items-center gap-1">
                <span
                  class="rounded-full px-2 py-0.5 text-[10px] font-medium"
                  :class="
                    detail.entry.completeness.has_cover
                      ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                      : 'bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]'
                  "
                >
                  {{ detail.entry.completeness.has_cover ? '有封面' : '缺封面' }}
                </span>
                <span
                  class="rounded-full px-2 py-0.5 text-[10px] font-medium"
                  :class="
                    detail.entry.completeness.has_summary_or_lyrics
                      ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                      : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
                  "
                >
                  {{
                    detail.entry.media_kind === 'music'
                      ? detail.entry.completeness.has_summary_or_lyrics
                        ? '有歌词'
                        : '缺歌词'
                      : detail.entry.completeness.has_summary_or_lyrics
                        ? '有简介'
                        : '缺简介'
                  }}
                </span>
                <span
                  class="rounded-full px-2 py-0.5 text-[10px] font-medium"
                  :class="
                    detail.entry.completeness.has_nfo_or_tag_block
                      ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                      : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
                  "
                >
                  {{ detail.entry.completeness.has_nfo_or_tag_block ? '有标签' : '缺标签' }}
                </span>

                <button
                  v-if="localEditMode"
                  type="button"
                  :disabled="uploadingArtwork"
                  class="md-chip md-state ml-auto !h-6 !rounded-full !px-2 !text-[10px]"
                  @click="triggerLocalArtworkUpload"
                >
                  <LuUpload class="h-2.5 w-2.5" />
                  <span>{{ uploadingArtwork ? '上传中…' : '上传图片' }}</span>
                </button>
              </div>
            </div>
          </div>

          <div
            v-if="errorMsg"
            class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-3.5 py-2.5 text-xs text-[var(--md-sys-color-on-error-container)]"
          >
            {{ errorMsg }}
          </div>

          <!-- Inline Domain Metadata Form when editing directly in left pane -->
          <template v-if="localEditMode">
            <InspectorMusicForm
              v-if="detail.metadata.kind === 'music'"
              :form="localForm.musicForm"
              :sidecar-lrc-filename="detail.metadata.data.sidecar_lrc_filename"
              :file-key="entryCompositeKey(detail.entry)"
            />
            <InspectorVideoForm
              v-else-if="detail.metadata.kind === 'video'"
              :form="localForm.videoForm"
            />
            <InspectorComicForm
              v-else-if="detail.metadata.kind === 'comic'"
              :form="localForm.comicForm"
            />
            <InspectorBookForm
              v-else-if="detail.metadata.kind === 'book'"
              :form="localForm.bookForm"
              :book-format="detail.metadata.data.format"
              :writable="detail.metadata.data.writable"
              :is-epub="detail.entry.extension === 'epub'"
              :has-cover="detail.metadata.data.has_cover"
              :polishing-book="polishingBook"
              @polish="handlePolishLocalBook"
            />
          </template>

          <!-- Read-Only Structured Local Metadata Grid (double-click any field to start inline editing) -->
          <div v-else class="grid grid-cols-2 gap-2.5">
            <div
              v-for="field in localReadOnlyFields"
              :key="field.id"
              :class="field.half ? 'col-span-1' : 'col-span-2'"
              class="space-y-1"
              :title="canSaveMetadata ? '双击直接进入内联编辑' : undefined"
              @dblclick="canSaveMetadata && enterLocalEditMode()"
            >
              <div class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
                {{ field.label }}
              </div>
              <div
                v-if="field.multiline"
                class="min-h-[84px] max-h-56 overflow-y-auto rounded-xl border border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface)]/80 px-2.5 py-2 text-xs leading-relaxed whitespace-pre-wrap select-text"
                :class="
                  field.value
                    ? 'text-[var(--md-sys-color-on-surface)]'
                    : 'text-[var(--md-sys-color-on-surface-variant)]/55'
                "
              >
                {{ field.value || '—' }}
              </div>
              <div
                v-else
                class="flex min-h-9 items-center rounded-xl border border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface)]/80 px-2.5 py-1.5 text-xs break-words select-text"
                :class="
                  field.value
                    ? 'text-[var(--md-sys-color-on-surface)]'
                    : 'text-[var(--md-sys-color-on-surface-variant)]/55'
                "
              >
                {{ field.value || '—' }}
              </div>
            </div>
          </div>
        </div>

        <!-- Left Pane Inline Edit Footer -->
        <div
          v-if="localEditMode"
          class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container)]/75 px-4 py-2.5"
        >
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-text md-state"
            @click="exitLocalEditMode"
          >
            取消
          </button>
          <div class="flex items-center gap-1.5">
            <button
              type="button"
              :disabled="savingLocal"
              class="md-btn md-btn-sm md-btn-outlined md-state"
              title="保存修改并按默认模板重命名归档"
              @click="saveLocalEdit(true, false)"
            >
              <LuFileSignature class="h-3.5 w-3.5" />
              <span>保存并归档</span>
            </button>
            <button
              type="button"
              :disabled="savingLocal"
              class="md-btn md-btn-sm md-btn-filled md-state"
              title="直接写入当前原文件 (⌘S)"
              @click="saveLocalEdit(false, false)"
            >
              <LuLoader2 v-if="savingLocal" class="h-3.5 w-3.5 animate-spin" />
              <LuSave v-else class="h-3.5 w-3.5" />
              <span>{{ savingLocal ? '保存中…' : '保存修改' }}</span>
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
            :settings="settings"
            :auto-search-trigger="autoSearchTrigger"
            :selected-preview-key="candidatePreview?.key ?? null"
            @direct-apply="(m, mode) => handleApplyAndSave(m, mode, false)"
            @select-preview="candidatePreview = $event"
            @open-settings="emit('openSettingsScrapers')"
            @settings-updated="emit('settingsUpdated', $event)"
          />
        </div>
      </div>

      <!-- COLUMN 3 (RIGHT): Editable Pending Result & Save Area -->
      <div class="flex min-h-0 flex-col overflow-hidden bg-[var(--md-sys-color-surface-container-low)]">
        <CandidateDetailPreviewPane
          v-if="candidatePreview"
          ref="previewPaneRef"
          :media-kind="detail.entry.media_kind"
          :preview="candidatePreview"
          :has-next="hasNext"
          :closable="false"
          :saving="saving"
          @direct-apply="(m, mode, opts) => handleApplyAndSave(m, mode, false, opts)"
          @direct-apply-next="(m, mode, opts) => handleApplyAndSave(m, mode, true, opts)"
        />
        <div
          v-else
          class="flex flex-1 flex-col items-center justify-center gap-3 px-8 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          <div
            class="flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-surface-container-high)]"
          >
            <LuImage class="h-5 w-5 opacity-55" />
          </div>
          <p class="font-semibold text-[var(--md-sys-color-on-surface)]">
            等待选择刮削源结果
          </p>
          <p class="max-w-[260px] text-[11px] leading-relaxed opacity-80">
            点击中间候选结果即可对照编辑并写入，或直接在左栏点击「直接编辑」修改本地字段
          </p>
          <button
            v-if="canSaveMetadata && !localEditMode"
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="enterLocalEditMode"
          >
            <LuPencil class="h-3.5 w-3.5" />
            <span>在左栏直接编辑</span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
