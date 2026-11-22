<script setup lang="ts">
import { computed, useSlots } from 'vue'
import { LuChevronRight, LuMoreVertical } from 'vue-icons-plus/lu'
import { MdIconButton, MdLinearProgress, MdMenu } from '@/components/ui'

const props = withDefaults(
  defineProps<{
    badge: string
    title: string
    metaText?: string
    progressText?: string
    progressPercent?: number | null
    completed?: boolean
    completedLabel?: string
    missing?: boolean
    missingLabel?: string
    active?: boolean
    disabled?: boolean
    menuAriaLabel?: string
  }>(),
  {
    metaText: '',
    progressText: '',
    progressPercent: null,
    completed: false,
    completedLabel: '',
    missing: false,
    missingLabel: '',
    active: false,
    disabled: false,
    menuAriaLabel: '',
  },
)

const emit = defineEmits<{
  open: []
}>()

const slots = useSlots()

const hasMenu = computed(() => Boolean(slots.menu))
const normalizedProgress = computed<number | null>(() => {
  if (props.progressPercent === null || props.progressPercent === undefined) return null
  if (!Number.isFinite(props.progressPercent) || props.progressPercent <= 0) return null
  return Math.min(100, Math.max(0, Math.round(props.progressPercent)))
})
</script>

<template>
  <div
    class="group flex items-center gap-2 px-4 py-3 transition-colors duration-150 ease-standard hover:bg-surface-container-high/70 sm:px-5"
    :class="[
      missing || disabled ? 'opacity-55' : '',
      active ? 'bg-secondary-container/35' : '',
    ]"
  >
    <button
      type="button"
      :disabled="disabled || missing"
      class="flex min-w-0 flex-1 cursor-pointer items-center justify-between gap-3 text-left outline-none focus-visible:rounded-corner-sm focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-secondary disabled:cursor-default"
      @click="emit('open')"
    >
      <div class="min-w-0 flex-1 space-y-1">
        <div class="flex flex-wrap items-center gap-2">
          <span
            class="text-label-medium shrink-0 rounded-corner-xs bg-surface-container-highest px-2 py-0.5 text-on-surface"
          >
            {{ badge }}
          </span>
          <span
            class="text-title-small min-w-0 truncate"
            :class="
              completed
                ? 'text-on-surface-variant'
                : 'text-on-surface group-hover:text-primary'
            "
          >
            {{ title }}
          </span>
          <span
            v-if="completed && completedLabel"
            class="text-label-small shrink-0 rounded-corner-full bg-success-container px-2 py-0.5 text-on-success-container"
          >
            {{ completedLabel }}
          </span>
          <span
            v-if="missing && missingLabel"
            class="text-label-small shrink-0 rounded-corner-full bg-error-container px-2 py-0.5 text-on-error-container"
          >
            {{ missingLabel }}
          </span>
        </div>

        <div
          v-if="metaText || progressText || normalizedProgress !== null"
          class="flex flex-wrap items-center gap-3 text-body-small text-on-surface-variant"
        >
          <span v-if="metaText">{{ metaText }}</span>
          <span v-if="progressText" class="text-label-medium text-primary">
            {{ progressText }}
          </span>
          <div v-if="normalizedProgress !== null" class="w-24 shrink-0">
            <MdLinearProgress
              :value="normalizedProgress"
              :aria-label="`${title} ${normalizedProgress}%`"
            />
          </div>
        </div>
      </div>

      <slot name="trailing">
        <LuChevronRight
          v-if="!disabled && !missing && !hasMenu"
          class="h-4 w-4 shrink-0 text-on-surface-variant/70 transition-transform duration-150 group-hover:translate-x-0.5 group-hover:text-primary"
          aria-hidden="true"
        />
      </slot>
    </button>

    <div v-if="hasMenu" class="shrink-0" @click.stop>
      <MdMenu placement="bottom-end" :aria-label="menuAriaLabel">
        <template #trigger="{ toggle, triggerAttrs }">
          <MdIconButton
            v-bind="triggerAttrs"
            size="sm"
            variant="standard"
            :icon="LuMoreVertical"
            :aria-label="menuAriaLabel"
            @click="toggle"
          />
        </template>
        <template #default="{ close }">
          <slot name="menu" :close="close" />
        </template>
      </MdMenu>
    </div>
  </div>
</template>
