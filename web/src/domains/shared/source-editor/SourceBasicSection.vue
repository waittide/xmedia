<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { MdTextField } from '@/components/ui'

withDefaults(
  defineProps<{
    sourceName: string
    sourceGroup: string
    sourceUrl: string
    weight: number | string
    sourceComment: string
    nameJsonKey?: string
    groupJsonKey?: string
    urlJsonKey?: string
    weightJsonKey?: string
    commentJsonKey?: string
  }>(),
  {
    nameJsonKey: 'sourceName',
    groupJsonKey: 'sourceGroup',
    urlJsonKey: 'sourceUrl',
    weightJsonKey: 'weight',
    commentJsonKey: 'sourceComment',
  },
)

const emit = defineEmits<{
  'update:sourceName': [value: string]
  'update:sourceGroup': [value: string]
  'update:sourceUrl': [value: string]
  'update:weight': [value: string]
  'update:sourceComment': [value: string]
}>()

const { t } = useI18n()
</script>

<template>
  <section class="flex flex-col gap-4 rounded-corner-lg bg-surface-container p-4 sm:p-5">
    <h4 class="text-title-small text-on-surface">
      {{ t('onlineSources.edit.basicSectionTitle') }}
    </h4>

    <div class="grid grid-cols-1 gap-3.5 sm:grid-cols-2">
      <MdTextField
        :model-value="sourceName"
        size="sm"
        :label="t('onlineSources.edit.nameLabel')"
        :supporting-text="nameJsonKey"
        :title="nameJsonKey"
        @update:model-value="emit('update:sourceName', $event)"
      />

      <MdTextField
        :model-value="sourceGroup"
        name="group_name"
        size="sm"
        :label="t('onlineSources.edit.groupLabel')"
        :aria-label="t('onlineSources.edit.groupLabel')"
        :placeholder="t('onlineSources.edit.groupPlaceholder')"
        :supporting-text="groupJsonKey"
        :title="groupJsonKey"
        @update:model-value="emit('update:sourceGroup', $event)"
      />

      <div class="sm:col-span-2">
        <MdTextField
          :model-value="sourceUrl"
          size="sm"
          :label="t('onlineSources.edit.urlLabel')"
          :placeholder="t('onlineSources.edit.urlPlaceholder')"
          :supporting-text="urlJsonKey"
          :title="urlJsonKey"
          class="font-mono"
          @update:model-value="emit('update:sourceUrl', $event)"
        />
      </div>

      <slot name="extra-fields" />

      <MdTextField
        :model-value="weight"
        type="number"
        size="sm"
        :label="t('onlineSources.edit.weightLabel')"
        :supporting-text="weightJsonKey"
        :title="weightJsonKey"
        class="font-mono"
        @update:model-value="emit('update:weight', $event)"
      />

      <div class="sm:col-span-2">
        <MdTextField
          :model-value="sourceComment"
          multiline
          :rows="2"
          size="sm"
          :label="t('onlineSources.edit.commentLabel')"
          :placeholder="t('onlineSources.edit.commentPlaceholder')"
          :supporting-text="commentJsonKey"
          :title="commentJsonKey"
          @update:model-value="emit('update:sourceComment', $event)"
        />
      </div>
    </div>

    <div class="flex flex-wrap items-center gap-x-6 gap-y-2.5 pt-1">
      <slot name="switches" />
    </div>
  </section>
</template>
