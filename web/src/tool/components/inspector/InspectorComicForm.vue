<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import MdSelect from '@/components/ui/MdSelect.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import type { ComicInspectorForm } from './types'

defineProps<{
  form: ComicInspectorForm
}>()

const { t } = useI18n()

const directionOptions = computed(() => [
  { value: 'rtl', label: t('tool.forms.comic.dirRtl') },
  { value: 'ltr', label: t('tool.forms.comic.dirLtr') },
  { value: 'webtoon', label: t('tool.forms.comic.dirWebtoon') },
])
</script>

<template>
  <div class="space-y-3">
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.series"
          size="sm"
          :label="t('tool.forms.comic.series')"
        />
      </div>
      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.seriesGroup"
          size="sm"
          :label="t('tool.forms.comic.seriesGroup')"
          :placeholder="t('tool.forms.comic.seriesGroupPlaceholder')"
        />
      </div>
      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.title"
          size="sm"
          :label="t('tool.forms.comic.title')"
        />
      </div>
      <div class="grid grid-cols-3 gap-2.5 sm:col-span-2">
        <MdTextField
          v-model="form.number"
          size="sm"
          :label="t('tool.forms.comic.number')"
        />
        <MdTextField
          v-model="form.volume"
          size="sm"
          type="number"
          :label="t('tool.forms.comic.volume')"
        />
        <MdTextField
          v-model="form.year"
          size="sm"
          type="number"
          :label="t('tool.forms.common.year')"
        />
      </div>
      <MdTextField
        v-model="form.writers"
        size="sm"
        :label="t('tool.forms.comic.writers')"
      />
      <MdTextField
        v-model="form.pencillers"
        size="sm"
        :label="t('tool.forms.comic.pencillers')"
      />
      <MdTextField
        v-model="form.publisher"
        size="sm"
        :label="t('tool.forms.common.publisher')"
      />
      <MdTextField
        v-model="form.language"
        size="sm"
        :label="t('tool.forms.common.language')"
        :placeholder="t('tool.forms.comic.languagePlaceholder')"
      />
      <div class="flex flex-col gap-1 sm:col-span-2">
        <span class="text-label-medium text-on-surface-variant">
          {{ t('tool.forms.comic.direction') }}
        </span>
        <MdSelect v-model="form.direction" :options="directionOptions" size="sm" />
      </div>
      <MdTextField
        v-model="form.genres"
        size="sm"
        :label="t('tool.forms.comic.genres')"
      />
      <MdTextField
        v-model="form.tags"
        size="sm"
        :label="t('tool.forms.comic.tags')"
      />
      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.summary"
          multiline
          :rows="5"
          :label="t('tool.forms.comic.summary')"
        />
      </div>
    </div>
  </div>
</template>
