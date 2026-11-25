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
    class="md-touch-target text-label-medium inline-flex h-9 w-full cursor-pointer items-center justify-between gap-2 rounded-corner-xs border px-3 text-left transition outline-none select-none focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-secondary disabled:cursor-not-allowed disabled:opacity-38"
    :class="[
      open
        ? 'border-primary bg-surface-container-lowest text-on-surface ring-1 ring-inset ring-primary'
        : modelValue.length > 0
          ? 'border-primary/70 bg-secondary-container/70 text-on-secondary-container'
          : 'border-outline bg-surface-container-lowest text-on-surface hover:border-on-surface',
    ]"
    @click="toggleMenu"
    @keydown="onTriggerKeydown"
  >
    <span class="truncate">{{ triggerText }}</span>
    <span class="flex shrink-0 items-center gap-1">
      <span
        v-if="modelValue.length > 1"
        class="text-label-small rounded-corner-full bg-primary px-1.5 py-0.5 tabular-nums text-on-primary"
      >
        {{ modelValue.length }}
      </span>
      <LuChevronDown
        class="h-3.5 w-3.5 text-on-surface-variant transition-transform duration-150"
        :class="open ? 'rotate-180 text-primary' : ''"
      />
    </span>
  </button>

  <Teleport to="body">
    <div
      v-if="open"
      ref="menuRef"
      :style="menuStyle"
      class="z-[var(--z-popover)] overflow-hidden rounded-corner-xs bg-surface-container py-2 text-on-surface shadow-elevation-2"
      @keydown="onMenuKeydown"
    >
      <!-- Optional Header Controls Slot (e.g. Tag Match Any / Match All toggle) -->
      <div
        v-if="$slots.header"
        class="mb-2 bg-surface-container-high px-3 py-2"
      >
        <slot name="header" />
      </div>

      <!-- Instant Search Filter for Long Lists -->
      <div v-if="showSearchBox" class="relative px-3 pb-2">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-5.5 -mt-1 h-4 w-4 -translate-y-1/2 text-on-surface-variant"
        />
        <input
          ref="searchInputRef"
          v-model="query"
          type="text"
          :placeholder="resolvedSearchPlaceholder"
          class="md-input text-base sm:text-body-small h-9 w-full !pr-8 !pl-8"
        />
        <button
          v-if="query"
          type="button"
          class="md-state md-touch-target absolute top-1/2 right-4 -mt-1 -translate-y-1/2 rounded-corner-full p-0.5 text-on-surface-variant hover:text-on-surface"
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
        class="max-h-60 overflow-y-auto"
      >
        <div
          v-if="renderedOptions.length === 0"
          class="text-body-small py-6 text-center text-on-surface-variant"
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
          class="md-state text-body-medium flex h-12 w-full cursor-pointer items-center justify-between gap-3 px-3 text-left transition"
          :class="
            selectedSet.has(opt.value)
              ? 'bg-secondary-container text-on-secondary-container'
              : idx === highlightedIdx
                ? 'bg-on-surface/8 text-on-surface'
                : 'text-on-surface'
          "
          @mouseenter="highlightedIdx = idx"
          @click="toggleValue(opt.value)"
        >
          <span class="flex min-w-0 items-center gap-3">
            <span
              class="flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded-[2px] border-2 transition"
              :class="
                selectedSet.has(opt.value)
                  ? 'border-primary bg-primary text-on-primary'
                  : 'border-on-surface-variant bg-transparent'
              "
            >
              <LuCheck v-if="selectedSet.has(opt.value)" class="h-3.5 w-3.5 stroke-[3]" />
            </span>
            <span class="truncate">{{ opt.label }}</span>
          </span>

          <span
            v-if="opt.badge !== undefined"
            class="text-label-small shrink-0 rounded-corner-xs bg-surface-container-highest px-1.5 py-0.5 tabular-nums text-on-surface-variant"
          >
            {{ opt.badge }}
          </span>
        </button>

        <p
          v-if="matchedOptions.length > MAX_RENDERED_ITEMS"
          class="text-label-small px-3 py-1.5 text-center text-on-surface-variant"
        >
          {{ formatShowingTop(MAX_RENDERED_ITEMS, matchedOptions.length) }}
        </p>
      </div>

      <!-- Footer Clear Action -->
      <div
        v-if="modelValue.length > 0"
        class="mt-1 flex items-center justify-between bg-surface-container-high px-3 py-2"
      >
        <span class="text-label-small text-on-surface-variant">
          {{ formatSelectedCount(modelValue.length) }}
        </span>
        <button
          type="button"
          class="md-state md-touch-target text-label-medium cursor-pointer rounded-corner-full px-2.5 py-1 text-primary"
          @click="clearAll"
        >
          {{ resolvedClearText }}
        </button>
      </div>
    </div>
  </Teleport>
</template>
