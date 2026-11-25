<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuImage, LuPencil } from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdTabs, { type MdTabItem } from '@/components/ui/MdTabs.vue'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { buildArtworkUrl, toolApi } from '../../api'
import { entryCompositeKey } from '../../completeness'
import { saveAndOrganizeEntry } from '../../services/saveEntry'
import { useToolInspectorStore } from '../../stores/inspector'
import { useToolScrapeStore } from '../../stores/scrape'
import { useToolWorkspaceStore } from '../../stores/workspace'
import type { MediaFileEntry, ScrapedMetadata } from '../../types'
import { useMetadataForm, type ApplyFillMode } from '../inspector/useMetadataForm'
import CandidateDetailPreviewPane, {
  type CandidateApplyOptions,
  type CandidatePreviewState,
} from './CandidateDetailPreviewPane.vue'
import WorkbenchCandidatePane from './WorkbenchCandidatePane.vue'
import WorkbenchLocalPane from './WorkbenchLocalPane.vue'
import WorkbenchTopBar from './WorkbenchTopBar.vue'

const { t } = useI18n()
const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()
const workspaceStore = useToolWorkspaceStore()
const inspectorStore = useToolInspectorStore()
const scrapeStore = useToolScrapeStore()

const detail = computed(() => inspectorStore.activeDetail)
const loadingDetail = computed(() => inspectorStore.loadingInspector)
const entries = computed(() => scrapeStore.workbenchQueueEntries)
const savedKeys = computed(() => scrapeStore.scrapeWorkbenchSavedPaths)
const artworkVersion = computed(() => inspectorStore.artworkVersion)
const artworkVersions = computed(() => inspectorStore.fileArtworkVersions)
const sidebarCollapsed = computed(() => workspaceStore.collapsed)
const hasPrev = computed(() => scrapeStore.hasPrevInspect)
const hasNext = computed(() => scrapeStore.hasNextInspect)
const positionLabel = computed(() => scrapeStore.inspectPositionLabel)
const settings = computed(() => workspaceStore.settings)

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
let saveSeq = 0

const isMobileViewport = ref(typeof window !== 'undefined' ? window.innerWidth < 600 : false)
const showCompactQueueDropdown = computed(() => sidebarCollapsed.value || isMobileViewport.value)
const queueDropdownOpen = ref(false)

type WorkbenchMobilePane = 'local' | 'candidates' | 'preview'
type WorkbenchDetailPane = 'local' | 'preview'

const mobilePane = ref<WorkbenchMobilePane>('candidates')
const detailPaneTab = ref<WorkbenchDetailPane>('local')

const mobilePaneTabs = computed<MdTabItem<WorkbenchMobilePane>[]>(() => [
  {
    value: 'local',
    label: localEditMode.value
      ? t('tool.workbench.view.tabLocalEditing')
      : t('tool.workbench.view.tabLocal'),
  },
  { value: 'candidates', label: t('tool.workbench.view.tabCandidates') },
  {
    value: 'preview',
    label: candidatePreview.value
      ? t('tool.workbench.view.tabPreviewSelected')
      : t('tool.workbench.view.tabPreview'),
  },
])

const detailPaneTabs = computed<MdTabItem<WorkbenchDetailPane>[]>(() => [
  {
    value: 'local',
    label: localEditMode.value
      ? t('tool.workbench.view.tabLocalEditing')
      : t('tool.workbench.view.tabLocal'),
  },
  {
    value: 'preview',
    label: candidatePreview.value
      ? t('tool.workbench.view.tabPreviewSelected')
      : t('tool.workbench.view.tabPreview'),
  },
])

function handleSelectCandidatePreview(next: CandidatePreviewState | null): void {
  candidatePreview.value = next
  if (next) {
    detailPaneTab.value = 'preview'
    if (typeof window !== 'undefined' && window.innerWidth < 840) {
      mobilePane.value = 'preview'
    }
  }
}

function handleResize(): void {
  isMobileViewport.value = window.innerWidth < 600
}

