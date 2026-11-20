<script setup lang="ts">
import { ref, watch } from 'vue'
import { LuSparkles, LuX } from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import type { BookTextTransformOptions } from '../../types'

const CUSTOM_RULES_STORAGE_KEY = 'xmedia.book.customStripPatterns.v1'

defineProps<{
  transformScope: 'book' | 'chapter'
  transforming: boolean
}>()

const emit = defineEmits<{
  'update:transformScope': [scope: 'book' | 'chapter']
  runTransform: [options: BookTextTransformOptions, label: string]
  close: []
}>()

const scopeOptions = [
  { value: 'book', label: '全书范围' },
  { value: 'chapter', label: '仅当前章' },
] as const

const cleanWebnovelAds = ref(true)
const normalizePunctuation = ref(true)
const convertCornerQuotes = ref(false)
const customStripRulesOpen = ref(false)
const customStripRulesText = ref(
  typeof window !== 'undefined' ? window.localStorage.getItem(CUSTOM_RULES_STORAGE_KEY) ?? '' : '',
)

watch(customStripRulesText, (val) => {
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(CUSTOM_RULES_STORAGE_KEY, val)
  }
})

function handleRunClean(): void {
  const customPatterns = customStripRulesText.value
    .split('\n')
    .map((s) => s.trim())
    .filter(Boolean)
  emit(
    'runTransform',
    {
      clean_webnovel_ads: cleanWebnovelAds.value,
      normalize_punctuation: normalizePunctuation.value,
      convert_quotes_to_horizontal: convertCornerQuotes.value,
      custom_strip_patterns: customPatterns,
    },
    '精校净化',
  )
}
</script>

<template>
  <div
    class="space-y-2 border-b border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] px-4 py-2.5 text-xs"
  >
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex flex-wrap items-center gap-1.5">
        <MdSegmentedButton
          :model-value="transformScope"
          :options="scopeOptions"
          size="sm"
          @update:model-value="emit('update:transformScope', $event)"
        />

        <button
          type="button"
          :disabled="transforming"
          class="md-btn md-btn-sm md-btn-outlined md-state disabled:opacity-50"
          title="OpenCC 级繁转简（含台港词汇转换，保护 XHTML 标签）"
          @click="emit('runTransform', { zh_direction: 't2s' }, '繁转简')"
        >
          繁 → 简 (台港词库)
        </button>
        <button
          type="button"
          :disabled="transforming"
          class="md-btn md-btn-sm md-btn-outlined md-state disabled:opacity-50"
          title="简转繁（内置多音字词组消歧，避免后/发/干/里误转）"
          @click="emit('runTransform', { zh_direction: 's2t' }, '简转繁')"
        >
          简 → 繁 (词组消歧)
        </button>
        <button
          type="button"
          :disabled="transforming"
          class="md-btn md-btn-sm md-btn-outlined md-state disabled:opacity-50"
          title="将台版/日版竖排 CSS 改为横排、直角引号「」改为弯引号，并在保存时同步 LTR 翻页方向"
          @click="emit('runTransform', { vertical_to_horizontal: true, convert_quotes_to_horizontal: true }, '竖排转横排')"
        >
          竖排 → 横排 (CSS + 引号)
        </button>
      </div>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-on-surface-variant)]"
        aria-label="关闭精校面板"
        @click="emit('close')"
      >
        <LuX class="h-3.5 w-3.5" />
      </button>
    </div>

    <div class="flex flex-wrap items-center justify-between gap-2 border-t border-[var(--md-sys-color-outline-variant)]/60 pt-2">
      <div class="flex flex-wrap items-center gap-3">
        <AppCheckbox v-model="cleanWebnovelAds" label="去网文求票/域名水印行" />
        <AppCheckbox v-model="normalizePunctuation" label="中文标点规整 (弯引号/省略号/破折号)" />
        <AppCheckbox v-model="convertCornerQuotes" label="直角引号「」→“”" />
        <button
          type="button"
          class="text-[11px] font-medium text-[var(--md-sys-color-primary)] hover:underline"
          @click="customStripRulesOpen = !customStripRulesOpen"
        >
          {{ customStripRulesOpen ? '收起自定义正则' : '自定义净化正则' }}
        </button>
      </div>

      <button
        type="button"
        :disabled="transforming"
        class="md-btn md-btn-sm md-btn-filled md-state disabled:opacity-50"
        @click="handleRunClean"
      >
        <LuSparkles class="h-3 w-3" />
        <span>{{ transforming ? '处理中…' : '执行精校净化' }}</span>
      </button>
    </div>

    <div v-if="customStripRulesOpen" class="pt-1">
      <textarea
        v-model="customStripRulesText"
        rows="3"
        placeholder="每行输入一条整行匹配剔除正则（自动保存在浏览器本地），例如：^.*本书由某某论坛整理.*$"
        class="md-input w-full p-2 font-mono text-[11px]"
      />
    </div>
  </div>
</template>
