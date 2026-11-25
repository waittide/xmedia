<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuReplace, LuReplaceAll, LuX } from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdChip from '@/components/ui/MdChip.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'

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

const { t } = useI18n()

const scopeOptions = computed(
  () =>
    [
      { value: 'chapter', label: t('tool.bookEditor.find.scopeChapter') },
      { value: 'book', label: t('tool.bookEditor.find.scopeBook') },
    ] as const,
)

const findFieldRef = ref<InstanceType<typeof MdTextField> | null>(null)

function focusFind(): void {
  findFieldRef.value?.focus()
}

defineExpose({ focusFind })
</script>

<template>
  <div
    class="flex shrink-0 flex-wrap items-center gap-2 border-b border-outline-variant/50 bg-surface-container-low px-4 py-2"
  >
    <div class="w-48">
      <MdTextField
        ref="findFieldRef"
        :model-value="findQuery"
        size="sm"
        :label="t('tool.bookEditor.find.findLabel')"
        @update:model-value="emit('update:findQuery', $event)"
      />
    </div>
    <div class="w-48">
      <MdTextField
        :model-value="replaceQuery"
        size="sm"
        :label="t('tool.bookEditor.find.replaceLabel')"
        @update:model-value="emit('update:replaceQuery', $event)"
      />
    </div>

    <MdSegmentedButton
      :model-value="findScope"
      :options="scopeOptions"
      size="sm"
      @update:model-value="emit('update:findScope', $event)"
    />

    <MdChip
      variant="filter"
      :selected="findCaseSensitive"
      label="Aa"
      :title="t('tool.bookEditor.find.caseSensitive')"
      @click="emit('update:findCaseSensitive', !findCaseSensitive)"
    />
    <MdChip
      variant="filter"
      :selected="findUseRegex"
      label=".*"
      :title="t('tool.bookEditor.find.useRegex')"
      @click="emit('update:findUseRegex', !findUseRegex)"
    />

    <span v-if="findQuery" class="font-mono text-label-small text-on-surface-variant">
      {{ searchMatchSummary }}
    </span>

    <div class="ml-auto flex items-center gap-1.5">
      <MdButton
        variant="outlined"
        size="sm"
        :disabled="!findQuery"
        @click="emit('replaceSingle')"
      >
        <LuReplace class="h-3.5 w-3.5" />
        <span>{{ t('tool.bookEditor.find.replaceCurrent') }}</span>
      </MdButton>
      <MdButton
        variant="filled"
        size="sm"
        :disabled="!findQuery"
        @click="emit('replaceAll')"
      >
        <LuReplaceAll class="h-3.5 w-3.5" />
        <span>{{ t('tool.bookEditor.find.replaceAll') }}</span>
      </MdButton>
      <MdIconButton
        size="sm"
        :aria-label="t('tool.bookEditor.find.closeAria')"
        @click="emit('close')"
      >
        <LuX class="h-4 w-4" />
      </MdIconButton>
    </div>
  </div>
</template>
