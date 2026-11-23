<script setup lang="ts">
import {
  LuBookOpen,
  LuCheck,
  LuKeyboard,
  LuRefreshCw,
  LuSave,
  LuWand2,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'

const SHORTCUT_HELP_TEXT =
  '快捷键：Ctrl+S 原地保存 · Ctrl+F 查找替换 · Ctrl+Shift+Enter 光标拆章 · Alt+↑/↓ 切换章节 · Esc 退出'

defineProps<{
  title: string
  docFormat: 'txt' | 'epub'
  chapterCount: number
  totalCharCount: number
  savedBadge: boolean
  hasUnsavedEdits: boolean
  saving: boolean
  converting: boolean
  polishing: boolean
  deleteSourceTxt: boolean
}>()

const emit = defineEmits<{
  'update:deleteSourceTxt': [val: boolean]
  save: []
  convertToEpub: []
  polish: []
  close: []
}>()
</script>

<template>
  <header
    class="safe-pt safe-px flex min-h-14 shrink-0 items-center justify-between gap-3 border-b border-outline-variant bg-surface-container-low px-4 py-1.5 select-none"
  >
    <div class="flex min-w-0 items-center gap-2.5">
      <div
        class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuBookOpen class="h-4 w-4" />
      </div>
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          <h1 class="truncate text-sm font-bold text-[var(--md-sys-color-on-surface)]">
            {{ title }}
          </h1>
          <span
            class="shrink-0 rounded-md bg-[var(--md-sys-color-secondary-container)] px-1.5 py-0.5 font-mono text-[10px] font-bold uppercase text-[var(--md-sys-color-on-secondary-container)]"
          >
            {{ docFormat }}
          </span>
          <span
            v-if="savedBadge"
            class="inline-flex shrink-0 items-center gap-1 rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-success-container)]"
          >
            <LuCheck class="h-3 w-3" />
            <span>已保存</span>
          </span>
          <span
            v-else-if="hasUnsavedEdits"
            class="shrink-0 rounded-full bg-[var(--md-sys-color-warning-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-warning-container)]"
          >
            未保存
          </span>
        </div>
        <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
          {{ chapterCount }} 章 · 全书约 {{ totalCharCount.toLocaleString() }} 字
        </p>
      </div>
    </div>

    <!-- Right Save / Convert / Close Actions -->
    <div class="flex shrink-0 items-center gap-2">
      <div class="group relative">
        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-on-surface-variant)]"
          :title="SHORTCUT_HELP_TEXT"
          aria-label="编辑器快捷键说明"
        >
          <LuKeyboard class="h-4 w-4" />
        </button>
        <div
          class="pointer-events-none invisible absolute top-full right-0 z-20 mt-1.5 w-64 rounded-2xl border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-high)] p-3 text-[11px] text-[var(--md-sys-color-on-surface)] opacity-0 shadow-[var(--md-sys-elevation-2)] transition group-focus-within:visible group-focus-within:opacity-100 group-hover:visible group-hover:opacity-100"
        >
          <p class="mb-1.5 font-semibold text-[var(--md-sys-color-on-surface)]">快捷键一览</p>
          <ul class="space-y-1 text-[var(--md-sys-color-on-surface-variant)]">
            <li class="flex justify-between gap-2">
              <span>原地保存</span>
              <kbd class="font-mono text-[var(--md-sys-color-on-surface)]">Ctrl+S</kbd>
            </li>
            <li class="flex justify-between gap-2">
              <span>查找替换</span>
              <kbd class="font-mono text-[var(--md-sys-color-on-surface)]">Ctrl+F</kbd>
            </li>
            <li class="flex justify-between gap-2">
              <span>光标拆章</span>
              <kbd class="font-mono text-[var(--md-sys-color-on-surface)]">Ctrl+Shift+Enter</kbd>
            </li>
            <li class="flex justify-between gap-2">
              <span>上下章节</span>
              <kbd class="font-mono text-[var(--md-sys-color-on-surface)]">Alt+↑ / ↓</kbd>
            </li>
            <li class="flex justify-between gap-2">
              <span>退出工作台</span>
              <kbd class="font-mono text-[var(--md-sys-color-on-surface)]">Esc</kbd>
            </li>
          </ul>
        </div>
      </div>

      <template v-if="docFormat === 'txt'">
        <div class="hidden sm:block">
          <AppCheckbox
            :model-value="deleteSourceTxt"
            label="转后删原 TXT"
            @update:model-value="emit('update:deleteSourceTxt', $event)"
          />
        </div>

        <button
          type="button"
          :disabled="saving || converting || chapterCount === 0"
          class="md-btn md-btn-sm md-btn-outlined md-state disabled:opacity-50"
          title="保存修改回原 TXT 文件 (Ctrl+S)"
          @click="emit('save')"
        >
          <LuSave class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
          <span>{{ saving ? '保存中…' : '保存 TXT' }}</span>
        </button>

        <button
          type="button"
          :disabled="converting || saving || chapterCount === 0"
          class="md-btn md-btn-sm md-btn-filled md-state disabled:opacity-50"
          @click="emit('convertToEpub')"
        >
          <LuBookOpen class="h-3.5 w-3.5" />
          <span>{{ converting ? '打包中…' : '转为 EPUB' }}</span>
        </button>
      </template>

      <template v-else>
        <button
          type="button"
          :disabled="polishing || saving || chapterCount === 0"
          class="md-btn md-btn-sm md-btn-tonal md-state disabled:opacity-50"
          title="自动生成/刷新内封简介页、同步 EPUB2/3 双目录并清理未引用冗余资源"
          @click="emit('polish')"
        >
          <LuRefreshCw v-if="polishing" class="h-3.5 w-3.5 animate-spin" />
          <LuWand2 v-else class="h-3.5 w-3.5" />
          <span class="hidden sm:inline">{{ polishing ? '抛光中…' : '书籍抛光' }}</span>
        </button>

        <button
          type="button"
          :disabled="saving || polishing || chapterCount === 0"
          class="md-btn md-btn-sm md-btn-filled md-state disabled:opacity-50"
          title="原地保存目录与正文到 EPUB，不关闭编辑器 (Ctrl+S)"
          @click="emit('save')"
        >
          <LuSave class="h-3.5 w-3.5" />
          <span>{{ saving ? '保存中…' : '保存 (Ctrl+S)' }}</span>
        </button>
      </template>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-on-surface-variant)]"
        title="退出工作台"
        aria-label="退出工作台"
        @click="emit('close')"
      >
        <LuX class="h-4 w-4" />
      </button>
    </div>
  </header>
</template>
