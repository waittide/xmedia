<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { MdTextField } from '@/components/ui'

withDefaults(
  defineProps<{
    concurrentRate: string
    header: string
    loginUrl: string
    loginUi: string
    loginCheckJs: string
    jsLib: string
    variableText: string
    headerJsonKey?: string
    secondaryHeader?: string
    secondaryHeaderLabel?: string
    secondaryHeaderJsonKey?: string
    hasSecondaryHeader?: boolean
    urlPattern?: string
    urlPatternLabel?: string
    urlPatternJsonKey?: string
    hasUrlPattern?: boolean
  }>(),
  {
    headerJsonKey: 'header',
    secondaryHeader: '',
    secondaryHeaderLabel: '',
    secondaryHeaderJsonKey: '',
    hasSecondaryHeader: false,
    urlPattern: '',
    urlPatternLabel: '',
    urlPatternJsonKey: '',
    hasUrlPattern: false,
  },
)

const emit = defineEmits<{
  'update:concurrentRate': [value: string]
  'update:header': [value: string]
  'update:secondaryHeader': [value: string]
  'update:urlPattern': [value: string]
  'update:loginUrl': [value: string]
  'update:loginUi': [value: string]
  'update:loginCheckJs': [value: string]
  'update:jsLib': [value: string]
  'update:variableText': [value: string]
}>()

const { t } = useI18n()
</script>

<template>
  <section class="flex flex-col gap-4 rounded-corner-lg bg-surface-container p-4 sm:p-5">
    <h4 class="text-title-small text-on-surface">
      {{ t('onlineSources.edit.networkSectionTitle') }}
    </h4>

    <div class="grid grid-cols-1 gap-3.5 sm:grid-cols-2">
      <MdTextField
        :model-value="concurrentRate"
        size="sm"
        :label="t('onlineSources.edit.concurrentRateLabel')"
        :placeholder="t('onlineSources.edit.concurrentRatePlaceholder')"
        supporting-text="concurrentRate"
        title="concurrentRate"
        class="font-mono"
        @update:model-value="emit('update:concurrentRate', $event)"
      />

      <MdTextField
        v-if="hasUrlPattern"
        :model-value="urlPattern"
        size="sm"
        :label="urlPatternLabel"
        :supporting-text="urlPatternJsonKey"
        :title="urlPatternJsonKey"
        class="font-mono"
        @update:model-value="emit('update:urlPattern', $event)"
      />

      <div class="sm:col-span-2">
        <MdTextField
          :model-value="header"
          multiline
          :rows="2"
          size="sm"
          :label="t('onlineSources.edit.headerLabel')"
          :placeholder="t('onlineSources.edit.headerPlaceholder')"
          :supporting-text="headerJsonKey"
          :title="headerJsonKey"
          class="font-mono"
          @update:model-value="emit('update:header', $event)"
        />
      </div>

      <div v-if="hasSecondaryHeader" class="sm:col-span-2">
        <MdTextField
          :model-value="secondaryHeader"
          multiline
          :rows="2"
          size="sm"
          :label="secondaryHeaderLabel"
          :placeholder="t('onlineSources.edit.headerPlaceholder')"
          :supporting-text="secondaryHeaderJsonKey"
          :title="secondaryHeaderJsonKey"
          class="font-mono"
          @update:model-value="emit('update:secondaryHeader', $event)"
        />
      </div>

      <div class="sm:col-span-2">
        <MdTextField
          :model-value="loginUrl"
          multiline
          :rows="2"
          size="sm"
          :label="t('onlineSources.edit.loginUrlLabel')"
          supporting-text="loginUrl"
          title="loginUrl"
          class="font-mono"
          @update:model-value="emit('update:loginUrl', $event)"
        />
      </div>

      <div class="sm:col-span-2">
        <MdTextField
          :model-value="loginUi"
          multiline
          :rows="3"
          size="sm"
          :label="t('onlineSources.edit.loginUiLabel')"
          supporting-text="loginUi"
          title="loginUi"
          class="font-mono"
          @update:model-value="emit('update:loginUi', $event)"
        />
      </div>

      <div class="sm:col-span-2">
        <MdTextField
          :model-value="loginCheckJs"
          multiline
          :rows="2"
          size="sm"
          :label="t('onlineSources.edit.loginCheckJsLabel')"
          supporting-text="loginCheckJs"
          title="loginCheckJs"
          class="font-mono"
          @update:model-value="emit('update:loginCheckJs', $event)"
        />
      </div>

      <div class="sm:col-span-2">
        <MdTextField
          :model-value="jsLib"
          multiline
          :rows="3"
          size="sm"
          :label="t('onlineSources.edit.jsLibLabel')"
          supporting-text="jsLib"
          title="jsLib"
          class="font-mono"
          @update:model-value="emit('update:jsLib', $event)"
        />
      </div>

      <div class="sm:col-span-2">
        <MdTextField
          :model-value="variableText"
          size="sm"
          :label="t('onlineSources.edit.variableLabel')"
          :placeholder="t('onlineSources.edit.variablePlaceholder')"
          supporting-text="source.variable"
          title="source.variable"
          class="font-mono"
          @update:model-value="emit('update:variableText', $event)"
        />
      </div>
    </div>

    <slot />
  </section>
</template>
