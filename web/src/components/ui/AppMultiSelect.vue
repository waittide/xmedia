<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuChevronDown, LuSearch, LuX } from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'

export interface MultiSelectOption<T extends string = string> {
  value: T
  label: string
  badge?: string | number
}

const MAX_RENDERED_ITEMS = 120

const props = withDefaults(
  defineProps<{
    modelValue: string[]
    options: MultiSelectOption[]
    label: string
    allPlaceholder: string
    searchPlaceholder?: string
    noMatchText?: string
    clearText?: string
    selectedCountText?: (label: string, count: number) => string
    showingTopText?: (shown: number, total: number) => string
    disabled?: boolean
  }>(),
  {
    searchPlaceholder: '',
    noMatchText: '',
    clearText: '',
    selectedCountText: undefined,
    showingTopText: undefined,
    disabled: false,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: string[]]
  change: [value: string[]]
}>()

const { t } = useI18n()

const resolvedSearchPlaceholder = computed(
  () => props.searchPlaceholder || t('common.filterPlaceholder'),
)
const resolvedNoMatchText = computed(() => props.noMatchText || t('common.noResults'))
const resolvedClearText = computed(() => props.clearText || t('common.clear'))

function formatSelectedCount(count: number): string {
  if (props.selectedCountText) {
    return props.selectedCountText(props.label, count)
  }
  return t('common.selectedCount', { label: props.label, count })
}

function formatShowingTop(shown: number, total: number): string {
  if (props.showingTopText) {
    return props.showingTopText(shown, total)
  }
  return t('common.showingTop', { shown, total })
}

const listboxId = useId()
const open = ref(false)
const query = ref('')
const highlightedIdx = ref(0)
const triggerRef = ref<HTMLButtonElement | null>(null)
const menuRef = ref<HTMLDivElement | null>(null)
const searchInputRef = ref<HTMLInputElement | null>(null)
const menuStyle = ref<Record<string, string>>({})

function optionDomId(idx: number): string {
  return `${listboxId}-opt-${idx}`
}

const activeDescendantId = computed<string | undefined>(() => {
  if (!open.value || renderedOptions.value.length === 0) return undefined
  const idx = Math.max(0, Math.min(highlightedIdx.value, renderedOptions.value.length - 1))
  return optionDomId(idx)
})

const selectedSet = computed(() => new Set(props.modelValue))

const triggerText = computed(() => {
  if (props.modelValue.length === 0) return props.allPlaceholder
  if (props.modelValue.length === 1) {
    const first = props.modelValue[0]!
    return props.options.find((o) => o.value === first)?.label ?? first
  }
  return formatSelectedCount(props.modelValue.length)
})

const showSearchBox = computed(() => props.options.length > 5)

// Selected items are pinned to the top so users never lose track of checked items in a 500-item list
const matchedOptions = computed(() => {
  const q = query.value.trim().toLowerCase()
  const matched = q
    ? props.options.filter((opt) => opt.label.toLowerCase().includes(q))
    : props.options

  const checked: MultiSelectOption[] = []
  const unchecked: MultiSelectOption[] = []
  for (const opt of matched) {
    if (selectedSet.value.has(opt.value)) {
      checked.push(opt)
    } else {
      unchecked.push(opt)
    }
  }
  return [...checked, ...unchecked]
})

const renderedOptions = computed(() =>
  matchedOptions.value.slice(0, MAX_RENDERED_ITEMS),
)

function updatePosition(): void {
  if (!open.value) return
  const el = triggerRef.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const viewportH = window.innerHeight
  const viewportW = window.innerWidth
  const estimatedHeight = 320
  const openUpward = rect.bottom + estimatedHeight + 12 > viewportH && rect.top > estimatedHeight

  const minWidth = Math.max(rect.width, 232)
  const left = Math.min(rect.left, Math.max(8, viewportW - minWidth - 8))

  menuStyle.value = {
    position: 'fixed',
    left: `${Math.round(left)}px`,
    minWidth: `${Math.round(minWidth)}px`,
    maxWidth: 'min(22rem, calc(100vw - 16px))',
    ...(openUpward
      ? { bottom: `${Math.round(viewportH - rect.top + 6)}px` }
      : { top: `${Math.round(rect.bottom + 6)}px` }),
  }
}

async function openMenu(): Promise<void> {
  if (props.disabled) return
  open.value = true
  query.value = ''
  highlightedIdx.value = 0
  updatePosition()
  await nextTick()
  updatePosition()
  if (showSearchBox.value) {
    searchInputRef.value?.focus()
  }
}

