<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  LuArrowDown,
  LuArrowUp,
  LuBookOpen,
  LuClapperboard,
  LuColumns3,
  LuDisc3,
  LuFileSignature,
  LuFileText,
  LuFilm,
  LuImage,
  LuLayers,
  LuLock,
  LuMusic,
  LuRefreshCw,
  LuSave,
  LuSparkles,
  LuUnlock,
  LuUpload,
  LuWand2,
  LuX,
} from 'vue-icons-plus/lu'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { buildArtworkUrl, resolveScraperCoverUrl, toolApi } from '../api'
import { formatBytes, formatDuration, useToolText } from '../i18n'
import type { MediaFileDetail, MediaFileEntry, ScrapedMetadata } from '../types'
import InspectorBookForm from './inspector/InspectorBookForm.vue'
import InspectorComicForm from './inspector/InspectorComicForm.vue'
import InspectorMusicForm from './inspector/InspectorMusicForm.vue'
import InspectorVideoForm from './inspector/InspectorVideoForm.vue'
import {
  useMetadataForm,
  type ApplyFillMode,
  type ScrapedFieldKey,
} from './inspector/useMetadataForm'
import CandidateDetailPreviewPane, {
  type CandidatePreviewState,
} from './scrape/CandidateDetailPreviewPane.vue'
import WorkbenchCandidatePane from './scrape/WorkbenchCandidatePane.vue'

const props = withDefaults(
  defineProps<{
    detail: MediaFileDetail
    artworkVersion: number
    showCandidates?: boolean
    hasPrev?: boolean
    hasNext?: boolean
    positionLabel?: string
  }>(),
  {
    showCandidates: false,
    hasPrev: false,
    hasNext: false,
    positionLabel: '',
  },
)

const emit = defineEmits<{
  close: []
  updated: [detail: MediaFileDetail, bumpArtwork: boolean]
  step: [delta: -1 | 1]
  'update:showCandidates': [open: boolean]
  openScrape: [detail: MediaFileDetail]
  openSettingsScrapers: []
  openContentEditor: [entry: MediaFileEntry]
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

function handlePreviewFillField(field: ScrapedFieldKey, meta: ScrapedMetadata, label: string): void {
  const changed = applyScrapedField(props.detail.entry.media_kind, field, meta, 'overwrite')
  if (changed) {
    snackbar.show(`已将「${label}」填入左侧表单，按 ⌘S 保存落盘`)
  }
}

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
    if (props.showCandidates && autoSearchOnSwitch.value) {
      autoSearchTrigger.value += 1
    }
  },
  { immediate: true },
)

watch(
  () => props.showCandidates,
  (open) => {
    if (!open) {
      candidatePreview.value = null
      return
    }
    if (props.detail) {
      autoSearchTrigger.value += 1
    }
  },
)

const isCurrentDirty = isDirty

const canSaveMetadata = computed(() => {
  if (props.detail.metadata.kind === 'book') {
    return props.detail.metadata.data.writable
  }
  return true
})

const artworkSrc = computed(() => {
  if (pendingCoverUrl.value) {
    return resolveScraperCoverUrl(pendingCoverUrl.value)
  }
  const m = props.detail.metadata
  let has = false
  let slot: 'cover' | 'poster' | 'fanart' | 'thumb' = 'cover'
  if (m.kind === 'music') has = m.data.has_embedded_cover
  else if (m.kind === 'book') has = m.data.has_cover
  else if (m.kind === 'comic') has = m.data.has_cover
  else if (m.kind === 'video') {
    slot = videoArtworkSlot.value
    if (slot === 'poster') has = m.data.has_poster
    else if (slot === 'fanart') has = m.data.has_fanart
    else has = m.data.has_thumb
  }
  if (!has) return null
  return buildArtworkUrl(
    props.detail.entry.root_id,
    props.detail.entry.rel_path,
    props.detail.entry.media_kind,
    props.artworkVersion,
    slot,
  )
})

async function confirmDiscardIfDirty(): Promise<boolean> {
  if (!isCurrentDirty.value) return true
  return dialogStore.confirm({
    title: '放弃未保存的更改？',
    message: `当前条目「${props.detail.entry.file_name}」有尚未保存的修改，继续操作将会丢失这些改动。`,
    confirmText: '放弃更改',
    cancelText: '继续编辑',
    danger: true,
  })
}

async function requestClose(): Promise<void> {
  if (!(await confirmDiscardIfDirty())) return
  emit('close')
}

async function requestStep(delta: -1 | 1): Promise<void> {
  if (delta < 0 && !props.hasPrev) return
  if (delta > 0 && !props.hasNext) return
  if (!(await confirmDiscardIfDirty())) return
  emit('step', delta)
}

