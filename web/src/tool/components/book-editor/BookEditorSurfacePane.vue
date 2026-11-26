<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SurfaceMode } from './useBookEditorChapters'

defineProps<{
  activeIndex: number
  activeTitle: string
  activeCharCount: number
  surfaceMode: SurfaceMode
  indentParagraphs: boolean
  sourceBuffer: string
}>()

const emit = defineEmits<{
  'update:activeTitle': [value: string]
  'update:sourceBuffer': [value: string]
  wysiwygFocus: []
  wysiwygInput: []
  wysiwygCompositionStart: []
  wysiwygCompositionEnd: []
  sourceInput: []
  sourceKeydown: [ev: KeyboardEvent]
}>()

const { t } = useI18n()
const wysiwygEl = ref<HTMLDivElement | null>(null)
const sourceTextareaEl = ref<HTMLTextAreaElement | null>(null)

defineExpose({
  wysiwygEl,
  sourceTextareaEl,
})
</script>

<template>
  <div class="flex shrink-0 items-center gap-3 bg-surface-container-lowest px-6 py-2.5">
    <span class="text-label-small shrink-0 font-mono text-on-surface-variant">
      #{{ activeIndex + 1 }}
    </span>
    <input
      :value="activeTitle"
      type="text"
      :aria-label="t('tool.bookEditor.modal.chapterTitlePlaceholder')"
      :placeholder="t('tool.bookEditor.modal.chapterTitlePlaceholder')"
      class="text-title-medium min-w-0 flex-1 bg-transparent font-semibold text-on-surface outline-none placeholder:text-on-surface-variant/50"
      @input="emit('update:activeTitle', ($event.target as HTMLInputElement).value)"
    />
    <span class="text-label-small shrink-0 font-mono text-on-surface-variant">
      {{ t('tool.bookEditor.modal.chapterChars', { count: activeCharCount.toLocaleString() }) }}
    </span>
  </div>

  <div class="relative min-h-0 flex-1 overflow-y-auto">
    <div v-show="surfaceMode === 'wysiwyg'" class="mx-auto max-w-3xl px-6 py-6 sm:px-10">
      <div
        ref="wysiwygEl"
        contenteditable="true"
        spellcheck="false"
        class="epub-wysiwyg-surface min-h-[65vh] outline-none"
        :class="indentParagraphs ? 'indent-paras' : ''"
        @focus="emit('wysiwygFocus')"
        @input="emit('wysiwygInput')"
        @compositionstart="emit('wysiwygCompositionStart')"
        @compositionend="emit('wysiwygCompositionEnd')"
      />
    </div>

    <textarea
      v-show="surfaceMode === 'xhtml'"
      ref="sourceTextareaEl"
      :value="sourceBuffer"
      spellcheck="false"
      :aria-label="t('tool.bookEditor.modal.xhtmlPlaceholder')"
      :placeholder="t('tool.bookEditor.modal.xhtmlPlaceholder')"
      class="text-body-small h-full w-full resize-none bg-transparent p-5 font-mono leading-relaxed text-on-surface outline-none"
      @input="
        emit('update:sourceBuffer', ($event.target as HTMLTextAreaElement).value);
        emit('sourceInput')
      "
      @keydown="emit('sourceKeydown', $event)"
    />
  </div>
</template>

<style scoped>
@reference "tailwindcss";

.epub-wysiwyg-surface {
  font-family: "Noto Serif CJK SC", "Songti SC", "PingFang SC", "Microsoft YaHei", Georgia, serif;
  font-size: 16px;
  line-height: 1.9;
  color: var(--md-sys-color-on-surface);
  word-break: break-word;
}

.epub-wysiwyg-surface :deep(h1) {
  font-size: 1.6em;
  font-weight: 800;
  line-height: 1.35;
  margin: 0.6em 0 0.75em;
}

.epub-wysiwyg-surface :deep(h2) {
  font-size: 1.35em;
  font-weight: 700;
  line-height: 1.4;
  margin: 0.6em 0 0.7em;
}

.epub-wysiwyg-surface :deep(h3) {
  font-size: 1.15em;
  font-weight: 700;
  line-height: 1.45;
  margin: 0.6em 0 0.6em;
}

.epub-wysiwyg-surface :deep(p),
.epub-wysiwyg-surface :deep(div) {
  margin: 0 0 0.85em;
  text-align: justify;
}

.epub-wysiwyg-surface.indent-paras :deep(p) {
  text-indent: 2em;
}

.epub-wysiwyg-surface :deep(blockquote) {
  margin: 1em 0;
  padding: 0.5em 1em;
  border-left: 3px solid var(--md-sys-color-primary);
  background: var(--md-sys-color-surface-container-low);
  border-radius: 0 0.5rem 0.5rem 0;
}

.epub-wysiwyg-surface :deep(code) {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 0.88em;
  padding: 0.15em 0.4em;
  border-radius: 0.35rem;
  background: var(--md-sys-color-surface-container-high);
}

.epub-wysiwyg-surface :deep(hr) {
  border: none;
  border-top: 1px solid var(--md-sys-color-outline-variant);
  margin: 1.5em 0;
}

.epub-wysiwyg-surface :deep(img),
.epub-wysiwyg-surface :deep(svg) {
  max-width: 100%;
  height: auto;
  border-radius: 0.5rem;
  margin: 0.75em auto;
  display: block;
}
</style>