watch(
  () => (detail.value ? entryCompositeKey(detail.value.entry) : ''),
  (key) => {
    if (!key) {
      lastEntryKey = ''
      candidatePreview.value = null
      localEditMode.value = false
      errorMsg.value = null
      return
    }
    if (detail.value) {
      localForm.hydrateForm(detail.value)
    }
    if (key === lastEntryKey) return
    lastEntryKey = key
    errorMsg.value = null
    candidatePreview.value = null
    localEditMode.value = false
    detailPaneTab.value = 'local'
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
  if (!detail.value) return false
  if (detail.value.metadata.kind === 'book') {
    return detail.value.metadata.data.writable
  }
  return true
})

function resolveArtworkVersion(rootId: string, relPath: string): number {
  return artworkVersion.value + (artworkVersions.value?.[`${rootId}::${relPath}`] ?? 0)
}

const artworkSrc = computed(() => {
  if (!detail.value) return ''
  const kind = detail.value.entry.media_kind
  const ver = resolveArtworkVersion(detail.value.entry.root_id, detail.value.entry.rel_path)
  if (detail.value.metadata.kind === 'video') {
    const slot = videoArtworkSlot.value
    const has =
      slot === 'poster'
        ? detail.value.metadata.data.has_poster
        : slot === 'fanart'
          ? detail.value.metadata.data.has_fanart
          : detail.value.metadata.data.has_thumb
    if (!has) return ''
    return buildArtworkUrl(
      detail.value.entry.root_id,
      detail.value.entry.rel_path,
      'video',
      ver,
      slot,
    )
  }
  if (!detail.value.entry.completeness.has_cover) return ''
  return buildArtworkUrl(
    detail.value.entry.root_id,
    detail.value.entry.rel_path,
    kind,
    ver,
    'cover',
  )
})

function enterLocalEditMode(): void {
  if (!detail.value || !canSaveMetadata.value) return
  localForm.hydrateForm(detail.value)
  localEditMode.value = true
  detailPaneTab.value = 'local'
  mobilePane.value = 'local'
}

async function exitLocalEditMode(): Promise<void> {
  if (localForm.isDirty.value) {
    const ok = await dialogStore.confirm({
      title: t('tool.workbench.view.discardLocalTitle'),
      message: t('tool.workbench.view.discardLocalMsg'),
      confirmText: t('tool.workbench.view.discardLocalConfirm'),
      cancelText: t('tool.inspector.continueEditing'),
      danger: true,
    })
    if (!ok) return
  }
  if (detail.value) {
    localForm.hydrateForm(detail.value)
  }
  localEditMode.value = false
}

function triggerLocalArtworkUpload(): void {
  if (!detail.value) return
  pendingUploadSlot.value =
    detail.value.metadata.kind === 'video' ? videoArtworkSlot.value : 'cover'
  coverInputRef.value?.click()
}

async function handleLocalArtworkPicked(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file || !detail.value) return
  const seq = ++saveSeq
  const startKey = entryCompositeKey(detail.value.entry)
  uploadingArtwork.value = true
  errorMsg.value = null
  try {
    const updated = await toolApi.uploadArtwork(
      detail.value.entry.root_id,
      detail.value.entry.rel_path,
      file,
      pendingUploadSlot.value,
      detail.value.entry.media_kind,
    )
    if (seq !== saveSeq || !detail.value || entryCompositeKey(detail.value.entry) !== startKey) {
      return
    }
    lastEntryKey = entryCompositeKey(updated.entry)
    await scrapeStore.handleDetailUpdated(updated, true)
    snackbar.show(t('tool.inspector.coverUpdated'))
  } catch (err) {
    if (seq === saveSeq) {
      errorMsg.value = err instanceof Error ? err.message : String(err)
    }
  } finally {
    if (seq === saveSeq) {
      uploadingArtwork.value = false
    }
  }
}