function triggerArtworkUpload(
  slot: 'cover' | 'series_cover' | 'poster' | 'fanart' | 'thumb',
): void {
  pendingSlot.value = slot
  coverInputRef.value?.click()
}

async function handleArtworkPicked(ev: Event): Promise<void> {
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
    pendingCoverUrl.value = null
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
  if (polishingBook.value || props.detail.entry.media_kind !== 'book') return
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
  if (!canSaveMetadata.value || saving.value) return
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
  const count = applyScrapedMetadata(props.detail.entry.media_kind, meta, mode)
  snackbar.show(`已将 ${count} 个在线字段填入编辑表单（尚未保存）`)
}

function handleFillField(field: ScrapedFieldKey, meta: ScrapedMetadata): void {
  const ok = applyScrapedField(props.detail.entry.media_kind, field, meta, 'overwrite')
  if (ok) {
    snackbar.show('已回填字段到表单，按 ⌘S 保存落盘')
  }
}

async function handleDirectApply(meta: ScrapedMetadata, mode: ApplyFillMode): Promise<void> {
  applyScrapedMetadata(props.detail.entry.media_kind, meta, mode)
  await saveMetadata(false, false)
}

function handleKeydown(ev: KeyboardEvent): void {
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
  } else if (ev.altKey && ev.key === 'ArrowDown') {
    ev.preventDefault()
    void requestStep(1)
  }
}

