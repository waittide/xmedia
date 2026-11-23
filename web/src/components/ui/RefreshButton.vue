<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuAlertCircle, LuCheck, LuRefreshCw } from 'vue-icons-plus/lu'
import { useActionFeedback } from '@/composables/useActionFeedback'

const props = withDefaults(
  defineProps<{
    action: () => Promise<unknown> | unknown
    busy?: boolean
    size?: 'sm' | 'md'
    variant?: 'bordered' | 'ghost'
    title?: string
    minSpinMs?: number
  }>(),
  {
    busy: false,
    size: 'sm',
    variant: 'bordered',
    title: '',
    minSpinMs: 420,
  },
)

const { t } = useI18n()
const feedback = useActionFeedback()

const isSpinning = computed(() => feedback.running.value || props.busy)

const effectiveTitle = computed(() => {
  if (isSpinning.value) return t('common.refreshing')
  if (feedback.done.value) return t('common.refreshed')
  if (feedback.errored.value) return t('common.refreshFailed')
  return props.title || t('common.refresh')
})

const sizeClasses = computed(() => (props.size === 'md' ? 'h-10 w-10' : 'h-8 w-8'))
const iconClasses = computed(() => (props.size === 'md' ? 'h-4 w-4' : 'h-3.5 w-3.5'))

const stateClasses = computed(() => {
  if (feedback.done.value) {
    return props.variant === 'ghost'
      ? 'bg-success-container text-on-success-container'
      : 'border border-success/45 bg-success-container text-on-success-container'
  }
  if (feedback.errored.value) {
    return props.variant === 'ghost'
      ? 'bg-error-container text-on-error-container'
      : 'border border-error/45 bg-error-container text-on-error-container'
  }
  if (isSpinning.value) {
    return props.variant === 'ghost'
      ? 'bg-primary-container text-on-primary-container'
      : 'border border-primary/60 bg-surface text-primary'
  }
  return props.variant === 'ghost'
    ? 'text-on-surface-variant hover:text-on-surface'
    : 'border border-outline-variant bg-surface text-on-surface-variant hover:border-outline hover:text-on-surface'
})

function handleClick(): void {
  if (isSpinning.value) return
  void feedback.run(props.action, { minSpinMs: props.minSpinMs })
}
</script>

<template>
  <button
    type="button"
    :disabled="isSpinning"
    :title="effectiveTitle"
    :aria-label="effectiveTitle"
    class="md-state md-touch-target inline-flex shrink-0 cursor-pointer items-center justify-center rounded-corner-full disabled:cursor-wait"
    :class="[sizeClasses, stateClasses]"
    @click="handleClick"
  >
    <LuCheck v-if="feedback.done.value && !isSpinning" :class="iconClasses" aria-hidden="true" />
    <LuAlertCircle v-else-if="feedback.errored.value && !isSpinning" :class="iconClasses" aria-hidden="true" />
    <LuRefreshCw v-else :class="[iconClasses, isSpinning ? 'animate-spin' : '']" aria-hidden="true" />
  </button>
</template>
