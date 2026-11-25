<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowDown,
  LuArrowUp,
  LuBookOpen,
  LuClapperboard,
  LuColumns3,
  LuDisc3,
  LuMoreVertical,
  LuFileSignature,
  LuFileText,
  LuFilm,
  LuImage,
  LuLayers,
  LuLock,
  LuMusic,
  LuPackageCheck,
  LuSave,
  LuSkipForward,
  LuTrash2,
  LuUnlock,
  LuUpload,
  LuWand2,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdCircularProgress from '@/components/ui/MdCircularProgress.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdMenu from '@/components/ui/MdMenu.vue'
import MdMenuItem from '@/components/ui/MdMenuItem.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdSheet from '@/components/ui/MdSheet.vue'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { formatBytes, formatDuration } from '@/utils/format'
import { buildArtworkUrl, resolveScraperCoverUrl, toolApi } from '../api'
import { saveAndOrganizeEntry } from '../services/saveEntry'
import { useToolInspectorStore } from '../stores/inspector'
import type { MediaFileDetail, MediaFileEntry } from '../types'
import InspectorBookForm from './inspector/InspectorBookForm.vue'
import InspectorComicForm from './inspector/InspectorComicForm.vue'
import InspectorMusicForm from './inspector/InspectorMusicForm.vue'
import InspectorVideoForm from './inspector/InspectorVideoForm.vue'
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
  {
    open: true,
    modal: false,
    loading: false,
    hasPrev: false,
    hasNext: false,
    positionLabel: '',
    shortcutsDisabled: false,
  },
)

const emit = defineEmits<{
  close: []
  updated: [detail: MediaFileDetail, bumpArtwork: boolean]
  step: [delta: -1 | 1]
  openWorkbench: [detail: MediaFileDetail]
  openContentEditor: [entry: MediaFileEntry]
  openNormalizeEpub: [entry: MediaFileEntry]
  openComicPages: [entry: MediaFileEntry]
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
} = useMetadataForm()

const saving = ref(false)
const organizeAfterSave = ref(false)
const polishingBook = ref(false)
const uploadingArtwork = ref(false)
const errorMsg = ref<string | null>(null)
const drawerRootRef = ref<HTMLElement | null>(null)
const mdSheetRef = ref<InstanceType<typeof MdSheet> | null>(null)
const coverInputRef = ref<HTMLInputElement | null>(null)
const pendingSlot = ref<'cover' | 'series_cover' | 'poster' | 'fanart' | 'thumb'>('cover')
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
  if (props.detail.metadata.kind === 'book') {
    return props.detail.metadata.data.writable
  }
  return true
})

const headerSubtitle = computed(() => {
  if (!props.detail) return ''
  const size = formatBytes(props.detail.entry.file_size)
  return props.positionLabel ? `${props.positionLabel} · ${size}` : size
})

const videoSlotOptions = computed(() => {
  if (!props.detail || props.detail.metadata.kind !== 'video') return []
  return [
    { value: 'poster' as const, label: t('tool.inspector.slotPoster') },
    { value: 'fanart' as const, label: t('tool.inspector.slotFanart') },
    { value: 'thumb' as const, label: t('tool.inspector.slotThumb') },
  ]
})