async function handlePolishLocalBook(): Promise<void> {
  if (polishingBook.value || !detail.value || detail.value.entry.media_kind !== 'book') return
  const seq = ++saveSeq
  const startKey = entryCompositeKey(detail.value.entry)
  polishingBook.value = true
  errorMsg.value = null
  try {
    const report = await toolApi.polishBook(
      detail.value.entry.root_id,
      detail.value.entry.rel_path,
    )
    if (seq !== saveSeq || !detail.value || entryCompositeKey(detail.value.entry) !== startKey) {
      return
    }
    lastEntryKey = entryCompositeKey(report.detail.entry)
    localForm.hydrateForm(report.detail)
    await scrapeStore.handleDetailUpdated(report.detail, true)
    snackbar.show(
      t('tool.inspector.bookPolished', {
        step: report.jacket_updated
          ? t('tool.inspector.jacketUpdated')
          : t('tool.inspector.tocSynced'),
        removed: report.removed_orphan_files,
      }),
    )
  } catch (err) {
    if (seq === saveSeq) {
      errorMsg.value = err instanceof Error ? err.message : String(err)
    }
  } finally {
    if (seq === saveSeq) {
      polishingBook.value = false
    }
  }
}

async function saveLocalEdit(organizeAfterSave = false, advanceNext = false): Promise<void> {
  if (!detail.value || !canSaveMetadata.value || savingLocal.value) return
  const seq = ++saveSeq
  const startKey = entryCompositeKey(detail.value.entry)
  savingLocal.value = true
  errorMsg.value = null
  try {
    const envelope = localForm.buildPatchEnvelope(detail.value.entry.media_kind)
    const { detail: updated } = await saveAndOrganizeEntry({
      rootId: detail.value.entry.root_id,
      relPath: detail.value.entry.rel_path,
      mediaKind: detail.value.entry.media_kind,
      envelope,
      organizeAfterSave,
    })
    if (seq !== saveSeq || !detail.value || entryCompositeKey(detail.value.entry) !== startKey) {
      return
    }
    lastEntryKey = entryCompositeKey(updated.entry)
    localForm.hydrateForm(updated)
    localEditMode.value = false
    await scrapeStore.handleDetailUpdated(updated, false)
    snackbar.show({
      message: organizeAfterSave
        ? t('tool.inspector.savedAndArchived')
        : t('tool.workbench.view.savedLocal'),
      actionLabel: t('tool.inspector.historyAction'),
      onAction: () => workspaceStore.openHistoryModal('metadata'),
    })
    if (advanceNext && hasNext.value) {
      await scrapeStore.stepInspectingFile(1)
    }
  } catch (err) {
    if (seq === saveSeq) {
      errorMsg.value = err instanceof Error ? err.message : String(err)
    }
  } finally {
    if (seq === saveSeq) {
      savingLocal.value = false
    }
  }
}

async function confirmDiscardIfDirty(): Promise<boolean> {
  if (!isCurrentDirty.value) return true
  return dialogStore.confirm({
    title: t('tool.workbench.view.discardTitle'),
    message: t('tool.workbench.view.discardMsg'),
    confirmText: t('tool.workbench.view.discardConfirm'),
    cancelText: t('tool.workbench.view.stayEditing'),
    danger: true,
  })
}

async function requestStep(delta: -1 | 1): Promise<void> {
  if (saving.value || savingLocal.value) return
  if (delta < 0 && !hasPrev.value) return
  if (delta > 0 && !hasNext.value) return
  if (!(await confirmDiscardIfDirty())) return
  queueDropdownOpen.value = false
  await scrapeStore.stepInspectingFile(delta)
}

async function requestPickEntry(entry: MediaFileEntry): Promise<void> {
  if (saving.value || savingLocal.value) return
  queueDropdownOpen.value = false
  if (detail.value && entryCompositeKey(detail.value.entry) === entryCompositeKey(entry)) return
  if (!(await confirmDiscardIfDirty())) return
  await workspaceStore.inspectEntry(entry, { skipDirtyCheck: true })
}

