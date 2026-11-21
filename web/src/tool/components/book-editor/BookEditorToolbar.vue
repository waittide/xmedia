<script setup lang="ts">
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
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import type { SurfaceMode } from './useBookEditorChapters'

const surfaceModeOptions = [
  { value: 'wysiwyg', label: '所见即所得' },
  { value: 'xhtml', label: 'XHTML 源码' },
] as const

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
</script>

<template>
  <div
    class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] px-3 py-2 select-none"
  >
    <!-- Left Formatting Buttons (`@mousedown.prevent` keeps caret & selection intact!) -->
    <div class="flex flex-wrap items-center gap-0.5">
      <template v-if="surfaceMode === 'wysiwyg'">
        <button
          type="button"
          class="tb-btn"
          title="撤销 (Ctrl+Z)"
          @mousedown.prevent="emit('execCmd', 'undo')"
        >
          <LuUndo2 class="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          class="tb-btn"
          title="重做 (Ctrl+Y)"
          @mousedown.prevent="emit('execCmd', 'redo')"
        >
          <LuRedo2 class="h-3.5 w-3.5" />
        </button>

        <span class="mx-1 h-4 w-px bg-[var(--md-sys-color-outline-variant)]" />
      </template>

      <button
        type="button"
        class="tb-btn"
        title="正文段落 (<p>)"
        @mousedown.prevent="emit('formatBlock', 'P')"
      >
        <LuPilcrow class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        class="tb-btn"
        title="一级标题 (<h1>)"
        @mousedown.prevent="emit('formatBlock', 'H1')"
      >
        <LuHeading1 class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        class="tb-btn"
        title="二级章节标题 (<h2>)"
        @mousedown.prevent="emit('formatBlock', 'H2')"
      >
        <LuHeading2 class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        class="tb-btn"
        title="三级小节标题 (<h3>)"
        @mousedown.prevent="emit('formatBlock', 'H3')"
      >
        <LuHeading3 class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        class="tb-btn"
        title="引用块 (<blockquote>)"
        @mousedown.prevent="emit('formatBlock', 'BLOCKQUOTE')"
      >
        <LuQuote class="h-3.5 w-3.5" />
      </button>

      <span class="mx-1 h-4 w-px bg-[var(--md-sys-color-outline-variant)]" />

      <template v-if="surfaceMode === 'wysiwyg'">
        <button
          type="button"
          class="tb-btn"
          title="加粗 (Ctrl+B)"
          @mousedown.prevent="emit('execCmd', 'bold')"
        >
          <LuBold class="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          class="tb-btn"
          title="斜体 (Ctrl+I)"
          @mousedown.prevent="emit('execCmd', 'italic')"
        >
          <LuItalic class="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          class="tb-btn"
          title="下划线 (Ctrl+U)"
          @mousedown.prevent="emit('execCmd', 'underline')"
        >
          <LuUnderline class="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          class="tb-btn"
          title="清除选中文字格式"
          @mousedown.prevent="emit('execCmd', 'removeFormat')"
        >
          <LuEraser class="h-3.5 w-3.5" />
        </button>
      </template>

      <button
        type="button"
        class="tb-btn"
        title="行内代码 (<code>)"
        @mousedown.prevent="emit('wrapInlineCode')"
      >
        <LuCode class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        class="tb-btn"
        title="插入水平分割线 (<hr/>)"
        @mousedown.prevent="emit('insertHr')"
      >
        <LuMinus class="h-3.5 w-3.5" />
      </button>

      <span class="mx-1 h-4 w-px bg-[var(--md-sys-color-outline-variant)]" />

      <button
        type="button"
        class="md-state inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]"
        title="在当前光标位置拆分为上下两章 (Ctrl+Shift+Enter)"
        @mousedown.prevent="emit('splitChapter')"
      >
        <LuScissors class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
        <span>光标拆章</span>
      </button>

      <button
        type="button"
        class="md-state inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]"
        title="自动清理本章空段落与段首全角/半角冗余空格"
        @click="emit('cleanTypography')"
      >
        <LuSparkles class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
        <span>一键净版</span>
      </button>

      <button
        v-if="docFormat === 'epub'"
        type="button"
        :disabled="uploadingImage"
        class="md-state inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)] disabled:opacity-50"
        title="上传图片写入 EPUB 资源包并插入当前光标处"
        @mousedown.prevent="emit('uploadImage')"
      >
        <LuImage class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
        <span>{{ uploadingImage ? '上传中…' : '插入图片' }}</span>
      </button>

      <button
        type="button"
        class="md-state inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium transition"
        :class="
          proofreadPanelOpen
            ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
            : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]'
        "
        title="简繁转换、台版竖排转横排、网文求票净化与中文标点智能规整"
        @click="emit('toggleProofread')"
      >
        <LuLanguages class="h-3.5 w-3.5" />
        <span>简繁与精校</span>
      </button>

      <button
        type="button"
        class="md-state inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium transition"
        :class="
          cssPanelOpen
            ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
            : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]'
        "
        title="切换中文排版 CSS 预设与编辑全局样式表"
        @click="emit('toggleCss')"
      >
        <LuPalette class="h-3.5 w-3.5" />
        <span>排版样式</span>
      </button>

      <button
        type="button"
        class="md-state inline-flex h-7 items-center gap-1 rounded-lg px-2 text-[11px] font-medium transition"
        :class="
          findBarOpen
            ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
            : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]'
        "
        title="查找与批量替换 (Ctrl+F)"
        @click="emit('toggleFind')"
      >
        <LuSearch class="h-3.5 w-3.5" />
        <span>查找替换</span>
      </button>
    </div>

    <!-- Right Mode Switcher + Live Preview Split Toggle -->
    <div class="flex items-center gap-1.5">
      <button
        v-if="surfaceMode === 'wysiwyg'"
        type="button"
        class="md-state h-8 rounded-full border px-2.5 text-[11px] font-medium transition"
        :class="
          indentParagraphs
            ? 'border-[var(--md-sys-color-primary)]/40 bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
            : 'border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-lowest)] text-[var(--md-sys-color-on-surface-variant)]'
        "
        title="切换段落首行缩进两字符显示"
        @click="emit('toggleIndent')"
      >
        首行缩进
      </button>

      <MdSegmentedButton
        :model-value="surfaceMode"
        :options="surfaceModeOptions"
        size="sm"
        @update:model-value="emit('switchSurfaceMode', $event)"
      />

      <button
        type="button"
        class="md-state inline-flex h-8 items-center gap-1 rounded-full border px-3 text-[11px] font-semibold transition"
        :class="
          showLivePreview
            ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
            : 'border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-lowest)] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
        "
        title="开启/关闭右侧阅读器真实样式对照预览窗"
        @click="emit('toggleLivePreview')"
      >
        <LuEye v-if="showLivePreview" class="h-3.5 w-3.5" />
        <LuEyeOff v-else class="h-3.5 w-3.5" />
        <span class="hidden sm:inline">预览分屏</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
@reference "tailwindcss";

.tb-btn {
  @apply flex h-7 w-7 items-center justify-center rounded-lg text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)];
}
</style>
