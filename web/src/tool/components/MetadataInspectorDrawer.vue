<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuArrowDown, LuArrowUp, LuColumns3, LuX } from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdSheet from '@/components/ui/MdSheet.vue'
import MdSkeleton from '@/components/ui/MdSkeleton.vue'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { formatBytes } from '@/utils/format'
import { buildArtworkUrl, resolveScraperCoverUrl, toolApi } from '../api'
import { saveAndOrganizeEntry } from '../services/saveEntry'
import { useToolInspectorStore } from '../stores/inspector'
import type { MediaFileDetail, MediaFileEntry } from '../types'
import InspectorDetailBody from './inspector/InspectorDetailBody.vue'
import InspectorFooterBar from './inspector/InspectorFooterBar.vue'
import type { InspectorUploadSlot } from './inspector/InspectorQuickActionsMenu.vue'
import { useMetadataForm } from './inspector/useMetadataForm'

const props = withDefaults(
  defineProps<{
    open?: boolean
    modal?: boolean
    loading?: boolean
    detail: MediaFileDetail | null
    artworkVersion: number
    hasPrev?: boolean
    hasNext?: boolean
    positionLabel?: string
    shortcutsDisabled?: boolean
  }>(),
  { open: true, modal: false, loading: false, hasPrev: false, hasNext: false, positionLabel: '', shortcutsDisabled: false },
)

const emit = defineEmits<{
  close: []
  updated: [detail: MediaFileDetail, bumpArtwork: boolean]
  step: [delta: -1 | 1]
  openWorkbench: [detail: MediaFileDetail]
  openContentEditor: [entry: MediaFileEntry]
  openNormalizeEpub: [entry: MediaFileEntry]
  openComicPages: [entry: MediaFileEntry]
  openComicOptimize: [entry: MediaFileEntry]
  openMusicTools: [entry: MediaFileEntry]
  openVideoTools: [entry: MediaFileEntry]
  openRename: [entry: MediaFileEntry]
  requestDelete: [entry: MediaFileEntry]
  openHistory: []
}>()

const { t } = useI18n()
const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()
const inspectorStore = useToolInspectorStore()

const {
  bookForm, comicForm, musicForm, videoForm, pendingCoverUrl, pendingBackdropUrl,
  videoArtworkSlot, isDirty, isBookFieldLocked, toggleBookFieldLock,
  isVideoFieldLocked, toggleVideoFieldLock, hydrateForm, buildPatchEnvelope,
} = useMetadataForm()

const saving = ref(false)
const organizeAfterSave = ref(false)
const polishingBook = ref(false)
const uploadingArtwork = ref(false)
const errorMsg = ref<string | null>(null)
const drawerRootRef = ref<HTMLElement | null>(null)
const mdSheetRef = ref<InstanceType<typeof MdSheet> | null>(null)
const coverInputRef = ref<HTMLInputElement | null>(null)
const pendingSlot = ref<InspectorUploadSlot>('cover')
const skipNextHydrateKey = ref<string | null>(null)

function detailIdentityKey(d: MediaFileDetail): string {
  return `${d.entry.root_id}::${d.entry.rel_path}`
}

watch(
  () => props.detail,
  (d) => {
    if (!d) return
    if (skipNextHydrateKey.value === detailIdentityKey(d)) {
      skipNextHydrateKey.value = null
      return
    }
    skipNextHydrateKey.value = null
    errorMsg.value = null
    hydrateForm(d)
  },
  { immediate: true },
)

const isCurrentDirty = computed(() => Boolean(props.detail && isDirty.value))

const canSaveMetadata = computed(() => {
  if (!props.detail) return false
  if (props.detail.metadata.kind === 'book') return props.detail.metadata.data.writable
  return true
})

const headerSubtitle = computed(() => {
  if (!props.detail) return ''
  const size = formatBytes(props.detail.entry.file_size)
  return props.positionLabel ? `${props.positionLabel} · ${size}` : size
})

