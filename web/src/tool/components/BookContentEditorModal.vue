<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, toRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuLoader2, LuX } from 'vue-icons-plus/lu'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import { uploadedFonts } from '@/theme/customFonts'
import type { MediaFileDetail, MediaFileEntry } from '../types'
import BookEditorCssPanel from './book-editor/BookEditorCssPanel.vue'
import BookEditorFindReplaceBar from './book-editor/BookEditorFindReplaceBar.vue'
import BookEditorHeader from './book-editor/BookEditorHeader.vue'
import BookEditorProofreadPanel from './book-editor/BookEditorProofreadPanel.vue'
import BookEditorSurfacePane from './book-editor/BookEditorSurfacePane.vue'
import BookEditorTocSidebar from './book-editor/BookEditorTocSidebar.vue'
import BookEditorToolbar from './book-editor/BookEditorToolbar.vue'
import { buildLivePreviewSrcdoc as buildPreviewDoc } from './book-editor/previewDoc'
import {
  useBookEditorChapters,
  type ChapterRecord,
  type SurfaceMode,
} from './book-editor/useBookEditorChapters'
import { useBookEditorDocument } from './book-editor/useBookEditorDocument'
import { useBookEditorFindReplace } from './book-editor/useBookEditorFindReplace'
import { useBookEditorSurface } from './book-editor/useBookEditorSurface'

const props = defineProps<{
  open: boolean
  entry: MediaFileEntry | null
}>()

const emit = defineEmits<{
  close: []
  saved: [detail: MediaFileDetail]
  converted: [detail: MediaFileDetail, deletedSourceTxt: boolean]
}>()

const { t } = useI18n()
const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()
const themeStore = useThemeStore()

const errorMsg = ref<string | null>(null)
const docFormat = ref<'txt' | 'epub'>('epub')
const docTitle = ref('')
const docAuthors = ref<string[]>([])
const docStylesheets = shallowRef<string[]>([])
const forceHorizontalSpine = ref(false)
const primaryCssDraft = ref('')

const chapters = shallowRef<ChapterRecord[]>([])
const activeIndex = ref(0)
const hasUnsavedEdits = ref(false)

const surfaceMode = ref<SurfaceMode>('wysiwyg')
const showLivePreview = ref(false)
const indentParagraphs = ref(true)
const activeTitle = ref('')
const activeCharCount = ref(0)
const sourceBuffer = ref('')
const previewSrcdoc = ref('')

const surfacePaneRef = ref<InstanceType<typeof BookEditorSurfacePane> | null>(null)
const wysiwygRef = computed<HTMLDivElement | null>({
  get: () => surfacePaneRef.value?.wysiwygEl ?? null,
  set: () => {},
})
const sourceTextareaRef = computed<HTMLTextAreaElement | null>({
  get: () => surfacePaneRef.value?.sourceTextareaEl ?? null,
  set: () => {},
})
const findBarRef = ref<InstanceType<typeof BookEditorFindReplaceBar> | null>(null)
const epubImageInputRef = ref<HTMLInputElement | null>(null)

const tocResetKey = ref(0)
const deleteSourceTxt = ref(false)
const proofreadPanelOpen = ref(false)
const cssPanelOpen = ref(false)
const transformScope = ref<'book' | 'chapter'>('book')

function showToast(msg: string): void {
  snackbar.show(msg)
}

function epubAssetContext() {
  const entry = props.entry
  return entry && docFormat.value === 'epub'
    ? { rootId: entry.root_id, relPath: entry.rel_path }
    : null
}

function buildLivePreviewSrcdoc(xhtmlBody: string, sourceHref: string | null): string {
  return buildPreviewDoc({
    xhtmlBody,
    sourceHref,
    epubAssetContext: epubAssetContext(),
    stylesheets: docStylesheets.value,
    indentParagraphs: indentParagraphs.value,
  })
}

const totalCharCount = computed(() =>
  chapters.value.reduce((acc, ch, idx) => {
    if (idx === activeIndex.value) return acc + activeCharCount.value
    return acc + ch.charCount
  }, 0),
)

const currentRecord = computed<ChapterRecord | null>(
  () => chapters.value[activeIndex.value] ?? null,
)

