<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { LuImage, LuPencil } from 'vue-icons-plus/lu'
import MdTabs, { type MdTabItem } from '@/components/ui/MdTabs.vue'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { buildArtworkUrl, toolApi } from '../../api'
import { entryCompositeKey } from '../../completeness'
import type {
  MediaFileDetail,
  MediaFileEntry,
  PublicSettingsDto,
  ScrapedMetadata,
} from '../../types'
import { useMetadataForm, type ApplyFillMode } from '../inspector/useMetadataForm'
import CandidateDetailPreviewPane, {
  type CandidateApplyOptions,
  type CandidatePreviewState,
} from './CandidateDetailPreviewPane.vue'
import WorkbenchCandidatePane from './WorkbenchCandidatePane.vue'
import WorkbenchLocalPane from './WorkbenchLocalPane.vue'
import WorkbenchTopBar from './WorkbenchTopBar.vue'

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

const localForm = useMetadataForm()
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

const isMobileViewport = ref(typeof window !== 'undefined' ? window.innerWidth < 840 : false)
const showCompactQueueDropdown = computed(() => props.sidebarCollapsed || isMobileViewport.value)
const queueDropdownOpen = ref(false)

type WorkbenchMobilePane = 'local' | 'candidates' | 'preview'
const mobilePane = ref<WorkbenchMobilePane>('candidates')
const mobilePaneTabs = computed<MdTabItem<WorkbenchMobilePane>[]>(() => [
  { value: 'local', label: localEditMode.value ? '当前信息（编辑中）' : '当前信息' },
  { value: 'candidates', label: '候选结果' },
  { value: 'preview', label: candidatePreview.value ? '对比预览 · 已选' : '对比预览' },
])

function handleSelectCandidatePreview(next: CandidatePreviewState | null): void {
  candidatePreview.value = next
  if (next && typeof window !== 'undefined' && window.innerWidth < 1200) {
    mobilePane.value = 'preview'
  }
}

function handleResize(): void {
  isMobileViewport.value = window.innerWidth < 840
}

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

    <WorkbenchTopBar
      :detail="detail"
      :entries="entries"
      :saved-keys="savedKeys"
      :artwork-version="artworkVersion"
      :artwork-versions="artworkVersions"
      :sidebar-collapsed="sidebarCollapsed"
      :show-compact-queue-dropdown="showCompactQueueDropdown"
      :queue-dropdown-open="queueDropdownOpen"
      :has-prev="hasPrev"
      :has-next="hasNext"
      :position-label="positionLabel"
      :is-current-dirty="isCurrentDirty"
      :auto-search-on-switch="autoSearchOnSwitch"
      @exit="requestExit"
      @toggle-sidebar="emit('toggleSidebar')"
      @step="requestStep"
      @select-entry="requestPickEntry"
      @update:queue-dropdown-open="queueDropdownOpen = $event"
      @update:auto-search-on-switch="autoSearchOnSwitch = $event"
    />

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

    <!-- MAIN 3-COLUMN WORKBENCH -->
    <div v-else class="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div class="shrink-0 bg-[var(--md-sys-color-surface-container-low)] lg:hidden">
        <MdTabs
          v-model="mobilePane"
          :tabs="mobilePaneTabs"
          variant="primary"
          aria-label="刮削工作台分区"
        />
      </div>

      <div
        class="grid min-h-0 flex-1 grid-cols-1 overflow-hidden lg:grid-cols-[minmax(0,1fr)_330px_minmax(0,1fr)] xl:grid-cols-[minmax(0,1.05fr)_370px_minmax(0,1.05fr)]"
      >
        <!-- COLUMN 1 (LEFT): Current Persisted Entry -->
        <div
          class="min-h-0 flex-col overflow-hidden bg-[var(--md-sys-color-surface-container-low)] lg:flex"
          :class="mobilePane === 'local' ? 'flex' : 'hidden'"
        >
          <WorkbenchLocalPane
            :detail="detail"
            :saved-keys="savedKeys"
            :local-edit-mode="localEditMode"
            :can-save-metadata="canSaveMetadata"
            :artwork-src="artworkSrc"
            :video-artwork-slot="videoArtworkSlot"
            :uploading-artwork="uploadingArtwork"
            :polishing-book="polishingBook"
            :saving-local="savingLocal"
            :error-msg="errorMsg"
            :local-form="localForm"
            @update:video-artwork-slot="videoArtworkSlot = $event"
            @enter-edit="enterLocalEditMode"
            @exit-edit="exitLocalEditMode"
            @upload-artwork="triggerLocalArtworkUpload"
            @polish-book="handlePolishLocalBook"
            @save-local="saveLocalEdit($event, false)"
          />
        </div>

        <!-- COLUMN 2 (MIDDLE): Scraper Sources & Candidate List -->
        <div
          class="min-h-0 flex-col border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container)]/40 lg:flex lg:border-x"
          :class="mobilePane === 'candidates' ? 'flex' : 'hidden'"
        >
          <div class="min-h-0 flex-1 overflow-hidden">
            <WorkbenchCandidatePane
              :detail="detail"
              :settings="settings"
              :auto-search-trigger="autoSearchTrigger"
              :selected-preview-key="candidatePreview?.key ?? null"
              @direct-apply="(m, mode) => handleApplyAndSave(m, mode, false)"
              @select-preview="handleSelectCandidatePreview"
              @open-settings="emit('openSettingsScrapers')"
              @settings-updated="emit('settingsUpdated', $event)"
            />
          </div>
        </div>

        <!-- COLUMN 3 (RIGHT): Editable Pending Result & Save Area -->
        <div
          class="min-h-0 flex-col overflow-hidden bg-[var(--md-sys-color-surface-container-low)] lg:flex"
          :class="mobilePane === 'preview' ? 'flex' : 'hidden'"
        >
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
              @click="enterLocalEditMode(); mobilePane = 'local'"
            >
              <LuPencil class="h-3.5 w-3.5" />
              <span>在左栏直接编辑</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
