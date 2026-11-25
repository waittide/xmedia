<script setup lang="ts">
import { ref, watch } from 'vue'
import { LuChevronDown } from 'vue-icons-plus/lu'
import RuleFieldsSection from '@/components/online-sources/RuleFieldsSection.vue'
import { MdTextField } from '@/components/ui'
import { countStageConfigured, type RuleStageItem } from './types'

const props = defineProps<{
  stages: RuleStageItem[]
}>()

const expandedStageIds = ref<Set<string>>(new Set())

watch(
  () => props.stages.map((s) => s.id).join(','),
  () => {
    const next = new Set<string>()
    const firstConfigured = props.stages.find(
      (s) => countStageConfigured(s.modelValue, s.urlValue) > 0,
    )
    if (firstConfigured) {
      next.add(firstConfigured.id)
    } else if (props.stages[0]) {
      next.add(props.stages[0].id)
    }
    expandedStageIds.value = next
  },
  { immediate: true },
)

function toggleStage(id: string): void {
  const next = new Set(expandedStageIds.value)
  if (next.has(id)) {
    next.delete(id)
  } else {
    next.add(id)
  }
  expandedStageIds.value = next
}

function stageCount(stage: RuleStageItem): number {
  return countStageConfigured(stage.modelValue, stage.urlValue)
}
</script>

<template>
  <div class="flex flex-col gap-3">
    <section
      v-for="stage in stages"
      :key="stage.id"
      class="overflow-hidden rounded-corner-lg bg-surface-container"
    >
      <button
        type="button"
        :aria-expanded="expandedStageIds.has(stage.id)"
        class="md-state flex w-full cursor-pointer items-center justify-between gap-3 px-4 py-3.5 text-left sm:px-5"
        @click="toggleStage(stage.id)"
      >
        <div class="flex min-w-0 flex-wrap items-center gap-2">
          <span class="text-title-small text-on-surface">
            {{ stage.title }}
          </span>
          <span
            class="text-label-small font-mono text-on-surface-variant"
            :title="stage.jsonKey"
          >
            {{ stage.jsonKey }}
          </span>
          <span
            v-if="stageCount(stage) > 0"
            class="text-label-small rounded-corner-full bg-secondary-container px-2 py-0.5 tabular-nums text-on-secondary-container"
          >
            {{ stageCount(stage) }}
          </span>
        </div>

        <LuChevronDown
          class="h-4 w-4 shrink-0 text-on-surface-variant transition-transform duration-200 ease-standard"
          :class="expandedStageIds.has(stage.id) ? 'rotate-180' : ''"
          aria-hidden="true"
        />
      </button>

      <div
        v-if="expandedStageIds.has(stage.id)"
        class="flex flex-col gap-3 px-4 pt-1 pb-4 sm:px-5"
      >
        <div
          v-if="stage.onUpdateUrl"
          class="rounded-corner-md bg-surface-container-low p-3.5"
        >
          <MdTextField
            :model-value="stage.urlValue ?? ''"
            multiline
            :rows="stage.urlRows ?? 3"
            size="sm"
            :label="stage.urlLabel"
            :placeholder="stage.urlPlaceholder"
            :supporting-text="stage.urlJsonKey"
            :title="stage.urlJsonKey"
            class="font-mono"
            @update:model-value="stage.onUpdateUrl($event)"
          />
        </div>

        <RuleFieldsSection
          :model-value="stage.modelValue"
          :title="stage.jsonKey"
          :fields="stage.fields"
          @update:model-value="stage.onUpdateRules($event)"
        />
      </div>
    </section>
  </div>
</template>