const {
  clearIdleSyncTimer,
  flushActiveSurfaceToRecord,
  mountActiveChapterIntoSurface,
  onWysiwygFocus,
  onWysiwygCompositionStart,
  onWysiwygInput,
  onWysiwygCompositionEnd,
  onSourceTextareaInput,
  onSourceKeydown,
  onActiveTitleInput,
  selectChapter,
  switchSurfaceMode,
  toggleLivePreview,
  execWysiwygCmd,
  formatBlockTag,
  wrapInlineCode,
  insertHorizontalRule,
} = useBookEditorSurface({
  chapters,
  activeIndex,
  currentRecord,
  hasUnsavedEdits,
  surfaceMode,
  showLivePreview,
  indentParagraphs,
  activeTitle,
  activeCharCount,
  sourceBuffer,
  previewSrcdoc,
  wysiwygRef,
  sourceTextareaRef,
  epubAssetContext,
  buildLivePreviewSrcdoc,
  themeDeps: () => [
    indentParagraphs.value,
    themeStore.isDark,
    themeStore.effectiveSeedColor,
    themeStore.uiFont,
    uploadedFonts.value,
  ],
})

const {
  mapDtoList,
  cleanChapterTypography,
  splitChapterAtCursor,
  handleRenameChapter,
  toggleLevel,
  autoDetectTocHierarchy,
  flattenAllTocLevels,
  syncTocTitlesFromBodyHeadings,
  insertVolumeBelow,
  insertChapterBelow,
  mergeWithPrevious,
  deleteChapterAt,
  handleReorderChapter,
} = useBookEditorChapters({
  chapters,
  activeIndex,
  activeTitle,
  hasUnsavedEdits,
  surfaceMode,
  sourceBuffer,
  wysiwygRef,
  sourceTextareaRef,
  currentRecord,
  flushActiveSurfaceToRecord,
  mountActiveChapterIntoSurface,
  clearIdleSyncTimer,
  confirmDelete: (title) =>
    dialogStore.confirm({
      title: t('tool.bookEditor.modal.deleteChapterTitle'),
      message: t('tool.bookEditor.modal.deleteChapterMsg', { title }),
      danger: true,
    }),
  showToast,
})

const {
  findBarOpen,
  findQuery,
  replaceQuery,
  findScope,
  findUseRegex,
  findCaseSensitive,
  searchMatchSummary,
  handleReplaceSingle,
  handleReplaceAll,
} = useBookEditorFindReplace({
  chapters,
  activeIndex,
  currentRecord,
  surfaceMode,
  sourceBuffer,
  hasUnsavedEdits,
  flushActiveSurfaceToRecord,
  mountActiveChapterIntoSurface,
  showToast,
})

const {
  loading,
  saving,
  converting,
  resegmenting,
  transforming,
  polishing,
  uploadingImage,
  savedBadge,
  applyCssPreset,
  syncPrimaryCssDraftToStylesheets,
  runBackendTextTransform,
  onEpubImagePicked,
  handlePolishInEditor,
  handleResegmentTxt,
  handleSaveInPlace,
  handleConvertToEpub,
} = useBookEditorDocument({
  open: toRef(props, 'open'),
  entry: toRef(props, 'entry'),
  docFormat,
  docTitle,
  docAuthors,
  docStylesheets,
  forceHorizontalSpine,
  primaryCssDraft,
  chapters,
  activeIndex,
  activeTitle,
  hasUnsavedEdits,
  surfaceMode,
  showLivePreview,
  sourceBuffer,
  previewSrcdoc,
  wysiwygRef,
  sourceTextareaRef,
  currentRecord,
  tocResetKey,
  deleteSourceTxt,
  transformScope,
  errorMsg,
  mapDtoList,
  flushActiveSurfaceToRecord,
  mountActiveChapterIntoSurface,
  buildLivePreviewSrcdoc,
  confirmResegment: () =>
    dialogStore.confirm({
      title: t('tool.bookEditor.modal.resegmentTitle'),
      message: t('tool.bookEditor.modal.resegmentMsg'),
      danger: true,
    }),
  showToast,
  onSaved: (detail) => emit('saved', detail),
  onConverted: (detail, deleted) => emit('converted', detail, deleted),
})

function triggerEpubImageUpload(): void {
  if (docFormat.value !== 'epub' || uploadingImage.value) return
  epubImageInputRef.value?.click()
}