const artworkSrc = computed(() => {
  if (!props.detail) return null
  if (pendingCoverUrl.value) return resolveScraperCoverUrl(pendingCoverUrl.value)
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
  if (!isCurrentDirty.value || !props.detail) return true
  return dialogStore.confirm({
    title: t('tool.inspector.discardTitle'),
    message: t('tool.inspector.discardMessage', { name: props.detail.entry.file_name }),
    confirmText: t('tool.inspector.discardConfirm'),
    cancelText: t('tool.inspector.continueEditing'),
    danger: true,
  })
}

async function requestClose(): Promise<void> {
  if (saving.value || uploadingArtwork.value) return
  if (!(await confirmDiscardIfDirty())) return
  emit('close')
}

async function requestStep(delta: -1 | 1): Promise<void> {
  if (saving.value || uploadingArtwork.value) return
  if (delta < 0 && !props.hasPrev) return
  if (delta > 0 && !props.hasNext) return
  if (!(await confirmDiscardIfDirty())) return
  emit('step', delta)
}

function triggerArtworkUpload(slot: InspectorUploadSlot): void {
  pendingSlot.value = slot
  coverInputRef.value?.click()
}

async function handleArtworkPicked(ev: Event): Promise<void> {
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
      pendingSlot.value,
      props.detail.entry.media_kind,
    )
    pendingCoverUrl.value = null
    skipNextHydrateKey.value = detailIdentityKey(updated)
    emit('updated', updated, true)
    snackbar.show(t('tool.inspector.coverUpdated'))
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    uploadingArtwork.value = false
  }
}

async function handlePolishBook(): Promise<void> {
  if (polishingBook.value || !props.detail || props.detail.entry.media_kind !== 'book') return
  polishingBook.value = true
  errorMsg.value = null
  try {
    const report = await toolApi.polishBook(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
    )
    emit('updated', report.detail, true)
    snackbar.show(
      t('tool.inspector.bookPolished', {
        step: report.jacket_updated
          ? t('tool.inspector.jacketUpdated')
          : t('tool.inspector.tocSynced'),
        removed: report.removed_orphan_files,
      }),
    )
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    polishingBook.value = false
  }
}

async function saveMetadata(shouldOrganize = false, advanceNext = false): Promise<void> {
  if (!props.detail || !canSaveMetadata.value || saving.value) return
  saving.value = true
  errorMsg.value = null
  try {
    const envelope = buildPatchEnvelope(props.detail.entry.media_kind)
    const {
      detail: updated,
      bumpedArtwork,
      artworkWarnings,
    } = await saveAndOrganizeEntry({
      rootId: props.detail.entry.root_id,
      relPath: props.detail.entry.rel_path,
      mediaKind: props.detail.entry.media_kind,
      envelope,
      pendingCoverUrl: pendingCoverUrl.value,
      pendingBackdropUrl: pendingBackdropUrl.value,
      organizeAfterSave: shouldOrganize,
    })
    pendingCoverUrl.value = null
    pendingBackdropUrl.value = null

    hydrateForm(updated)
    emit('updated', updated, bumpedArtwork)
    if (artworkWarnings.length > 0) {
      errorMsg.value = artworkWarnings.join('；')
      snackbar.show({
        message: t('tool.inspector.savedWithWarnings', { warnings: artworkWarnings.join('；') }),
        tone: 'error',
      })
    } else {
      snackbar.show({
        message: shouldOrganize
          ? t('tool.inspector.savedAndArchived')
          : t('tool.inspector.savedChanges'),
        actionLabel: t('tool.inspector.historyAction'),
        onAction: () => emit('openHistory'),
      })
    }

    if (advanceNext && props.hasNext) emit('step', 1)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    saving.value = false
  }
}

function hasBlockingExternalOverlay(): boolean {
  if (typeof document === 'undefined') return false
  if (document.querySelector('[role="menu"], [role="listbox"]')) return true
  const rootEl = props.modal ? (mdSheetRef.value?.sheetRef ?? null) : drawerRootRef.value
  for (const modalEl of document.querySelectorAll('[aria-modal="true"]')) {
    if (!rootEl || (!modalEl.contains(rootEl) && !rootEl.contains(modalEl))) return true
  }
  return false
}

function handleKeydown(ev: KeyboardEvent): void {
  if (!props.open || props.shortcutsDisabled || !props.detail || hasBlockingExternalOverlay()) return
  if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === 's') {
    ev.preventDefault()
    void saveMetadata(organizeAfterSave.value, false)
  } else if ((ev.ctrlKey || ev.metaKey) && ev.key === 'Enter') {
    ev.preventDefault()
    void saveMetadata(organizeAfterSave.value, true)
  } else if (ev.altKey && ev.key === 'ArrowUp') {
    ev.preventDefault()
    void requestStep(-1)
  } else if (ev.altKey && ev.key === 'ArrowDown') {
    ev.preventDefault()
    void requestStep(1)
  }
}

onMounted(() => {
  window.addEventListener('keydown', handleKeydown)
  inspectorStore.registerInspectorGuard({
    isDirty: () => isCurrentDirty.value,
    confirmDiscard: confirmDiscardIfDirty,
    requestClose,
  })
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
  inspectorStore.registerInspectorGuard(null)
})

defineExpose({
  isDirty: isCurrentDirty,
  confirmDiscardIfDirty,
  save: () => saveMetadata(organizeAfterSave.value, false),
  requestClose,
})
</script>