const artworkSrc = computed(() => {
  if (!props.detail) return null
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

    if (advanceNext && props.hasNext) {
      emit('step', 1)
    }
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
  const modals = document.querySelectorAll('[aria-modal="true"]')
  for (const modalEl of modals) {
    if (!rootEl || (!modalEl.contains(rootEl) && !rootEl.contains(modalEl))) {
      return true
    }
  }
  return false
}

function handleKeydown(ev: KeyboardEvent): void {
  if (!props.open || props.shortcutsDisabled || !props.detail) return
  if (hasBlockingExternalOverlay()) return
  if ((ev.ctrlKey || ev.metaKey) && ev.key.toLowerCase() === 's') {
    ev.preventDefault()
    void saveMetadata(organizeAfterSave.value, false)
    return
  }
  if ((ev.ctrlKey || ev.metaKey) && ev.key === 'Enter') {
    ev.preventDefault()
    void saveMetadata(organizeAfterSave.value, true)
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
        size="sm"
        :icon="LuColumns3"
        :title="t('tool.browser.workbenchTooltip')"
        @click="emit('openWorkbench', detail)"
      >
        {{ t('tool.browser.workbenchBtn') }}
      </MdButton>
    </template>

    <div
      v-if="loading && !detail"
      class="text-body-small flex min-h-48 items-center justify-center gap-2 text-on-surface-variant"
    >
      <MdCircularProgress :size="18" :aria-label="t('tool.workspace.parsingFile')" />
      <span>{{ t('tool.workspace.parsingFile') }}</span>
    </div>

    <div v-else-if="detail" class="space-y-4">
      <div class="flex gap-3.5 rounded-corner-lg bg-surface p-3.5">
        <div class="flex shrink-0 flex-col items-center gap-1.5">
          <div
            class="relative overflow-hidden rounded-corner-md bg-surface-container-high"
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
              class="flex h-full w-full flex-col items-center justify-center gap-1 text-on-surface-variant"
            >
              <LuBookOpen v-if="detail.entry.media_kind === 'book'" class="h-6 w-6 opacity-55" aria-hidden="true" />
              <LuLayers v-else-if="detail.entry.media_kind === 'comic'" class="h-6 w-6 opacity-55" aria-hidden="true" />
              <LuDisc3 v-else-if="detail.entry.media_kind === 'music'" class="h-6 w-6 opacity-55" aria-hidden="true" />
              <LuClapperboard v-else class="h-6 w-6 opacity-55" aria-hidden="true" />
              <span class="text-label-small">{{ t('tool.inspector.noCover') }}</span>
            </div>
            <span
              v-if="pendingCoverUrl"
              class="text-label-small absolute inset-x-0 bottom-0 bg-primary/90 py-0.5 text-center text-on-primary"
            >
              {{ t('tool.inspector.pendingCover') }}
            </span>
          </div>

          <MdSegmentedButton
            v-if="detail.metadata.kind === 'video'"
            v-model="videoArtworkSlot"
            :options="videoSlotOptions"
            size="sm"
          />
        </div>

        <div class="flex min-w-0 flex-1 flex-col justify-between gap-2">
          <div>
            <div class="flex flex-wrap items-center gap-1.5">
              <span
                class="text-label-small rounded-corner-full bg-primary-container px-2.5 py-0.5 font-mono uppercase text-on-primary-container"
              >
                {{ detail.entry.extension }}
              </span>
              <span
                v-if="isCurrentDirty"
                class="text-label-small rounded-corner-full bg-warning-container px-2 py-0.5 text-on-warning-container"
              >
                {{ t('tool.inspector.unsaved') }}
              </span>
              <template v-if="detail.metadata.kind === 'music'">
                <span class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant">
                  {{ formatDuration(detail.metadata.data.duration_secs, '—') }}
                </span>
                <span
                  v-if="detail.metadata.data.bit_rate_kbps"
                  class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 font-mono text-on-surface-variant"
                >
                  {{ detail.metadata.data.bit_rate_kbps }} kbps
                </span>
              </template>
              <template v-else-if="detail.metadata.kind === 'comic'">
                <span class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant">
                  {{ t('tool.inspector.pagesCount', { count: detail.metadata.data.page_count }) }}
                </span>
              </template>
              <template v-else-if="detail.metadata.kind === 'video' && detail.metadata.data.width && detail.metadata.data.height">
                <span class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 font-mono text-on-surface-variant">
                  {{ detail.metadata.data.width }}×{{ detail.metadata.data.height }}
                </span>
              </template>
              <template v-else-if="detail.metadata.kind === 'book'">
                <span class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant">
                  {{
                    detail.metadata.data.format === 'epub'
                      ? t('tool.inspector.epubEmbedded')
                      : t('tool.inspector.sidecarOpf', {
                          format: detail.metadata.data.format.toUpperCase(),
                        })
                  }}
                </span>
              </template>
            </div>

            <div
              v-if="detail.metadata.kind === 'video'"
              class="text-body-small mt-2 space-y-0.5 text-on-surface-variant"
            >
              <div v-if="detail.metadata.data.audio_streams.length > 0" class="truncate">
                {{
                  t('tool.inspector.audioStreams', {
                    summary: detail.metadata.data.audio_streams
                      .map((a) => `${a.codec.toUpperCase()}${a.language ? ` [${a.language}]` : ''}`)
                      .join(' · '),
                  })
                }}
              </div>
              <div
                v-if="
                  detail.metadata.data.subtitle_streams.length > 0 ||
                  detail.metadata.data.sidecar_subtitles.length > 0
                "
                class="truncate"
              >
                {{
                  t('tool.inspector.subtitlesSummary', {
                    embedded: detail.metadata.data.subtitle_streams.length,
                    sidecar: detail.metadata.data.sidecar_subtitles.length,
                  })
                }}
              </div>
            </div>
          </div>

          <!-- Decluttered Cover Action Bar: 1 Primary Action + Overflow MdMenu -->
          <div class="flex items-center gap-1.5">
            <MdButton
              v-if="
                detail.entry.media_kind === 'book' &&
                (detail.entry.extension === 'txt' || detail.entry.extension === 'epub')
              "
              variant="tonal"
              size="sm"
              :icon="LuFileText"
              @click="emit('openContentEditor', detail.entry)"
            >
              {{
                detail.entry.extension === 'txt'
                  ? t('tool.inspector.txtConvertEpub')
                  : t('tool.inspector.editContent')
              }}
            </MdButton>
            <MdButton
              v-else-if="detail.entry.media_kind === 'comic'"
              variant="tonal"
              size="sm"
              :icon="LuImage"
              @click="emit('openComicPages', detail.entry)"
            >
              {{ t('tool.inspector.comicPages') }}
            </MdButton>
            <MdButton
              v-else-if="detail.entry.media_kind === 'music'"
              variant="tonal"
              size="sm"
              :icon="LuMusic"
              @click="emit('openMusicTools', detail.entry)"
            >
              {{ t('tool.inspector.musicToolbox') }}
            </MdButton>
            <MdButton
              v-else-if="detail.entry.media_kind === 'video'"
              variant="tonal"
              size="sm"
              :icon="LuFilm"
              @click="emit('openVideoTools', detail.entry)"
            >
              {{ t('tool.inspector.videoToolbox') }}
            </MdButton>
            <MdButton
              v-else
              variant="tonal"
              size="sm"
              :icon="LuUpload"
              :loading="uploadingArtwork"
              @click="triggerArtworkUpload('cover')"
            >
              {{ t('tool.actions.uploadCover') }}
            </MdButton>

            <MdMenu
              placement="bottom-end"
              :min-width="220"
              :aria-label="t('tool.inspector.moreActions')"
            >
              <template #trigger="{ toggle, triggerAttrs }">
                <MdIconButton
                  variant="tonal"
                  size="sm"
                  :icon="LuMoreVertical"
                  :tooltip="t('tool.inspector.moreActions')"
                  :aria-label="t('tool.inspector.moreActions')"
                  v-bind="triggerAttrs"
                  @click="toggle"
                />
              </template>

              <template v-if="detail.entry.media_kind !== 'video'">
                <MdMenuItem
                  :icon="LuUpload"
                  :disabled="uploadingArtwork"
                  :label="
                    detail.entry.media_kind === 'comic'
                      ? t('tool.inspector.chapterCover')
                      : t('tool.actions.uploadCover')
                  "
                  @select="triggerArtworkUpload('cover')"
                />
                <MdMenuItem
                  v-if="detail.entry.media_kind === 'comic' && detail.entry.rel_path.includes('/')"
                  :icon="LuImage"
                  :disabled="uploadingArtwork"
                  :label="t('tool.inspector.seriesCover')"
                  @select="triggerArtworkUpload('series_cover')"
                />
                <MdMenuItem
                  v-if="detail.metadata.kind === 'book' && detail.metadata.data.writable"
                  :icon="isBookFieldLocked('cover') ? LuUnlock : LuLock"
                  :label="
                    isBookFieldLocked('cover')
                      ? t('tool.inspector.coverLocked')
                      : t('tool.inspector.lockCover')
                  "
                  @select="toggleBookFieldLock('cover')"
                />
                <MdMenuItem
                  v-if="
                    detail.entry.media_kind === 'book' &&
                    (detail.entry.extension === 'txt' ||
                      detail.entry.extension === 'epub' ||
                      detail.entry.extension === 'pdf')
                  "
                  :icon="LuPackageCheck"
                  :label="
                    detail.entry.completeness.is_normalized
                      ? t('tool.inspector.normalizedEpub')
                      : t('tool.inspector.normalizeToEpub')
                  "
                  @select="emit('openNormalizeEpub', detail.entry)"
                />
                <MdMenuItem
                  v-if="detail.entry.media_kind === 'book' && detail.entry.extension === 'epub'"
                  :icon="LuWand2"
                  :disabled="polishingBook"
                  :label="t('tool.inspector.polishBook')"
                  @select="handlePolishBook"
                />
              </template>
              <template v-else>
                <MdMenuItem
                  :icon="LuImage"
                  :disabled="uploadingArtwork"
                  :label="t('tool.actions.uploadPoster')"
                  @select="triggerArtworkUpload('poster')"
                />
                <MdMenuItem
                  :icon="LuUpload"
                  :disabled="uploadingArtwork"
                  :label="t('tool.actions.uploadFanart')"
                  @select="triggerArtworkUpload('fanart')"
                />
                <MdMenuItem
                  :icon="isVideoFieldLocked('poster') ? LuUnlock : LuLock"
                  :label="
                    isVideoFieldLocked('poster')
                      ? t('tool.inspector.posterLocked')
                      : t('tool.inspector.lockPoster')
                  "
                  @select="toggleVideoFieldLock('poster')"
                />
              </template>

              <MdMenuItem
                :icon="LuFileSignature"
                :disabled="Boolean(detail.entry.completeness.is_organized)"
                :label="
                  detail.entry.completeness.is_organized
                    ? t('tool.inspector.organized')
                    : t('tool.inspector.organize')
                "
                @select="emit('openRename', detail.entry)"
              />
              <MdMenuItem
                destructive
                :icon="LuTrash2"
                :label="t('tool.inspector.deleteEntry')"
                @select="emit('requestDelete', detail.entry)"
              />
            </MdMenu>
          </div>
        </div>
      </div>

      <div
        v-if="errorMsg"
        role="alert"
        class="text-body-small rounded-corner-lg bg-error-container px-3.5 py-2.5 text-on-error-container"
      >
        {{ errorMsg }}
      </div>

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

    <template v-if="detail" #footer>
      <div class="flex w-full items-center justify-between gap-2">
        <AppCheckbox
          v-model="organizeAfterSave"
          :disabled="!canSaveMetadata || saving"
          :label="t('tool.inspector.saveAndArchive')"
        />

        <div class="flex shrink-0 items-center gap-1.5">
          <MdIconButton
            v-if="hasNext"
            variant="tonal"
            size="sm"
            :disabled="!canSaveMetadata || saving"
            :icon="LuSkipForward"
            :tooltip="t('tool.inspector.saveAndNext')"
            :aria-label="t('tool.inspector.saveAndNext')"
            @click="saveMetadata(organizeAfterSave, true)"
          />
          <MdButton
            variant="filled"
            size="sm"
            :icon="LuSave"
            :loading="saving"
            :disabled="!canSaveMetadata"
            @click="saveMetadata(organizeAfterSave, false)"
          >
            {{ saving ? t('tool.actions.saving') : t('tool.actions.save') }}
          </MdButton>
        </div>
      </div>
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
      class="text-body-small flex flex-1 items-center justify-center gap-2 px-8 text-on-surface-variant"
    >
      <MdCircularProgress :size="18" :aria-label="t('tool.workspace.parsingFile')" />
      <span>{{ t('tool.workspace.parsingFile') }}</span>
    </div>

    <template v-else-if="detail">
      <!-- Supporting Pane Header -->
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

        <div class="flex shrink-0 items-center gap-1">
          <MdButton
            variant="tonal"
            size="sm"
            :icon="LuColumns3"
            :title="t('tool.browser.workbenchTooltip')"
            @click="emit('openWorkbench', detail)"
          >
            {{ t('tool.browser.workbenchBtn') }}
          </MdButton>

          <MdIconButton
            size="sm"
            :icon="LuX"
            :tooltip="t('tool.inspector.closePanel')"
            :aria-label="t('tool.inspector.closePanel')"
            @click="requestClose"
          />
        </div>
      </div>

      <!-- Supporting Pane Body -->
      <div class="flex-1 space-y-4 overflow-y-auto px-4 pt-1 pb-4">
        <div class="flex gap-3.5 rounded-corner-lg bg-surface p-3.5">
          <div class="flex shrink-0 flex-col items-center gap-1.5">
            <div
              class="relative overflow-hidden rounded-corner-md bg-surface-container-high"
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
                class="flex h-full w-full flex-col items-center justify-center gap-1 text-on-surface-variant"
              >
                <LuBookOpen v-if="detail.entry.media_kind === 'book'" class="h-6 w-6 opacity-55" aria-hidden="true" />
                <LuLayers v-else-if="detail.entry.media_kind === 'comic'" class="h-6 w-6 opacity-55" aria-hidden="true" />
                <LuDisc3 v-else-if="detail.entry.media_kind === 'music'" class="h-6 w-6 opacity-55" aria-hidden="true" />
                <LuClapperboard v-else class="h-6 w-6 opacity-55" aria-hidden="true" />
                <span class="text-label-small">{{ t('tool.inspector.noCover') }}</span>
              </div>
              <span
                v-if="pendingCoverUrl"
                class="text-label-small absolute inset-x-0 bottom-0 bg-primary/90 py-0.5 text-center text-on-primary"
              >
                {{ t('tool.inspector.pendingCover') }}
              </span>
            </div>

            <MdSegmentedButton
              v-if="detail.metadata.kind === 'video'"
              v-model="videoArtworkSlot"
              :options="videoSlotOptions"
              size="sm"
            />
          </div>

          <div class="flex min-w-0 flex-1 flex-col justify-between gap-2">
            <div>
              <div class="flex flex-wrap items-center gap-1.5">
                <span
                  class="text-label-small rounded-corner-full bg-primary-container px-2.5 py-0.5 font-mono uppercase text-on-primary-container"
                >
                  {{ detail.entry.extension }}
                </span>

                <template v-if="detail.metadata.kind === 'music'">
                  <span class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant">
                    {{ formatDuration(detail.metadata.data.duration_secs, '—') }}
                  </span>
                  <span
                    v-if="detail.metadata.data.bit_rate_kbps"
                    class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 font-mono text-on-surface-variant"
                  >
                    {{ detail.metadata.data.bit_rate_kbps }} kbps
                  </span>
                </template>

                <template v-else-if="detail.metadata.kind === 'comic'">
                  <span class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant">
                    {{ t('tool.inspector.pagesCount', { count: detail.metadata.data.page_count }) }}
                  </span>
                  <span class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant">
                    {{
                      detail.metadata.data.has_comic_info_xml
                        ? t('tool.inspector.hasComicInfo')
                        : t('tool.inspector.noComicInfo')
                    }}
                  </span>
                </template>

                <template v-else-if="detail.metadata.kind === 'video'">
                  <span
                    v-if="detail.metadata.data.width && detail.metadata.data.height"
                    class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 font-mono text-on-surface-variant"
                  >
                    {{ detail.metadata.data.width }}×{{ detail.metadata.data.height }}
                  </span>
                  <span
                    v-if="detail.metadata.data.video_codec"
                    class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 font-mono uppercase text-on-surface-variant"
                  >
                    {{ detail.metadata.data.video_codec }}
                  </span>
                </template>

                <template v-else-if="detail.metadata.kind === 'book'">
                  <span class="text-label-small rounded-corner-full bg-surface-container px-2 py-0.5 text-on-surface-variant">
                    {{
                      detail.metadata.data.format === 'epub'
                        ? t('tool.inspector.epubEmbedded')
                        : t('tool.inspector.sidecarOpf', {
                            format: detail.metadata.data.format.toUpperCase(),
                          })
                    }}
                  </span>
                </template>
              </div>

              <div
                v-if="detail.metadata.kind === 'video'"
                class="text-body-small mt-2 space-y-0.5 text-on-surface-variant"
              >
                <div v-if="detail.metadata.data.audio_streams.length > 0" class="truncate">
                  {{
                    t('tool.inspector.audioStreams', {
                      summary: detail.metadata.data.audio_streams
                        .map((a) => `${a.codec.toUpperCase()}${a.language ? ` [${a.language}]` : ''}`)
                        .join(' · '),
                    })
                  }}
                </div>
                <div
                  v-if="
                    detail.metadata.data.subtitle_streams.length > 0 ||
                    detail.metadata.data.sidecar_subtitles.length > 0
                  "
                  class="truncate"
                >
                  {{
                    t('tool.inspector.subtitlesSummary', {
                      embedded: detail.metadata.data.subtitle_streams.length,
                      sidecar: detail.metadata.data.sidecar_subtitles.length,
                    })
                  }}
                </div>
              </div>
            </div>

            <!-- Decluttered Cover Action Bar: 1 Primary Action + Overflow MdMenu -->
            <div class="flex items-center gap-1.5">
              <MdButton
                v-if="
                  detail.entry.media_kind === 'book' &&
                  (detail.entry.extension === 'txt' || detail.entry.extension === 'epub')
                "
                variant="tonal"
                size="sm"
                :icon="LuFileText"
                @click="emit('openContentEditor', detail.entry)"
              >
                {{
                  detail.entry.extension === 'txt'
                    ? t('tool.inspector.txtConvertEpub')
                    : t('tool.inspector.editContent')
                }}
              </MdButton>
              <MdButton
                v-else-if="detail.entry.media_kind === 'comic'"
                variant="tonal"
                size="sm"
                :icon="LuImage"
                @click="emit('openComicPages', detail.entry)"
              >
                {{ t('tool.inspector.comicPages') }}
              </MdButton>
              <MdButton
                v-else-if="detail.entry.media_kind === 'music'"
                variant="tonal"
                size="sm"
                :icon="LuMusic"
                @click="emit('openMusicTools', detail.entry)"
              >
                {{ t('tool.inspector.musicToolbox') }}
              </MdButton>
              <MdButton
                v-else-if="detail.entry.media_kind === 'video'"
                variant="tonal"
                size="sm"
                :icon="LuFilm"
                @click="emit('openVideoTools', detail.entry)"
              >
                {{ t('tool.inspector.videoToolbox') }}
              </MdButton>
              <MdButton
                v-else
                variant="tonal"
                size="sm"
                :icon="LuUpload"
                :loading="uploadingArtwork"
                @click="triggerArtworkUpload('cover')"
              >
                {{ t('tool.actions.uploadCover') }}
              </MdButton>

              <MdMenu
                placement="bottom-end"
                :min-width="220"
                :aria-label="t('tool.inspector.moreActions')"
              >
                <template #trigger="{ toggle, triggerAttrs }">
                  <MdIconButton
                    variant="tonal"
                    size="sm"
                    :icon="LuMoreVertical"
                    :tooltip="t('tool.inspector.moreActions')"
                    :aria-label="t('tool.inspector.moreActions')"
                    v-bind="triggerAttrs"
                    @click="toggle"
                  />
                </template>

                <template v-if="detail.entry.media_kind !== 'video'">
                  <MdMenuItem
                    :icon="LuUpload"
                    :disabled="uploadingArtwork"
                    :label="
                      detail.entry.media_kind === 'comic'
                        ? t('tool.inspector.chapterCover')
                        : t('tool.actions.uploadCover')
                    "
                    @select="triggerArtworkUpload('cover')"
                  />
                  <MdMenuItem
                    v-if="detail.entry.media_kind === 'comic' && detail.entry.rel_path.includes('/')"
                    :icon="LuImage"
                    :disabled="uploadingArtwork"
                    :label="t('tool.inspector.seriesCover')"
                    @select="triggerArtworkUpload('series_cover')"
                  />
                  <MdMenuItem
                    v-if="detail.metadata.kind === 'book' && detail.metadata.data.writable"
                    :icon="isBookFieldLocked('cover') ? LuUnlock : LuLock"
                    :label="
                      isBookFieldLocked('cover')
                        ? t('tool.inspector.coverLocked')
                        : t('tool.inspector.lockCover')
                    "
                    @select="toggleBookFieldLock('cover')"
                  />
                  <MdMenuItem
                    v-if="
                      detail.entry.media_kind === 'book' &&
                      (detail.entry.extension === 'txt' ||
                        detail.entry.extension === 'epub' ||
                        detail.entry.extension === 'pdf')
                    "
                    :icon="LuPackageCheck"
                    :label="
                      detail.entry.completeness.is_normalized
                        ? t('tool.inspector.normalizedEpub')
                        : t('tool.inspector.normalizeToEpub')
                    "
                    @select="emit('openNormalizeEpub', detail.entry)"
                  />
                  <MdMenuItem
                    v-if="detail.entry.media_kind === 'book' && detail.entry.extension === 'epub'"
                    :icon="LuWand2"
                    :disabled="polishingBook"
                    :label="t('tool.inspector.polishBook')"
                    @select="handlePolishBook"
                  />
                </template>
                <template v-else>
                  <MdMenuItem
                    :icon="LuImage"
                    :disabled="uploadingArtwork"
                    :label="t('tool.actions.uploadPoster')"
                    @select="triggerArtworkUpload('poster')"
                  />
                  <MdMenuItem
                    :icon="LuUpload"
                    :disabled="uploadingArtwork"
                    :label="t('tool.actions.uploadFanart')"
                    @select="triggerArtworkUpload('fanart')"
                  />
                  <MdMenuItem
                    :icon="isVideoFieldLocked('poster') ? LuUnlock : LuLock"
                    :label="
                      isVideoFieldLocked('poster')
                        ? t('tool.inspector.posterLocked')
                        : t('tool.inspector.lockPoster')
                    "
                    @select="toggleVideoFieldLock('poster')"
                  />
                </template>

                <MdMenuItem
                  :icon="LuFileSignature"
                  :disabled="Boolean(detail.entry.completeness.is_organized)"
                  :label="
                    detail.entry.completeness.is_organized
                      ? t('tool.inspector.organized')
                      : t('tool.inspector.organize')
                  "
                  @select="emit('openRename', detail.entry)"
                />
                <MdMenuItem
                  destructive
                  :icon="LuTrash2"
                  :label="t('tool.inspector.deleteEntry')"
                  @select="emit('requestDelete', detail.entry)"
                />
              </MdMenu>
            </div>
          </div>
        </div>

        <div
          v-if="errorMsg"
          role="alert"
          class="text-body-small rounded-corner-lg bg-error-container px-3.5 py-2.5 text-on-error-container"
        >
          {{ errorMsg }}
        </div>

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

      <!-- Consolidated Bottom Save Bar: 1 Primary Save Button + Clean Secondary Option -->
      <div
        class="flex shrink-0 items-center justify-between gap-2 bg-surface-container/75 px-4 py-3"
      >
        <AppCheckbox
          v-model="organizeAfterSave"
          :disabled="!canSaveMetadata || saving"
          :label="t('tool.inspector.saveAndArchive')"
        />

        <div class="flex shrink-0 items-center gap-1.5">
          <MdIconButton
            v-if="hasNext"
            variant="tonal"
            size="sm"
            :disabled="!canSaveMetadata || saving"
            :icon="LuSkipForward"
            :tooltip="t('tool.inspector.saveAndNext')"
            :aria-label="t('tool.inspector.saveAndNext')"
            @click="saveMetadata(organizeAfterSave, true)"
          />

          <MdButton
            variant="filled"
            size="sm"
            :icon="LuSave"
            :loading="saving"
            :disabled="!canSaveMetadata"
            :title="t('tool.inspector.shortcutsHint')"
            @click="saveMetadata(organizeAfterSave, false)"
          >
            {{ saving ? t('tool.actions.saving') : t('tool.actions.save') }}
          </MdButton>
        </div>
      </div>
    </template>
  </aside>
</template>
