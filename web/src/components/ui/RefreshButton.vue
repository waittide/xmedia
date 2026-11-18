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

const sizeClasses = computed(() => (props.size === 'md' ? 'h-9 w-9' : 'h-8 w-8'))
const iconClasses = computed(() => (props.size === 'md' ? 'h-4 w-4' : 'h-3.5 w-3.5'))

const stateClasses = computed(() => {
  if (feedback.done.value) {
    return props.variant === 'ghost'
      ? 'bg-emerald-500/12 text-emerald-500'
      : 'border border-emerald-500/45 bg-emerald-500/10 text-emerald-500'
  }
  if (feedback.errored.value) {
    return props.variant === 'ghost'
      ? 'bg-red-500/12 text-red-400'
      : 'border border-red-500/45 bg-red-500/10 text-red-400'
  }
  if (isSpinning.value) {
    return props.variant === 'ghost'
      ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
      : 'border border-[var(--ctp-accent)]/60 bg-[var(--ctp-canvas)] text-[var(--ctp-accent)]'
  }
  return props.variant === 'ghost'
    ? 'text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-shell)] hover:text-[var(--ctp-text)]'
    : 'border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext1)] hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]'
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
    class="inline-flex shrink-0 cursor-pointer items-center justify-center rounded-xl transition duration-150 active:scale-95 disabled:cursor-wait"
    :class="[sizeClasses, stateClasses]"
    @click="handleClick"
  >
    <LuCheck v-if="feedback.done.value && !isSpinning" :class="iconClasses" />
    <LuAlertCircle v-else-if="feedback.errored.value && !isSpinning" :class="iconClasses" />
    <LuRefreshCw v-else :class="[iconClasses, isSpinning ? 'animate-spin' : '']" />
  </button>
</template>
