<script setup lang="ts">
import { ref } from 'vue'
import { LuReplace, LuReplaceAll, LuX } from 'vue-icons-plus/lu'

defineProps<{
  findQuery: string
  replaceQuery: string
  findScope: 'chapter' | 'book'
  findCaseSensitive: boolean
  findUseRegex: boolean
  searchMatchSummary: string
}>()

const emit = defineEmits<{
  'update:findQuery': [value: string]
  'update:replaceQuery': [value: string]
  'update:findScope': [scope: 'chapter' | 'book']
  'update:findCaseSensitive': [value: boolean]
  'update:findUseRegex': [value: boolean]
  replaceSingle: []
  replaceAll: []
  close: []
}>()

const findInputRef = ref<HTMLInputElement | null>(null)

function focusFind(): void {
  findInputRef.value?.focus()
}

defineExpose({ focusFind })
</script>

<template>
  <div
    class="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--ctp-surface0)]/80 bg-[var(--ctp-shell)] px-4 py-2 text-xs"
  >
    <input
      ref="findInputRef"
      :value="findQuery"
      type="text"
      placeholder="查找内容…"
      class="h-7 w-44 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
      @input="emit('update:findQuery', ($event.target as HTMLInputElement).value)"
    />
    <input
      :value="replaceQuery"
      type="text"
      placeholder="替换为…"
      class="h-7 w-44 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
      @input="emit('update:replaceQuery', ($event.target as HTMLInputElement).value)"
    />

    <div class="inline-flex rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-0.5">
      <button
        type="button"
        class="rounded px-2 py-0.5 text-[11px] font-semibold"
        :class="
          findScope === 'chapter'
            ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'text-[var(--ctp-subtext1)]'
        "
        @click="emit('update:findScope', 'chapter')"
      >
        当前章
      </button>
      <button
        type="button"
        class="rounded px-2 py-0.5 text-[11px] font-semibold"
        :class="
          findScope === 'book'
            ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'text-[var(--ctp-subtext1)]'
        "
        @click="emit('update:findScope', 'book')"
      >
        全书
      </button>
    </div>

    <button
      type="button"
      class="rounded-lg border px-2 py-0.5 font-mono text-[11px] font-bold"
      :class="
        findCaseSensitive
          ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
          : 'border-[var(--ctp-surface0)] text-[var(--ctp-subtext0)]'
      "
      title="区分大小写"
      @click="emit('update:findCaseSensitive', !findCaseSensitive)"
    >
      Aa
    </button>
    <button
      type="button"
      class="rounded-lg border px-2 py-0.5 font-mono text-[11px] font-bold"
      :class="
        findUseRegex
          ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
          : 'border-[var(--ctp-surface0)] text-[var(--ctp-subtext0)]'
      "
      title="使用正则表达式"
      @click="emit('update:findUseRegex', !findUseRegex)"
    >
      .*
    </button>

    <span v-if="findQuery" class="font-mono text-[11px] text-[var(--ctp-subtext0)]">
      {{ searchMatchSummary }}
    </span>

    <div class="ml-auto flex items-center gap-1.5">
      <button
        type="button"
        :disabled="!findQuery"
        class="inline-flex h-7 items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-[11px] font-medium text-[var(--ctp-text)] hover:border-[var(--ctp-accent)] disabled:opacity-45"
        @click="emit('replaceSingle')"
      >
        <LuReplace class="h-3 w-3" />
        <span>替换当前</span>
      </button>
      <button
        type="button"
        :disabled="!findQuery"
        class="inline-flex h-7 items-center gap-1 rounded-lg bg-[var(--ctp-accent)] px-2.5 text-[11px] font-semibold text-[var(--ctp-on-accent)] hover:opacity-95 disabled:opacity-45"
        @click="emit('replaceAll')"
      >
        <LuReplaceAll class="h-3 w-3" />
        <span>全部替换</span>
      </button>
      <button
        type="button"
        class="rounded p-1 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]"
        @click="emit('close')"
      >
        <LuX class="h-3.5 w-3.5" />
      </button>
    </div>
  </div>
</template>