async function requestExit(): Promise<void> {
  if (saving.value || savingLocal.value) return
  if (!(await confirmDiscardIfDirty())) return
  queueDropdownOpen.value = false
  scrapeStore.scrapeWorkbenchActive = false
  scrapeStore.scrapeWorkbenchPaths = null
  workspaceStore.mobileMenuOpen = false
  workspaceStore.syncRouteFromState(false)
}

async function handleApplyAndSave(
  meta: ScrapedMetadata,
  mode: ApplyFillMode,
  advanceNext = false,
  opts?: CandidateApplyOptions,
): Promise<void> {
  if (!detail.value || !canSaveMetadata.value || saving.value) return
  const seq = ++saveSeq
  const startKey = entryCompositeKey(detail.value.entry)
  saving.value = true
  errorMsg.value = null
  try {
    applyForm.hydrateForm(detail.value)
    applyForm.applyScrapedMetadata(detail.value.entry.media_kind, meta, mode)

    const envelope = applyForm.buildPatchEnvelope(detail.value.entry.media_kind)
    const organizeAfterSave = Boolean(opts?.organizeAfterSave)
    const { detail: updated, bumpedArtwork } = await saveAndOrganizeEntry({
      rootId: detail.value.entry.root_id,
      relPath: detail.value.entry.rel_path,
      mediaKind: detail.value.entry.media_kind,
      envelope,
      pendingCoverUrl: applyForm.pendingCoverUrl.value,
      pendingBackdropUrl: applyForm.pendingBackdropUrl.value,
      writeAlbumCover: opts?.writeAlbumCover,
      organizeAfterSave,
    })
    applyForm.pendingCoverUrl.value = null
    applyForm.pendingBackdropUrl.value = null

    if (seq !== saveSeq || !detail.value || entryCompositeKey(detail.value.entry) !== startKey) {
      return
    }
    lastEntryKey = entryCompositeKey(updated.entry)
    localForm.hydrateForm(updated)
    await scrapeStore.handleDetailUpdated(updated, bumpedArtwork)
    snackbar.show({
      message: organizeAfterSave
        ? t('tool.inspector.savedAndArchived')
        : t('tool.workbench.view.savedApplied'),
      actionLabel: t('tool.inspector.historyAction'),
      onAction: () => workspaceStore.openHistoryModal('metadata'),
    })

    if (advanceNext && hasNext.value) {
      await scrapeStore.stepInspectingFile(1)
    }
  } catch (err) {
    if (seq === saveSeq) {
      errorMsg.value = err instanceof Error ? err.message : String(err)
    }
  } finally {
    if (seq === saveSeq) {
      saving.value = false
    }
  }
}

function handleKeydown(ev: KeyboardEvent): void {
  if (
    typeof document !== 'undefined' &&
    document.querySelector('[aria-modal="true"], [role="menu"], [role="listbox"]')
  ) {
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
  inspectorStore.registerWorkbenchGuard({
    isDirty: () => isCurrentDirty.value,
    confirmDiscard: confirmDiscardIfDirty,
    requestPickEntry,
    requestStep,
    requestExit,
  })
})
onBeforeUnmount(() => {
  saveSeq += 1
  window.removeEventListener('keydown', handleKeydown)
  window.removeEventListener('resize', handleResize)
  inspectorStore.registerWorkbenchGuard(null)
})

defineExpose({
  isDirty: isCurrentDirty,
  confirmDiscardIfDirty,
  requestPickEntry,
  requestStep,
  requestExit,
})
</script>

