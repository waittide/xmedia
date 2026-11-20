<script setup lang="ts">
import { ref } from 'vue'
import { LuReplace, LuReplaceAll, LuX } from 'vue-icons-plus/lu'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'

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

const scopeOptions = [
  { value: 'chapter', label: '当前章' },
  { value: 'book', label: '全书' },
] as const

const findInputRef = ref<HTMLInputElement | null>(null)

function focusFind(): void {
  findInputRef.value?.focus()
}

defineExpose({ focusFind })
</script>

<template>
  <div
    class="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] px-4 py-2 text-xs"
  >
    <input
      ref="findInputRef"
      :value="findQuery"
      type="text"
      placeholder="查找内容…"
      class="md-input h-8 w-44 px-2.5 text-xs"
      @input="emit('update:findQuery', ($event.target as HTMLInputElement).value)"
    />
    <input
      :value="replaceQuery"
      type="text"
      placeholder="替换为…"
      class="md-input h-8 w-44 px-2.5 text-xs"
      @input="emit('update:replaceQuery', ($event.target as HTMLInputElement).value)"
    />

    <MdSegmentedButton
      :model-value="findScope"
      :options="scopeOptions"
      size="sm"
      @update:model-value="emit('update:findScope', $event)"
    />

    <button
      type="button"
      class="md-state h-8 rounded-lg border px-2 font-mono text-[11px] font-bold transition"
      :class="
        findCaseSensitive
          ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
          : 'border-[var(--md-sys-color-outline-variant)] text-[var(--md-sys-color-on-surface-variant)]'
      "
      title="区分大小写"
      @click="emit('update:findCaseSensitive', !findCaseSensitive)"
    >
      Aa
    </button>
    <button
      type="button"
      class="md-state h-8 rounded-lg border px-2 font-mono text-[11px] font-bold transition"
      :class="
        findUseRegex
          ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
          : 'border-[var(--md-sys-color-outline-variant)] text-[var(--md-sys-color-on-surface-variant)]'
      "
      title="使用正则表达式"
      @click="emit('update:findUseRegex', !findUseRegex)"
    >
      .*
    </button>

    <span v-if="findQuery" class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
      {{ searchMatchSummary }}
    </span>

    <div class="ml-auto flex items-center gap-1.5">
      <button
        type="button"
        :disabled="!findQuery"
        class="md-btn md-btn-sm md-btn-outlined md-state disabled:opacity-45"
        @click="emit('replaceSingle')"
      >
        <LuReplace class="h-3 w-3" />
        <span>替换当前</span>
      </button>
      <button
        type="button"
        :disabled="!findQuery"
        class="md-btn md-btn-sm md-btn-filled md-state disabled:opacity-45"
        @click="emit('replaceAll')"
      >
        <LuReplaceAll class="h-3 w-3" />
        <span>全部替换</span>
      </button>
      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-on-surface-variant)]"
        aria-label="关闭查找替换栏"
        @click="emit('close')"
      >
        <LuX class="h-3.5 w-3.5" />
      </button>
    </div>
  </div>
</template>
