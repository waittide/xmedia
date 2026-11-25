<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { LuSave, LuSkipForward } from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'

defineProps<{
  organizeAfterSave: boolean
  canSaveMetadata: boolean
  saving: boolean
  hasNext: boolean
}>()

const emit = defineEmits<{
  'update:organizeAfterSave': [value: boolean]
  save: []
  saveAndNext: []
}>()

const { t } = useI18n()
</script>

<template>
  <div class="flex w-full items-center justify-between gap-2">
    <AppCheckbox
      :model-value="organizeAfterSave"
      :disabled="!canSaveMetadata || saving"
      :label="t('tool.inspector.saveAndArchive')"
      @update:model-value="emit('update:organizeAfterSave', $event)"
    />

    <div class="flex shrink-0 items-center gap-2">
      <MdIconButton
        v-if="hasNext"
        variant="tonal"
        :disabled="!canSaveMetadata || saving"
        :icon="LuSkipForward"
        :tooltip="t('tool.inspector.saveAndNext')"
        :aria-label="t('tool.inspector.saveAndNext')"
        @click="emit('saveAndNext')"
      />

      <MdButton
        variant="filled"
        :icon="LuSave"
        :loading="saving"
        :disabled="!canSaveMetadata"
        :title="t('tool.inspector.shortcutsHint')"
        @click="emit('save')"
      >
        {{ saving ? t('tool.actions.saving') : t('tool.actions.save') }}
      </MdButton>
    </div>
  </div>
</template>