function onTriggerKeydown(ev: KeyboardEvent): void {
  if (props.disabled) return
  if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp' || ev.key === 'Enter' || ev.key === ' ') {
    if (!open.value) {
      ev.preventDefault()
      void openMenu()
      return
    }
  }
  onMenuKeydown(ev)
}

function onMenuKeydown(ev: KeyboardEvent): void {
  if (!open.value || ev.isComposing) return
  const len = renderedOptions.value.length
  if (ev.key === 'ArrowDown' && len > 0) {
    ev.preventDefault()
    ev.stopPropagation()
    highlightedIdx.value = (highlightedIdx.value + 1) % len
  } else if (ev.key === 'ArrowUp' && len > 0) {
    ev.preventDefault()
    ev.stopPropagation()
    highlightedIdx.value = (highlightedIdx.value - 1 + len) % len
  } else if (ev.key === 'Home' && len > 0) {
    ev.preventDefault()
    ev.stopPropagation()
    highlightedIdx.value = 0
  } else if (ev.key === 'End' && len > 0) {
    ev.preventDefault()
    ev.stopPropagation()
    highlightedIdx.value = len - 1
  } else if (
    (ev.key === 'Enter' || (ev.key === ' ' && document.activeElement !== searchInputRef.value)) &&
    len > 0
  ) {
    ev.preventDefault()
    ev.stopPropagation()
    const picked = renderedOptions.value[highlightedIdx.value]
    if (picked) toggleValue(picked.value)
  }
}

function closeMenu(): void {
  open.value = false
}

function toggleMenu(): void {
  if (open.value) {
    closeMenu()
  } else {
    void openMenu()
  }
}

function toggleValue(val: string): void {
  const next = selectedSet.value.has(val)
    ? props.modelValue.filter((v) => v !== val)
    : [...props.modelValue, val]
  emit('update:modelValue', next)
  emit('change', next)
}

function clearAll(): void {
  if (props.modelValue.length === 0) return
  emit('update:modelValue', [])
  emit('change', [])
}

function handleOutsideMouseDown(ev: MouseEvent): void {
  if (!open.value) return
  const target = ev.target as Node
  if (triggerRef.value?.contains(target) || menuRef.value?.contains(target)) {
    return
  }
  closeMenu()
}

const escapeStack = useEscapeStack()

escapeStack.register(
  () => open.value,
  () => {
    closeMenu()
    triggerRef.value?.focus()
  },
)

function handleViewportChange(): void {
  if (open.value) updatePosition()
}

watch(renderedOptions, () => {
  if (highlightedIdx.value >= renderedOptions.value.length) {
    highlightedIdx.value = 0
  }
  if (open.value) void nextTick(updatePosition)
})

function bindGlobalListeners(): void {
  document.addEventListener('mousedown', handleOutsideMouseDown)
  window.addEventListener('resize', handleViewportChange)
  window.addEventListener('scroll', handleViewportChange, true)
}

function unbindGlobalListeners(): void {
  document.removeEventListener('mousedown', handleOutsideMouseDown)
  window.removeEventListener('resize', handleViewportChange)
  window.removeEventListener('scroll', handleViewportChange, true)
}

watch(open, (isOpen) => {
  if (isOpen) bindGlobalListeners()
  else unbindGlobalListeners()
})

onBeforeUnmount(unbindGlobalListeners)
</script>

