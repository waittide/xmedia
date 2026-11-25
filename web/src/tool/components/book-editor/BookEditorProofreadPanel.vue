<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuSparkles, LuX } from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
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

const { t } = useI18n()

const scopeOptions = computed(
  () =>
    [
      { value: 'book', label: t('tool.bookEditor.proofread.scopeBook') },
      { value: 'chapter', label: t('tool.bookEditor.proofread.scopeChapter') },
    ] as const,
)

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
    t('tool.bookEditor.proofread.runClean'),
  )
}
</script>

<template>
  <div
    class="space-y-2.5 bg-surface-container px-4 py-3"
  >
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="flex flex-wrap items-center gap-1.5">
        <MdSegmentedButton
          :model-value="transformScope"
          :options="scopeOptions"
          size="sm"
          @update:model-value="emit('update:transformScope', $event)"
        />

        <MdButton
          variant="outlined"
          :disabled="transforming"
          @click="emit('runTransform', { zh_direction: 't2s' }, t('tool.bookEditor.proofread.t2sBtn'))"
        >
          {{ t('tool.bookEditor.proofread.t2sBtn') }}
        </MdButton>
        <MdButton
          variant="outlined"
          :disabled="transforming"
          @click="emit('runTransform', { zh_direction: 's2t' }, t('tool.bookEditor.proofread.s2tBtn'))"
        >
          {{ t('tool.bookEditor.proofread.s2tBtn') }}
        </MdButton>
        <MdButton
          variant="outlined"
          :disabled="transforming"
          @click="emit('runTransform', { vertical_to_horizontal: true, convert_quotes_to_horizontal: true }, t('tool.bookEditor.proofread.v2hBtn'))"
        >
          {{ t('tool.bookEditor.proofread.v2hBtn') }}
        </MdButton>
      </div>

      <MdIconButton
        size="sm"
        :aria-label="t('tool.actions.close')"
        @click="emit('close')"
      >
        <LuX class="h-4 w-4" />
      </MdIconButton>
    </div>

    <div class="flex flex-wrap items-center justify-between gap-2 rounded-corner-md bg-surface-container-low px-3 py-2">
      <div class="flex flex-wrap items-center gap-3">
        <AppCheckbox v-model="cleanWebnovelAds" :label="t('tool.bookEditor.proofread.cleanAds')" />
        <AppCheckbox v-model="normalizePunctuation" :label="t('tool.bookEditor.proofread.normalizePunct')" />
        <AppCheckbox v-model="convertCornerQuotes" :label="t('tool.bookEditor.proofread.cornerQuotes')" />
        <MdButton
          variant="text"
          @click="customStripRulesOpen = !customStripRulesOpen"
        >
          {{
            customStripRulesOpen
              ? t('tool.bookEditor.proofread.hideCustomRegex')
              : t('tool.bookEditor.proofread.showCustomRegex')
          }}
        </MdButton>
      </div>

      <MdButton
        variant="filled"
        :disabled="transforming"
        @click="handleRunClean"
      >
        <LuSparkles class="h-3.5 w-3.5" />
        <span>
          {{
            transforming
              ? t('tool.bookEditor.proofread.processing')
              : t('tool.bookEditor.proofread.runClean')
          }}
        </span>
      </MdButton>
    </div>

    <div v-if="customStripRulesOpen" class="pt-1">
      <MdTextField
        v-model="customStripRulesText"
        multiline
        :rows="3"
        :label="t('tool.bookEditor.proofread.customRegexLabel')"
      />
    </div>
  </div>
</template>