async function requestClose(): Promise<void> {
  if (hasUnsavedEdits.value) {
    const ok = await dialogStore.confirm({
      title: t('tool.bookEditor.modal.discardTitle'),
      message: t('tool.bookEditor.modal.discardMsg'),
      danger: true,
    })
    if (!ok) return
  }
  emit('close')
}

function handleWorkbenchKeydown(ev: KeyboardEvent): void {
  if (!props.open || dialogStore.active) return
  const mod = ev.metaKey || ev.ctrlKey
  const key = ev.key.toLowerCase()

  if (mod && key === 's') {
    ev.preventDefault()
    void handleSaveInPlace()
  } else if (mod && key === 'f') {
    ev.preventDefault()
    findBarOpen.value = true
    void nextTick(() => findBarRef.value?.focusFind())
  } else if (mod && ev.shiftKey && ev.key === 'Enter') {
    ev.preventDefault()
    void splitChapterAtCursor()
  } else if (ev.altKey && ev.key === 'ArrowUp') {
    ev.preventDefault()
    if (activeIndex.value > 0) void selectChapter(activeIndex.value - 1)
  } else if (ev.altKey && ev.key === 'ArrowDown') {
    ev.preventDefault()
    if (activeIndex.value < chapters.value.length - 1) void selectChapter(activeIndex.value + 1)
  }
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) window.addEventListener('keydown', handleWorkbenchKeydown)
    else window.removeEventListener('keydown', handleWorkbenchKeydown)
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleWorkbenchKeydown)
  clearIdleSyncTimer()
})
</script>