<template>
  <button
    ref="triggerRef"
    type="button"
    role="combobox"
    aria-haspopup="listbox"
    :aria-expanded="open"
    :aria-controls="open ? listboxId : undefined"
    :aria-activedescendant="activeDescendantId"
    :aria-label="label"
    :disabled="disabled"
    class="inline-flex h-9 w-full cursor-pointer items-center justify-between gap-2 rounded-[var(--md-sys-shape-corner-xs)] border px-3 text-left text-xs font-medium transition outline-none select-none focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-[var(--md-sys-color-secondary)] disabled:cursor-not-allowed disabled:opacity-38"
    :class="[
      open
        ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] ring-1 ring-[var(--md-sys-color-primary)]'
        : modelValue.length > 0
          ? 'border-[var(--md-sys-color-primary)]/70 bg-[var(--md-sys-color-secondary-container)]/70 text-[var(--md-sys-color-on-secondary-container)]'
          : 'border-[var(--md-sys-color-outline)] bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)] hover:border-[var(--md-sys-color-on-surface)]',
    ]"
    @click="toggleMenu"
    @keydown="onTriggerKeydown"
  >
    <span class="truncate">{{ triggerText }}</span>
    <span class="flex shrink-0 items-center gap-1">
      <span
        v-if="modelValue.length > 1"
        class="rounded-full bg-[var(--md-sys-color-primary)] px-1.5 py-0.2 text-[10px] font-bold text-[var(--md-sys-color-on-primary)]"
      >
        {{ modelValue.length }}
      </span>
      <LuChevronDown
        class="h-3.5 w-3.5 text-[var(--md-sys-color-on-surface-variant)] transition-transform duration-150"
        :class="open ? 'rotate-180 text-[var(--md-sys-color-primary)]' : ''"
      />
    </span>
  </button>

  <Teleport to="body">
    <div
      v-if="open"
      ref="menuRef"
      :style="menuStyle"
      class="z-[var(--z-popover)] overflow-hidden rounded-[var(--md-sys-shape-corner-xs)] bg-[var(--md-sys-color-surface-container)] p-2 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-2)] ring-1 ring-[var(--md-sys-color-outline-variant)]/40"
      @keydown="onMenuKeydown"
    >
      <!-- Optional Header Controls Slot (e.g. Tag Match Any / Match All toggle) -->
      <div
        v-if="$slots.header"
        class="mb-2 border-b border-[var(--md-sys-color-outline-variant)]/60 pb-2"
      >
        <slot name="header" />
      </div>

      <!-- Instant Search Filter for Long Lists -->
      <div v-if="showSearchBox" class="relative mb-1.5">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-2.5 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          ref="searchInputRef"
          v-model="query"
          type="text"
          :placeholder="resolvedSearchPlaceholder"
          class="md-input h-8 w-full !pr-7 !pl-8 text-xs"
        />
        <button
          v-if="query"
          type="button"
          class="md-state absolute top-1/2 right-2 -translate-y-1/2 rounded-full p-0.5 text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
          :aria-label="resolvedClearText"
          @click="query = ''"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>

      <!-- Scrollable Option List (Selected pinned at top) -->
      <div
        :id="listboxId"
        role="listbox"
        aria-multiselectable="true"
        :aria-label="label"
        class="max-h-60 space-y-0.5 overflow-y-auto pr-0.5"
      >
        <div
          v-if="renderedOptions.length === 0"
          class="py-6 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ resolvedNoMatchText }}
        </div>

        <button
          v-for="(opt, idx) in renderedOptions"
          :id="optionDomId(idx)"
          :key="opt.value"
          type="button"
          role="option"
          :aria-selected="selectedSet.has(opt.value)"
          class="md-state flex w-full cursor-pointer items-center justify-between gap-2.5 rounded-[var(--md-sys-shape-corner-xs)] px-2.5 py-1.5 text-left text-xs transition"
          :class="
            selectedSet.has(opt.value)
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : idx === highlightedIdx
                ? 'bg-[color-mix(in_srgb,var(--md-sys-color-on-surface)_8%,transparent)] text-[var(--md-sys-color-on-surface)]'
                : 'text-[var(--md-sys-color-on-surface)]'
          "
          @mouseenter="highlightedIdx = idx"
          @click="toggleValue(opt.value)"
        >
          <span class="flex min-w-0 items-center gap-2">
            <span
              class="flex h-4 w-4 shrink-0 items-center justify-center rounded-[2px] border-2 transition"
              :class="
                selectedSet.has(opt.value)
                  ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]'
                  : 'border-[var(--md-sys-color-on-surface-variant)] bg-transparent'
              "
            >
              <LuCheck v-if="selectedSet.has(opt.value)" class="h-3 w-3 stroke-[3]" />
            </span>
            <span class="truncate">{{ opt.label }}</span>
          </span>

          <span
            v-if="opt.badge !== undefined"
            class="shrink-0 rounded-md bg-[var(--md-sys-color-surface-container-highest)] px-1.5 py-0.5 text-[10px] font-medium tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ opt.badge }}
          </span>
        </button>

        <p
          v-if="matchedOptions.length > MAX_RENDERED_ITEMS"
          class="px-2 py-1.5 text-center text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ formatShowingTop(MAX_RENDERED_ITEMS, matchedOptions.length) }}
        </p>
      </div>

      <!-- Footer Clear Action -->
      <div
        v-if="modelValue.length > 0"
        class="mt-1.5 flex items-center justify-between border-t border-[var(--md-sys-color-outline-variant)]/60 px-1 pt-1.5"
      >
        <span class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
          {{ formatSelectedCount(modelValue.length) }}
        </span>
        <button
          type="button"
          class="md-state cursor-pointer rounded-full px-2 py-0.5 text-[11px] font-semibold text-[var(--md-sys-color-primary)] hover:underline"
          @click="clearAll"
        >
          {{ resolvedClearText }}
        </button>
      </div>
    </div>
  </Teleport>
</template>