<template>
  <div class="relative flex h-full w-full flex-col overflow-hidden bg-surface">
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
      @toggle-sidebar="workspaceStore.toggleSidebar()"
      @step="requestStep"
      @select-entry="requestPickEntry"
      @update:queue-dropdown-open="queueDropdownOpen = $event"
      @update:auto-search-on-switch="autoSearchOnSwitch = $event"
    />

    <!-- Loading Overlay when Switching Items -->
    <div
      v-if="loadingDetail && !detail"
      class="text-body-small flex flex-1 items-center justify-center text-on-surface-variant"
    >
      {{ t('tool.workbench.view.loadingEntry') }}
    </div>

    <!-- Empty Queue Fallback -->
    <div
      v-else-if="!detail"
      class="text-body-small flex flex-1 flex-col items-center justify-center gap-2 text-on-surface-variant"
    >
      <p>{{ t('tool.workbench.view.emptyQueue') }}</p>
      <MdButton variant="tonal" size="sm" @click="requestExit">
        {{ t('tool.workbench.view.backToMedia') }}
      </MdButton>
    </div>

    <!-- RESPONSIVE WORKBENCH:
         - <840px (<md): 1-column MdTabs (Local / Candidates / Preview)
         - 840px–1599px (md..xl): 2-column spacious split (Candidates left + Tabbed Local/Preview right)
         - >=1600px (xl+): 3-column full split (Local + Candidates + Preview) -->
    <div v-else class="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div class="shrink-0 bg-surface-container-low md:hidden">
        <MdTabs
          v-model="mobilePane"
          :tabs="mobilePaneTabs"
          variant="primary"
          :aria-label="t('tool.workbench.view.sectionsAria')"
        />
      </div>

      <div
        class="grid min-h-0 flex-1 grid-cols-1 overflow-hidden md:grid-cols-[340px_minmax(0,1fr)] xl:grid-cols-[minmax(0,1fr)_360px_minmax(0,1fr)]"
      >
        <!-- PANE: Local Persisted Entry -->
        <div
          class="min-h-0 flex-col overflow-hidden bg-surface-container-low md:order-2 xl:order-1 xl:flex"
          :class="[
            mobilePane === 'local' ? 'flex' : 'hidden',
            detailPaneTab === 'local' ? 'md:flex' : 'md:hidden',
          ]"
        >
          <div class="hidden shrink-0 bg-surface-container-low md:block xl:hidden">
            <MdTabs
              v-model="detailPaneTab"
              :tabs="detailPaneTabs"
              variant="secondary"
              :aria-label="t('tool.workbench.view.sectionsAria')"
            />
          </div>
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

        <!-- PANE: Scraper Sources & Candidate List -->
        <div
          class="min-h-0 flex-col border-outline-variant/45 bg-surface-container/40 md:order-1 md:flex md:border-r xl:order-2 xl:border-x"
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
              @open-settings="workspaceStore.openSettingsTab('scrapers')"
              @settings-updated="workspaceStore.handleSettingsUpdated($event)"
            />
          </div>
        </div>

        <!-- PANE: Editable Pending Result & Save Area -->
        <div
          class="min-h-0 flex-col overflow-hidden bg-surface-container-low md:order-2 xl:order-3 xl:flex"
          :class="[
            mobilePane === 'preview' ? 'flex' : 'hidden',
            detailPaneTab === 'preview' ? 'md:flex' : 'md:hidden',
          ]"
        >
          <div class="hidden shrink-0 bg-surface-container-low md:block xl:hidden">
            <MdTabs
              v-model="detailPaneTab"
              :tabs="detailPaneTabs"
              variant="secondary"
              :aria-label="t('tool.workbench.view.sectionsAria')"
            />
          </div>
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
            class="text-body-small flex flex-1 flex-col items-center justify-center gap-3 px-8 text-center text-on-surface-variant"
          >
            <div
              class="flex h-12 w-12 items-center justify-center rounded-corner-lg bg-surface-container-high"
            >
              <LuImage class="h-5 w-5 opacity-55" aria-hidden="true" />
            </div>
            <p class="text-title-small text-on-surface">
              {{ t('tool.workbench.view.waitingCandidate') }}
            </p>
            <p class="max-w-[260px] leading-relaxed opacity-85">
              {{ t('tool.workbench.view.waitingCandidateDesc') }}
            </p>
            <MdButton
              v-if="canSaveMetadata && !localEditMode"
              variant="tonal"
              size="sm"
              :icon="LuPencil"
              @click="enterLocalEditMode()"
            >
              {{ t('tool.workbench.view.editLocalDirectly') }}
            </MdButton>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