<template>
  <input
    ref="coverInputRef"
    type="file"
    accept="image/jpeg,image/png,image/webp"
    class="hidden"
    @change="handleArtworkPicked"
  />

  <!-- Compact / Medium (<1200px): Authentic MD3 Adaptive Sheet -->
  <MdSheet
    v-if="modal"
    ref="mdSheetRef"
    :open="open"
    :title="detail?.entry.file_name || t('tool.inspector.title')"
    :subtitle="headerSubtitle"
    size="md"
    @close="requestClose"
  >
    <template #headerActions>
      <template v-if="detail && (hasPrev || hasNext)">
        <MdIconButton
          size="sm"
          :disabled="!hasPrev"
          :icon="LuArrowUp"
          :tooltip="t('tool.inspector.prevItem')"
          :aria-label="t('tool.inspector.prevItem')"
          @click="requestStep(-1)"
        />
        <MdIconButton
          size="sm"
          :disabled="!hasNext"
          :icon="LuArrowDown"
          :tooltip="t('tool.inspector.nextItem')"
          :aria-label="t('tool.inspector.nextItem')"
          @click="requestStep(1)"
        />
      </template>
      <MdButton
        v-if="detail"
        variant="tonal"
        :icon="LuColumns3"
        :title="t('tool.browser.workbenchTooltip')"
        @click="emit('openWorkbench', detail)"
      >
        {{ t('tool.browser.workbenchBtn') }}
      </MdButton>
    </template>

    <div v-if="loading && !detail" class="space-y-4 py-2" :aria-label="t('tool.workspace.parsingFile')">
      <div class="flex gap-3.5 rounded-corner-lg bg-surface p-3.5">
        <MdSkeleton width="6rem" height="8rem" />
        <div class="flex-1 space-y-2.5 py-1">
          <MdSkeleton variant="text" :lines="3" />
          <MdSkeleton width="7rem" height="2.5rem" />
        </div>
      </div>
      <MdSkeleton height="3.25rem" />
      <MdSkeleton height="3.25rem" />
      <MdSkeleton height="6.5rem" />
    </div>

    <InspectorDetailBody
      v-else-if="detail"
      :detail="detail"
      :artwork-src="artworkSrc"
      :has-pending-cover="Boolean(pendingCoverUrl)"
      :is-dirty="isCurrentDirty"
      show-dirty-badge
      :video-artwork-slot="videoArtworkSlot"
      :uploading-artwork="uploadingArtwork"
      :polishing-book="polishingBook"
      :book-cover-locked="isBookFieldLocked('cover')"
      :video-poster-locked="isVideoFieldLocked('poster')"
      :error-msg="errorMsg"
      :book-form="bookForm"
      :comic-form="comicForm"
      :music-form="musicForm"
      :video-form="videoForm"
      @update:video-artwork-slot="videoArtworkSlot = $event"
      @upload-artwork="triggerArtworkUpload"
      @toggle-book-cover-lock="toggleBookFieldLock('cover')"
      @toggle-video-poster-lock="toggleVideoFieldLock('poster')"
      @open-content-editor="emit('openContentEditor', $event)"
      @open-normalize-epub="emit('openNormalizeEpub', $event)"
      @polish-book="handlePolishBook"
      @open-comic-pages="emit('openComicPages', $event)"
      @open-comic-optimize="emit('openComicOptimize', $event)"
      @open-music-tools="emit('openMusicTools', $event)"
      @open-video-tools="emit('openVideoTools', $event)"
      @open-rename="emit('openRename', $event)"
      @request-delete="emit('requestDelete', $event)"
    />

    <template v-if="detail" #footer>
      <InspectorFooterBar
        v-model:organize-after-save="organizeAfterSave"
        :can-save-metadata="canSaveMetadata"
        :saving="saving"
        :has-next="hasNext"
        @save="saveMetadata(organizeAfterSave, false)"
        @save-and-next="saveMetadata(organizeAfterSave, true)"
      />
    </template>
  </MdSheet>

  <!-- Large Desktop (>=1200px): Docked Tonal Supporting Pane -->
  <aside
    v-else-if="open"
    ref="drawerRootRef"
    class="flex h-full w-[440px] shrink-0 flex-col overflow-hidden rounded-l-corner-xl bg-surface-container-low xl:w-[460px]"
  >
    <div
      v-if="loading && !detail"
      class="flex-1 space-y-4 p-4"
      :aria-label="t('tool.workspace.parsingFile')"
    >
      <div class="flex items-center justify-between gap-2 pb-1">
        <MdSkeleton width="12rem" height="1.5rem" />
        <MdSkeleton width="5.5rem" height="2.5rem" />
      </div>
      <div class="flex gap-3.5 rounded-corner-lg bg-surface p-3.5">
        <MdSkeleton width="6rem" height="8rem" />
        <div class="flex-1 space-y-2.5 py-1">
          <MdSkeleton variant="text" :lines="3" />
          <MdSkeleton width="7rem" height="2.5rem" />
        </div>
      </div>
      <MdSkeleton height="3.25rem" />
      <MdSkeleton height="3.25rem" />
      <MdSkeleton height="6.5rem" />
    </div>

    <template v-else-if="detail">
      <div class="flex shrink-0 items-center justify-between gap-2 px-4 pt-3.5 pb-2.5">
        <div class="flex min-w-0 items-center gap-1.5">
          <div
            v-if="hasPrev || hasNext"
            class="flex shrink-0 items-center rounded-corner-full bg-surface-container-high p-0.5"
          >
            <MdIconButton
              size="sm"
              :disabled="!hasPrev"
              :icon="LuArrowUp"
              :tooltip="t('tool.inspector.prevItem')"
              :aria-label="t('tool.inspector.prevItem')"
              @click="requestStep(-1)"
            />
            <MdIconButton
              size="sm"
              :disabled="!hasNext"
              :icon="LuArrowDown"
              :tooltip="t('tool.inspector.nextItem')"
              :aria-label="t('tool.inspector.nextItem')"
              @click="requestStep(1)"
            />
          </div>

          <div class="min-w-0">
            <div class="flex items-center gap-1.5">
              <span class="text-title-small truncate text-on-surface">
                {{ detail.entry.file_name }}
              </span>
              <span
                v-if="isCurrentDirty"
                class="text-label-small shrink-0 rounded-corner-full bg-warning-container px-2 py-0.5 text-on-warning-container"
              >
                {{ t('tool.inspector.unsaved') }}
              </span>
            </div>
            <div class="text-body-small mt-0.5 truncate text-on-surface-variant">
              {{ headerSubtitle }}
            </div>
          </div>
        </div>

        <div class="flex shrink-0 items-center gap-1.5">
          <MdButton
            variant="tonal"
            :icon="LuColumns3"
            :title="t('tool.browser.workbenchTooltip')"
            @click="emit('openWorkbench', detail)"
          >
            {{ t('tool.browser.workbenchBtn') }}
          </MdButton>

          <MdIconButton
            :icon="LuX"
            :tooltip="t('tool.inspector.closePanel')"
            :aria-label="t('tool.inspector.closePanel')"
            @click="requestClose"
          />
        </div>
      </div>

      <div class="flex-1 overflow-y-auto px-4 pt-1 pb-4">
        <InspectorDetailBody
          :detail="detail"
          :artwork-src="artworkSrc"
          :has-pending-cover="Boolean(pendingCoverUrl)"
          :is-dirty="isCurrentDirty"
          :video-artwork-slot="videoArtworkSlot"
          :uploading-artwork="uploadingArtwork"
          :polishing-book="polishingBook"
          :book-cover-locked="isBookFieldLocked('cover')"
          :video-poster-locked="isVideoFieldLocked('poster')"
          :error-msg="errorMsg"
          :book-form="bookForm"
          :comic-form="comicForm"
          :music-form="musicForm"
          :video-form="videoForm"
          @update:video-artwork-slot="videoArtworkSlot = $event"
          @upload-artwork="triggerArtworkUpload"
          @toggle-book-cover-lock="toggleBookFieldLock('cover')"
          @toggle-video-poster-lock="toggleVideoFieldLock('poster')"
          @open-content-editor="emit('openContentEditor', $event)"
          @open-normalize-epub="emit('openNormalizeEpub', $event)"
          @polish-book="handlePolishBook"
          @open-comic-pages="emit('openComicPages', $event)"
          @open-comic-optimize="emit('openComicOptimize', $event)"
          @open-music-tools="emit('openMusicTools', $event)"
          @open-video-tools="emit('openVideoTools', $event)"
          @open-rename="emit('openRename', $event)"
          @request-delete="emit('requestDelete', $event)"
        />
      </div>

      <div class="shrink-0 bg-surface-container/75 px-4 py-3">
        <InspectorFooterBar
          v-model:organize-after-save="organizeAfterSave"
          :can-save-metadata="canSaveMetadata"
          :saving="saving"
          :has-next="hasNext"
          @save="saveMetadata(organizeAfterSave, false)"
          @save-and-next="saveMetadata(organizeAfterSave, true)"
        />
      </div>
    </template>
  </aside>
</template>
