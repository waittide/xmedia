<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuBold,
  LuCode,
  LuEraser,
  LuEye,
  LuEyeOff,
  LuHeading1,
  LuHeading2,
  LuHeading3,
  LuImage,
  LuItalic,
  LuLanguages,
  LuMinus,
  LuPalette,
  LuPilcrow,
  LuQuote,
  LuRedo2,
  LuScissors,
  LuSearch,
  LuSparkles,
  LuUnderline,
  LuUndo2,
} from 'vue-icons-plus/lu'
import MdChip from '@/components/ui/MdChip.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import type { SurfaceMode } from './useBookEditorChapters'

defineProps<{
  surfaceMode: SurfaceMode
  docFormat: 'txt' | 'epub'
  uploadingImage: boolean
  proofreadPanelOpen: boolean
  cssPanelOpen: boolean
  findBarOpen: boolean
  indentParagraphs: boolean
  showLivePreview: boolean
}>()

const emit = defineEmits<{
  execCmd: [command: string]
  formatBlock: [tag: 'P' | 'H1' | 'H2' | 'H3' | 'BLOCKQUOTE']
  wrapInlineCode: []
  insertHr: []
  splitChapter: []
  cleanTypography: []
  uploadImage: []
  toggleProofread: []
  toggleCss: []
  toggleFind: []
  toggleIndent: []
  switchSurfaceMode: [mode: SurfaceMode]
  toggleLivePreview: []
}>()

const { t } = useI18n()

const surfaceModeOptions = computed(
  () =>
    [
      { value: 'wysiwyg', label: t('tool.bookEditor.toolbar.wysiwyg') },
      { value: 'xhtml', label: t('tool.bookEditor.toolbar.xhtmlSource') },
    ] as const,
)
</script>