onMounted(() => window.addEventListener('keydown', handleKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', handleKeydown))

defineExpose({
  isDirty: isCurrentDirty,
  confirmDiscardIfDirty,
  save: () => saveMetadata(false, false),
  requestClose,
})
</script>

<template>
  <aside
    class="flex h-full w-full flex-col overflow-hidden bg-[var(--md-sys-color-surface-container-low)] transition-[width] duration-300 ease-[var(--md-sys-motion-easing-standard)] lg:rounded-l-[24px]"
    :class="
      showCandidates && candidatePreview
        ? 'max-w-[94vw] lg:w-[960px] xl:w-[1140px] 2xl:w-[1260px]'
        : showCandidates
          ? 'lg:w-[760px] xl:w-[860px]'
          : 'lg:w-[440px] xl:w-[470px]'
    "
  >
    <input
      ref="coverInputRef"
      type="file"
      accept="image/jpeg,image/png,image/webp"
      class="hidden"
      @change="handleArtworkPicked"
    />

    <!-- Supporting Pane Top App Bar -->
    <div
      class="flex shrink-0 items-center justify-between gap-2 px-4 pt-3.5 pb-2.5"
    >
      <div class="flex min-w-0 items-center gap-1.5">
        <div
          v-if="hasPrev || hasNext"
          class="flex shrink-0 items-center rounded-full bg-[var(--md-sys-color-surface-container-high)] p-0.5"
        >
          <button
            type="button"
            :disabled="!hasPrev"
            class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7 disabled:opacity-35"
            title="上一项 (Alt+↑)"
            @click="requestStep(-1)"
          >
            <LuArrowUp class="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            :disabled="!hasNext"
            class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7 disabled:opacity-35"
            title="下一项 (Alt+↓)"
            @click="requestStep(1)"
          >
            <LuArrowDown class="h-3.5 w-3.5" />
          </button>
        </div>

        <div class="min-w-0">
          <div class="flex items-center gap-1.5">
            <span class="truncate text-[13px] font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ detail.entry.file_name }}
            </span>
            <span
              v-if="isCurrentDirty"
              class="shrink-0 rounded-full bg-[var(--md-sys-color-warning-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-warning-container)]"
            >
              未保存
            </span>
          </div>
          <div class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            <span v-if="positionLabel" class="mr-1.5 font-mono">{{ positionLabel }} ·</span>
            {{ formatBytes(detail.entry.file_size) }}
          </div>
        </div>
      </div>

      <div class="flex shrink-0 items-center gap-1">
        <!-- Seamless Toggle for Inline Online Candidate Comparison -->
        <button
          type="button"
          class="md-btn md-btn-sm md-state"
          :class="
            showCandidates
              ? 'md-btn-filled'
              : 'md-btn-tonal'
          "
          title="在同屏右侧展开在线候选并单字段回填"
          @click="emit('update:showCandidates', !showCandidates)"
        >
          <LuColumns3 class="h-3.5 w-3.5" />
          <span>{{ showCandidates ? '收起候选' : '在线对照' }}</span>
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="多源高级刮削向导"
          @click="emit('openScrape', detail)"
        >
          <LuSparkles class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="关闭详情面板"
          @click="requestClose"
        >
          <LuX class="h-4 w-4" />
        </button>
      </div>
    </div>

    <!-- Main Body: Single Edit Pane OR Dual/Tri Side-by-Side (Edit + Online Candidates + Selected Preview) -->
    <div
      class="grid min-h-0 flex-1"
      :class="
        showCandidates && candidatePreview
          ? 'grid-cols-1 lg:grid-cols-[minmax(300px,1fr)_320px_minmax(280px,340px)] xl:grid-cols-[minmax(340px,1fr)_350px_minmax(320px,390px)]'
          : showCandidates
            ? 'grid-cols-1 lg:grid-cols-[minmax(0,1fr)_350px] xl:grid-cols-[minmax(0,1fr)_390px]'
            : 'grid-cols-1'
      "
    >
      <!-- Left Column: Metadata Form + Artwork -->
      <div class="flex min-h-0 flex-col overflow-hidden">
        <div class="flex-1 space-y-4 overflow-y-auto px-4 pt-1 pb-4">
          <!-- Artwork & Media Specs Card (Tonal Surface, No Harsh Border) -->
          <div
            class="flex gap-3.5 rounded-2xl bg-[var(--md-sys-color-surface)] p-3.5"
          >
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

              <!-- Video Multi-Slot Artwork Preview Tabs -->
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
                      : slotTab.ok
                        ? 'text-[var(--md-sys-color-on-surface)]'
                        : 'text-[var(--md-sys-color-on-surface-variant)]/60'
                  "
                  @click="videoArtworkSlot = slotTab.id"
                >
                  {{ slotTab.label }}
                </button>
              </div>
            </div>

            <div class="flex min-w-0 flex-1 flex-col justify-between gap-2">
              <div>
                <div class="flex flex-wrap items-center gap-1.5">
                  <span
                    class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 font-mono text-[10px] font-bold uppercase text-[var(--md-sys-color-on-primary-container)]"
                  >
                    {{ detail.entry.extension }}
                  </span>

                  <template v-if="detail.metadata.kind === 'music'">
                    <span class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                      {{ formatDuration(detail.metadata.data.duration_secs) }}
                    </span>
                    <span
                      v-if="detail.metadata.data.bit_rate_kbps"
                      class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
                    >
                      {{ detail.metadata.data.bit_rate_kbps }} kbps
                    </span>
                  </template>

                  <template v-else-if="detail.metadata.kind === 'comic'">
                    <span class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                      {{ detail.metadata.data.page_count }} 页
                    </span>
                    <span class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                      {{ detail.metadata.data.has_comic_info_xml ? '含 ComicInfo.xml' : '无 ComicInfo.xml' }}
                    </span>
                  </template>

                  <template v-else-if="detail.metadata.kind === 'video'">
                    <span
                      v-if="detail.metadata.data.width && detail.metadata.data.height"
                      class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
                    >
                      {{ detail.metadata.data.width }}×{{ detail.metadata.data.height }}
                    </span>
                    <span
                      v-if="detail.metadata.data.video_codec"
                      class="rounded-full bg-[var(--md-sys-color-surface-container)] px-2 py-0.5 font-mono text-[10px] uppercase text-[var(--md-sys-color-on-surface-variant)]"
                    >
                      {{ detail.metadata.data.video_codec }}
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

                <!-- Video Stream Summary -->
                <div
                  v-if="detail.metadata.kind === 'video'"
                  class="mt-2 space-y-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
                >
                  <div v-if="detail.metadata.data.audio_streams.length > 0" class="truncate">
                    音轨: {{
                      detail.metadata.data.audio_streams
                        .map((a) => `${a.codec.toUpperCase()}${a.language ? ` [${a.language}]` : ''}`)
                        .join(' · ')
                    }}
                  </div>
                  <div
                    v-if="
                      detail.metadata.data.subtitle_streams.length > 0 ||
                      detail.metadata.data.sidecar_subtitles.length > 0
                    "
                    class="truncate"
                  >
                    字幕: 内封 {{ detail.metadata.data.subtitle_streams.length }} · 外挂
                    {{ detail.metadata.data.sidecar_subtitles.length }}
                  </div>
                </div>
              </div>

              <!-- Quick Assist Chips -->
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
                    v-if="detail.entry.media_kind === 'comic' && detail.entry.rel_path.includes('/')"
                    type="button"
                    :disabled="uploadingArtwork"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="triggerArtworkUpload('series_cover')"
                  >
                    <LuImage class="h-3 w-3" />
                    <span>系列封面</span>
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
                    v-if="detail.entry.media_kind === 'book' && detail.entry.extension === 'epub'"
                    type="button"
                    :disabled="polishingBook"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="handlePolishBook"
                  >
                    <LuRefreshCw v-if="polishingBook" class="h-3 w-3 animate-spin" />
                    <LuWand2 v-else class="h-3 w-3" />
                    <span>抛光</span>
                  </button>

                  <button
                    v-if="detail.entry.media_kind === 'comic'"
                    type="button"
                    class="md-chip md-chip-selected md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="emit('openComicPages', detail.entry)"
                  >
                    <LuImage class="h-3 w-3" />
                    <span>页面管理</span>
                  </button>

                  <button
                    v-if="detail.entry.media_kind === 'music'"
                    type="button"
                    class="md-chip md-chip-selected md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="emit('openMusicTools', detail.entry)"
                  >
                    <LuMusic class="h-3 w-3" />
                    <span>音频工具箱</span>
                  </button>
                </template>

                <template v-else>
                  <button
                    type="button"
                    class="md-chip md-chip-selected md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="emit('openVideoTools', detail.entry)"
                  >
                    <LuFilm class="h-3 w-3" />
                    <span>字幕与剧集工具</span>
                  </button>
                  <button
                    type="button"
                    :disabled="uploadingArtwork"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="triggerArtworkUpload('poster')"
                  >
                    <LuImage class="h-3 w-3" />
                    <span>海报</span>
                  </button>
                  <button
                    type="button"
                    :disabled="uploadingArtwork"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    @click="triggerArtworkUpload('fanart')"
                  >
                    <LuUpload class="h-3 w-3" />
                    <span>背景</span>
                  </button>
                  <button
                    type="button"
                    class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                    :class="isVideoFieldLocked('poster') ? 'md-chip-selected' : ''"
                    @click="toggleVideoFieldLock('poster')"
                  >
                    <LuLock v-if="isVideoFieldLocked('poster')" class="h-3 w-3" />
                    <LuUnlock v-else class="h-3 w-3" />
                    <span>{{ isVideoFieldLocked('poster') ? '已锁' : '锁海报' }}</span>
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

          <!-- Domain Metadata Forms -->
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

        <!-- Bottom Action Bar (Strict MD3 Button Hierarchy) -->
        <div
          class="flex shrink-0 flex-wrap items-center justify-between gap-2 bg-[var(--md-sys-color-surface-container)]/75 px-4 py-3"
        >
          <span class="hidden text-[11px] text-[var(--md-sys-color-on-surface-variant)] sm:inline">
            ⌘S 保存 · ⌘↵ 下一项
          </span>

          <div class="ml-auto flex items-center gap-2">
            <button
              type="button"
              :disabled="!canSaveMetadata || saving"
              class="md-btn md-btn-sm md-btn-text md-state"
              title="保存元数据并按默认模板重命名归档"
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
              <span>{{ saving ? txt.actions.saving : txt.actions.save }}</span>
            </button>
          </div>
        </div>
      </div>

      <!-- Optional Expanded Middle Column: Live Online Candidates & Single-Field Fill -->
      <div
        v-if="showCandidates"
        class="hidden min-h-0 flex-col border-l border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container)]/40 lg:flex"
      >
        <div class="flex shrink-0 items-center justify-between px-3.5 pt-2.5 pb-1">
          <span class="text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]">
            在线候选对照 · 点击选中右侧预览
          </span>
          <label class="inline-flex cursor-pointer items-center gap-1.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            <input
              v-model="autoSearchOnSwitch"
              type="checkbox"
              class="h-3.5 w-3.5 accent-[var(--md-sys-color-primary)]"
            />
            <span>切项自动搜</span>
          </label>
        </div>
        <div class="min-h-0 flex-1 overflow-hidden">
          <WorkbenchCandidatePane
            :detail="detail"
            :auto-search-trigger="autoSearchTrigger"
            :selected-preview-key="candidatePreview?.key ?? null"
            @fill-all="handleFillAll"
            @fill-field="handleFillField"
            @direct-apply="handleDirectApply"
            @select-preview="candidatePreview = $event"
            @open-settings="emit('openSettingsScrapers')"
          />
        </div>
      </div>

      <!-- Optional Expanded Right Column: Mirror Structured Preview of Selected Candidate -->
      <div
        v-if="showCandidates && candidatePreview"
        class="hidden min-h-0 flex-col border-l border-[var(--md-sys-color-outline-variant)]/45 lg:flex"
      >
        <CandidateDetailPreviewPane
          :media-kind="detail.entry.media_kind"
          :preview="candidatePreview"
          @fill-all="handleFillAll"
          @fill-field="handlePreviewFillField"
          @direct-apply="handleDirectApply"
          @close="candidatePreview = null"
        />
      </div>
    </div>
  </aside>
</template>
