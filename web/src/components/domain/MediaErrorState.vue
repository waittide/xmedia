<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuAlertCircle, LuRefreshCw } from 'vue-icons-plus/lu'
import { MdButton } from '@/components/ui'

const props = withDefaults(
  defineProps<{
    title?: string
    message?: string | null
    retryLabel?: string
    retrying?: boolean
    showRetry?: boolean
    compact?: boolean
  }>(),
  {
    title: '',
    message: null,
    retryLabel: '',
    retrying: false,
    showRetry: true,
    compact: false,
  },
)

const emit = defineEmits<{
  retry: []
}>()

const { t } = useI18n()

const resolvedTitle = computed(() => props.title || t('common.error'))
const resolvedRetryLabel = computed(() => props.retryLabel || t('remote.retryBtn'))
</script>

<template>
  <div
    v-if="compact"
    role="alert"
    aria-live="assertive"
    class="flex flex-wrap items-center justify-between gap-3 rounded-corner-md bg-error-container/80 px-4 py-3 text-on-error-container"
  >
    <div class="flex min-w-0 flex-1 items-start gap-2.5">
      <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
      <div class="min-w-0 flex-1">
        <p class="text-label-large">
          {{ resolvedTitle }}
        </p>
        <p v-if="message" class="text-body-small mt-0.5 break-words opacity-90">
          {{ message }}
        </p>
        <slot />
      </div>
    </div>

    <div v-if="showRetry || $slots.action" class="flex shrink-0 items-center gap-2">
      <slot name="action" />
      <MdButton
        v-if="showRetry"
        variant="tonal"
        size="sm"
        :icon="LuRefreshCw"
        :loading="retrying"
        @click="emit('retry')"
      >
        {{ resolvedRetryLabel }}
      </MdButton>
    </div>
  </div>

  <div
    v-else
    role="alert"
    aria-live="assertive"
    class="flex flex-col items-center justify-center rounded-corner-xl bg-surface-container-low px-6 py-12 text-center"
  >
    <div
      class="mb-4 flex h-14 w-14 items-center justify-center rounded-corner-xl bg-error-container text-on-error-container"
      aria-hidden="true"
    >
      <LuAlertCircle class="h-6 w-6" />
    </div>

    <h3 class="text-title-medium text-on-surface">
      {{ resolvedTitle }}
    </h3>

    <p v-if="message" class="text-body-medium mt-1.5 max-w-md break-words text-on-surface-variant">
      {{ message }}
    </p>

    <slot />

    <div
      v-if="showRetry || $slots.action"
      class="mt-5 flex flex-wrap items-center justify-center gap-3"
    >
      <MdButton
        v-if="showRetry"
        variant="tonal"
        :icon="LuRefreshCw"
        :loading="retrying"
        @click="emit('retry')"
      >
        {{ resolvedRetryLabel }}
      </MdButton>
      <slot name="action" />
    </div>
  </div>
</template>