<template>
  <MdDialog
    :open="open && Boolean(entry)"
    fullscreen
    hide-header
    no-body-padding
    :close-on-backdrop="false"
    @close="requestClose"
  >
    <div
      v-if="entry"
      class="flex h-full min-h-0 flex-col overflow-hidden bg-surface text-on-surface"
    >
      <BookEditorHeader
        :title="docTitle || entry.display_title"
        :doc-format="docFormat"
        :chapter-count="chapters.length"
        :total-char-count="totalCharCount"
        :saved-badge="savedBadge"
        :has-unsaved-edits="hasUnsavedEdits"
        :saving="saving"
        :converting="converting"
        :polishing="polishing"
        :delete-source-txt="deleteSourceTxt"
        @update:delete-source-txt="deleteSourceTxt = $event"
        @save="handleSaveInPlace"
        @convert-to-epub="handleConvertToEpub"
        @polish="handlePolishInEditor"
        @close="requestClose"
      />

      <input
        ref="epubImageInputRef"
        type="file"
        accept="image/*"
        class="hidden"
        @change="onEpubImagePicked"
      />

      <div
        v-if="errorMsg"
        class="text-body-small flex shrink-0 items-center justify-between gap-2 bg-error-container px-4 py-1.5 text-on-error-container"
      >
        <span>{{ errorMsg }}</span>
        <MdIconButton size="sm" :aria-label="t('tool.actions.close')" @click="errorMsg = null">
          <LuX class="h-4 w-4" />
        </MdIconButton>
      </div>

      <div
        v-if="loading"
        class="text-body-small flex flex-1 flex-col items-center justify-center gap-3 text-on-surface-variant"
      >
        <LuLoader2 class="h-6 w-6 animate-spin text-primary" />
        <span>{{ t('tool.bookEditor.modal.loadingBook') }}</span>
      </div>

      <div v-else class="relative flex min-h-0 flex-1 overflow-hidden">
        <BookEditorTocSidebar
          :chapters="chapters"
          :active-index="activeIndex"
          :active-char-count="activeCharCount"
          :doc-format="docFormat"
          :resegmenting="resegmenting"
          :reset-key="tocResetKey"
          @select="selectChapter"
          @insert-below="insertChapterBelow"
          @insert-volume-below="insertVolumeBelow"
          @toggle-level="toggleLevel"
          @rename="handleRenameChapter"
          @merge-prev="mergeWithPrevious"
          @delete="deleteChapterAt"
          @reorder="handleReorderChapter"
          @auto-detect-hierarchy="autoDetectTocHierarchy"
          @flatten-levels="flattenAllTocLevels"
          @sync-titles-from-body="syncTocTitlesFromBodyHeadings"
          @resegment-txt="handleResegmentTxt"
        />

        <div class="flex min-w-0 flex-1 flex-col bg-surface">
          <BookEditorToolbar
            :surface-mode="surfaceMode"
            :doc-format="docFormat"
            :uploading-image="uploadingImage"
            :proofread-panel-open="proofreadPanelOpen"
            :css-panel-open="cssPanelOpen"
            :find-bar-open="findBarOpen"
            :indent-paragraphs="indentParagraphs"
            :show-live-preview="showLivePreview"
            @exec-cmd="execWysiwygCmd"
            @format-block="formatBlockTag"
            @wrap-inline-code="wrapInlineCode"
            @insert-hr="insertHorizontalRule"
            @split-chapter="splitChapterAtCursor"
            @clean-typography="cleanChapterTypography"
            @upload-image="triggerEpubImageUpload"
            @toggle-proofread="
              proofreadPanelOpen = !proofreadPanelOpen;
              if (proofreadPanelOpen) cssPanelOpen = false
            "
            @toggle-css="
              cssPanelOpen = !cssPanelOpen;
              if (cssPanelOpen) proofreadPanelOpen = false
            "
            @toggle-find="findBarOpen = !findBarOpen"
            @toggle-indent="indentParagraphs = !indentParagraphs"
            @switch-surface-mode="switchSurfaceMode"
            @toggle-live-preview="toggleLivePreview"
          />

          <BookEditorProofreadPanel
            v-if="proofreadPanelOpen"
            :transform-scope="transformScope"
            :transforming="transforming"
            @update:transform-scope="transformScope = $event"
            @run-transform="(opts, label) => runBackendTextTransform(opts, label)"
            @close="proofreadPanelOpen = false"
          />

          <BookEditorCssPanel
            v-if="cssPanelOpen"
            :model-value="primaryCssDraft"
            @update:model-value="
              primaryCssDraft = $event;
              syncPrimaryCssDraftToStylesheets()
            "
            @apply-preset="(css, label) => applyCssPreset(css, label)"
            @close="cssPanelOpen = false"
          />

          <BookEditorFindReplaceBar
            v-if="findBarOpen"
            ref="findBarRef"
            :find-query="findQuery"
            :replace-query="replaceQuery"
            :find-scope="findScope"
            :find-case-sensitive="findCaseSensitive"
            :find-use-regex="findUseRegex"
            :search-match-summary="searchMatchSummary"
            @update:find-query="findQuery = $event"
            @update:replace-query="replaceQuery = $event"
            @update:find-scope="findScope = $event"
            @update:find-case-sensitive="findCaseSensitive = $event"
            @update:find-use-regex="findUseRegex = $event"
            @replace-single="handleReplaceSingle"
            @replace-all="handleReplaceAll"
            @close="findBarOpen = false"
          />

          <BookEditorSurfacePane
            ref="surfacePaneRef"
            :active-index="activeIndex"
            :active-title="activeTitle"
            :active-char-count="activeCharCount"
            :surface-mode="surfaceMode"
            :indent-paragraphs="indentParagraphs"
            :source-buffer="sourceBuffer"
            @update:active-title="onActiveTitleInput"
            @update:source-buffer="sourceBuffer = $event"
            @wysiwyg-focus="onWysiwygFocus"
            @wysiwyg-input="onWysiwygInput"
            @wysiwyg-composition-start="onWysiwygCompositionStart"
            @wysiwyg-composition-end="onWysiwygCompositionEnd"
            @source-input="onSourceTextareaInput"
            @source-keydown="onSourceKeydown"
          />
        </div>

        <aside
          v-if="showLivePreview"
          class="hidden w-[38%] max-w-xl shrink-0 flex-col bg-surface-container-low lg:flex"
        >
          <div
            class="flex h-10 shrink-0 items-center justify-between px-4"
          >
            <span class="text-label-large font-semibold text-on-surface">
              {{ t('tool.bookEditor.modal.livePreviewTitle') }}
            </span>
            <span class="text-label-small text-on-surface-variant">
              {{ t('tool.bookEditor.modal.livePreviewSub') }}
            </span>
          </div>
          <iframe
            :srcdoc="previewSrcdoc"
            :title="t('tool.bookEditor.modal.livePreviewTitle')"
            sandbox="allow-same-origin"
            class="min-h-0 w-full flex-1 border-0 bg-surface"
          />
        </aside>
      </div>
    </div>
  </MdDialog>
</template>