<template>
  <div
    class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-outline-variant/50 bg-surface-container-low px-3 py-2 select-none"
  >
    <!-- Left Formatting Buttons (`@mousedown.prevent` keeps caret & selection intact) -->
    <div class="flex flex-wrap items-center gap-0.5">
      <template v-if="surfaceMode === 'wysiwyg'">
        <button
          type="button"
          class="tb-btn"
          :title="t('tool.bookEditor.toolbar.undo')"
          :aria-label="t('tool.bookEditor.toolbar.undo')"
          @mousedown.prevent="emit('execCmd', 'undo')"
        >
          <LuUndo2 class="h-4 w-4" />
        </button>
        <button
          type="button"
          class="tb-btn"
          :title="t('tool.bookEditor.toolbar.redo')"
          :aria-label="t('tool.bookEditor.toolbar.redo')"
          @mousedown.prevent="emit('execCmd', 'redo')"
        >
          <LuRedo2 class="h-4 w-4" />
        </button>

        <span class="mx-1 h-4 w-px bg-outline-variant/60" />
      </template>

      <button
        type="button"
        class="tb-btn"
        :title="t('tool.bookEditor.toolbar.paragraph')"
        :aria-label="t('tool.bookEditor.toolbar.paragraph')"
        @mousedown.prevent="emit('formatBlock', 'P')"
      >
        <LuPilcrow class="h-4 w-4" />
      </button>
      <button
        type="button"
        class="tb-btn"
        :title="t('tool.bookEditor.toolbar.h1')"
        :aria-label="t('tool.bookEditor.toolbar.h1')"
        @mousedown.prevent="emit('formatBlock', 'H1')"
      >
        <LuHeading1 class="h-4 w-4" />
      </button>
      <button
        type="button"
        class="tb-btn"
        :title="t('tool.bookEditor.toolbar.h2')"
        :aria-label="t('tool.bookEditor.toolbar.h2')"
        @mousedown.prevent="emit('formatBlock', 'H2')"
      >
        <LuHeading2 class="h-4 w-4" />
      </button>
      <button
        type="button"
        class="tb-btn"
        :title="t('tool.bookEditor.toolbar.h3')"
        :aria-label="t('tool.bookEditor.toolbar.h3')"
        @mousedown.prevent="emit('formatBlock', 'H3')"
      >
        <LuHeading3 class="h-4 w-4" />
      </button>
      <button
        type="button"
        class="tb-btn"
        :title="t('tool.bookEditor.toolbar.blockquote')"
        :aria-label="t('tool.bookEditor.toolbar.blockquote')"
        @mousedown.prevent="emit('formatBlock', 'BLOCKQUOTE')"
      >
        <LuQuote class="h-4 w-4" />
      </button>

      <span class="mx-1 h-4 w-px bg-outline-variant/60" />

      <template v-if="surfaceMode === 'wysiwyg'">
        <button
          type="button"
          class="tb-btn"
          :title="t('tool.bookEditor.toolbar.bold')"
          :aria-label="t('tool.bookEditor.toolbar.bold')"
          @mousedown.prevent="emit('execCmd', 'bold')"
        >
          <LuBold class="h-4 w-4" />
        </button>
        <button
          type="button"
          class="tb-btn"
          :title="t('tool.bookEditor.toolbar.italic')"
          :aria-label="t('tool.bookEditor.toolbar.italic')"
          @mousedown.prevent="emit('execCmd', 'italic')"
        >
          <LuItalic class="h-4 w-4" />
        </button>
        <button
          type="button"
          class="tb-btn"
          :title="t('tool.bookEditor.toolbar.underline')"
          :aria-label="t('tool.bookEditor.toolbar.underline')"
          @mousedown.prevent="emit('execCmd', 'underline')"
        >
          <LuUnderline class="h-4 w-4" />
        </button>
        <button
          type="button"
          class="tb-btn"
          :title="t('tool.bookEditor.toolbar.clearFormat')"
          :aria-label="t('tool.bookEditor.toolbar.clearFormat')"
          @mousedown.prevent="emit('execCmd', 'removeFormat')"
        >
          <LuEraser class="h-4 w-4" />
        </button>
      </template>

      <button
        type="button"
        class="tb-btn"
        :title="t('tool.bookEditor.toolbar.inlineCode')"
        :aria-label="t('tool.bookEditor.toolbar.inlineCode')"
        @mousedown.prevent="emit('wrapInlineCode')"
      >
        <LuCode class="h-4 w-4" />
      </button>
      <button
        type="button"
        class="tb-btn"
        :title="t('tool.bookEditor.toolbar.hr')"
        :aria-label="t('tool.bookEditor.toolbar.hr')"
        @mousedown.prevent="emit('insertHr')"
      >
        <LuMinus class="h-4 w-4" />
      </button>

      <span class="mx-1 h-4 w-px bg-outline-variant/60" />

      <button
        type="button"
        class="md-state inline-flex h-8 items-center gap-1.5 rounded-corner-sm px-2.5 text-label-medium font-medium text-on-surface-variant transition hover:bg-surface-container-high hover:text-on-surface"
        :title="t('tool.bookEditor.toolbar.splitChapter')"
        @mousedown.prevent="emit('splitChapter')"
      >
        <LuScissors class="h-3.5 w-3.5 text-primary" />
        <span>{{ t('tool.bookEditor.toolbar.splitChapter') }}</span>
      </button>

      <button
        type="button"
        class="md-state inline-flex h-8 items-center gap-1.5 rounded-corner-sm px-2.5 text-label-medium font-medium text-on-surface-variant transition hover:bg-surface-container-high hover:text-on-surface"
        :title="t('tool.bookEditor.toolbar.cleanTypography')"
        @click="emit('cleanTypography')"
      >
        <LuSparkles class="h-3.5 w-3.5 text-primary" />
        <span>{{ t('tool.bookEditor.toolbar.cleanTypography') }}</span>
      </button>

      <button
        v-if="docFormat === 'epub'"
        type="button"
        :disabled="uploadingImage"
        class="md-state inline-flex h-8 items-center gap-1.5 rounded-corner-sm px-2.5 text-label-medium font-medium text-on-surface-variant transition hover:bg-surface-container-high hover:text-on-surface disabled:opacity-50"
        :title="t('tool.bookEditor.toolbar.insertImage')"
        @mousedown.prevent="emit('uploadImage')"
      >
        <LuImage class="h-3.5 w-3.5 text-primary" />
        <span>{{ uploadingImage ? t('tool.common.loading') : t('tool.bookEditor.toolbar.insertImage') }}</span>
      </button>

      <MdChip
        variant="filter"
        :selected="proofreadPanelOpen"
        :label="t('tool.bookEditor.toolbar.proofread')"
        @click="emit('toggleProofread')"
      >
        <template #icon>
          <LuLanguages class="h-3.5 w-3.5" />
        </template>
      </MdChip>

      <MdChip
        variant="filter"
        :selected="cssPanelOpen"
        :label="t('tool.bookEditor.toolbar.cssStyle')"
        @click="emit('toggleCss')"
      >
        <template #icon>
          <LuPalette class="h-3.5 w-3.5" />
        </template>
      </MdChip>

      <MdChip
        variant="filter"
        :selected="findBarOpen"
        :label="t('tool.bookEditor.toolbar.findReplace')"
        @click="emit('toggleFind')"
      >
        <template #icon>
          <LuSearch class="h-3.5 w-3.5" />
        </template>
      </MdChip>
    </div>

    <!-- Right Mode Switcher + Live Preview Split Toggle -->
    <div class="flex items-center gap-1.5">
      <MdChip
        v-if="surfaceMode === 'wysiwyg'"
        variant="filter"
        :selected="indentParagraphs"
        :label="t('tool.bookEditor.toolbar.firstLineIndent')"
        @click="emit('toggleIndent')"
      />

      <MdSegmentedButton
        :model-value="surfaceMode"
        :options="surfaceModeOptions"
        size="sm"
        @update:model-value="emit('switchSurfaceMode', $event)"
      />

      <MdChip
        variant="filter"
        :selected="showLivePreview"
        :label="t('tool.bookEditor.toolbar.livePreview')"
        @click="emit('toggleLivePreview')"
      >
        <template #icon>
          <LuEye v-if="showLivePreview" class="h-3.5 w-3.5" />
          <LuEyeOff v-else class="h-3.5 w-3.5" />
        </template>
      </MdChip>
    </div>
  </div>
</template>

<style scoped>
.tb-btn {
  position: relative;
  display: flex;
  height: 2rem;
  width: 2rem;
  align-items: center;
  justify-content: center;
  border-radius: var(--md-sys-shape-corner-small, 0.5rem);
  color: var(--md-sys-color-on-surface-variant);
  transition: background-color 150ms ease, color 150ms ease;
}

.tb-btn:hover {
  background-color: var(--md-sys-color-surface-container-high);
  color: var(--md-sys-color-on-surface);
}

@media (pointer: coarse) {
  .tb-btn::before {
    content: "";
    position: absolute;
    inset: -6px;
  }
}
</style>
