<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, provide, ref, useId, watch } from 'vue'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { MD_MENU_CONTEXT_KEY } from './menuContext'

export type MdMenuPlacement = 'bottom-start' | 'bottom-end' | 'top-start' | 'top-end'

const props = withDefaults(
  defineProps<{
    open?: boolean
    placement?: MdMenuPlacement
    minWidth?: number
    maxWidth?: number
    anchorEl?: HTMLElement | null
    teleportTo?: string | HTMLElement | null
    ariaLabel?: string
  }>(),
  {
    open: undefined,
    placement: 'bottom-end',
    minWidth: 180,
    maxWidth: 280,
    anchorEl: null,
    teleportTo: null,
    ariaLabel: undefined,
  },
)

const emit = defineEmits<{
  'update:open': [value: boolean]
  close: []
}>()

const menuId = useId()
const escapeStack = useEscapeStack()
const internalOpen = ref(false)
const triggerWrapRef = ref<HTMLElement | null>(null)
const menuRef = ref<HTMLDivElement | null>(null)
const menuStyle = ref<Record<string, string>>({})

const isControlled = computed(() => props.open !== undefined)
const isOpen = computed(() => (isControlled.value ? Boolean(props.open) : internalOpen.value))

const resolvedTeleportTarget = computed<string | HTMLElement>(() => {
  if (props.teleportTo) return props.teleportTo
  if (typeof document !== 'undefined' && document.fullscreenElement instanceof HTMLElement) {
    return document.fullscreenElement
  }
  return 'body'
})

function setOpen(next: boolean): void {
  if (!isControlled.value) {
    internalOpen.value = next
  }
  emit('update:open', next)
  if (!next) {
    emit('close')
  }
}

function getAnchorElement(): HTMLElement | null {
  if (props.anchorEl) return props.anchorEl
  const wrap = triggerWrapRef.value
  if (!wrap) return null
  return (wrap.firstElementChild as HTMLElement | null) ?? wrap
}

function focusTrigger(): void {
  const anchor = getAnchorElement()
  if (anchor && typeof anchor.focus === 'function') {
    anchor.focus()
  }
}

function close(restoreFocus = false): void {
  if (!isOpen.value) return
  setOpen(false)
  if (restoreFocus) {
    focusTrigger()
  }
}

function toggle(): void {
  setOpen(!isOpen.value)
}

provide(MD_MENU_CONTEXT_KEY, {
  close: () => close(true),
})

escapeStack.register(
  () => isOpen.value,
  () => close(true),
)

function getMenuItems(): HTMLElement[] {
  const el = menuRef.value
  if (!el) return []
  return Array.from(
    el.querySelectorAll<HTMLElement>(
      '[role="menuitem"]:not([disabled]):not([aria-disabled="true"])',
    ),
  )
}

function updatePosition(): void {
  if (!isOpen.value || typeof window === 'undefined') return
  const anchor = getAnchorElement()
  if (!anchor) return

  const rect = anchor.getBoundingClientRect()
  const vw = window.innerWidth
  const vh = window.innerHeight
  const margin = 8
  const gap = 6

  const measuredW = menuRef.value?.offsetWidth || Math.max(rect.width, props.minWidth)
  const measuredH = menuRef.value?.offsetHeight || 200
  const width = Math.min(Math.max(measuredW, props.minWidth), Math.min(props.maxWidth, vw - margin * 2))

  const preferTop = props.placement.startsWith('top')
  const preferEnd = props.placement.endsWith('end')

  const spaceBelow = vh - rect.bottom - margin
  const spaceAbove = rect.top - margin

  let openUpward = preferTop
  if (!preferTop && measuredH + gap > spaceBelow && spaceAbove > spaceBelow) {
    openUpward = true
  } else if (preferTop && measuredH + gap > spaceAbove && spaceBelow > spaceAbove) {
    openUpward = false
  }

  let left = preferEnd ? rect.right - width : rect.left
  left = Math.max(margin, Math.min(left, vw - width - margin))

  const maxHeight = Math.max(120, Math.floor((openUpward ? spaceAbove : spaceBelow) - gap))

  menuStyle.value = {
    position: 'fixed',
    left: `${Math.round(left)}px`,
    minWidth: `${Math.round(Math.min(props.minWidth, vw - margin * 2))}px`,
    maxWidth: `${Math.round(Math.min(props.maxWidth, vw - margin * 2))}px`,
    maxHeight: `${maxHeight}px`,
    ...(openUpward
      ? { bottom: `${Math.round(vh - rect.top + gap)}px` }
      : { top: `${Math.round(rect.bottom + gap)}px` }),
  }
}

function onMenuKeydown(ev: KeyboardEvent): void {
  if (!isOpen.value) return
  const items = getMenuItems()
  if (ev.key === 'Tab') {
    close(false)
    return
  }
  if (items.length === 0) return

  const currentIdx = items.indexOf(document.activeElement as HTMLElement)
  if (ev.key === 'ArrowDown') {
    ev.preventDefault()
    items[(currentIdx + 1 + items.length) % items.length]?.focus()
  } else if (ev.key === 'ArrowUp') {
    ev.preventDefault()
    items[(currentIdx - 1 + items.length) % items.length]?.focus()
  } else if (ev.key === 'Home') {
    ev.preventDefault()
    items[0]?.focus()
  } else if (ev.key === 'End') {
    ev.preventDefault()
    items[items.length - 1]?.focus()
  }
}

function handleOutsideMouseDown(ev: MouseEvent): void {
  if (!isOpen.value) return
  const target = ev.target as Node
  if (
    menuRef.value?.contains(target) ||
    triggerWrapRef.value?.contains(target) ||
    props.anchorEl?.contains(target)
  ) {
    return
  }
  close(false)
}

function handleViewportChange(): void {
  if (isOpen.value) updatePosition()
}

function bindListeners(): void {
  document.addEventListener('mousedown', handleOutsideMouseDown)
  window.addEventListener('resize', handleViewportChange)
  window.addEventListener('scroll', handleViewportChange, true)
}

function unbindListeners(): void {
  document.removeEventListener('mousedown', handleOutsideMouseDown)
  window.removeEventListener('resize', handleViewportChange)
  window.removeEventListener('scroll', handleViewportChange, true)
}

watch(
  isOpen,
  async (openNow) => {
    if (openNow) {
      bindListeners()
      updatePosition()
      await nextTick()
      updatePosition()
      const items = getMenuItems()
      if (items.length > 0) {
        items[0]?.focus()
      } else {
        menuRef.value?.focus()
      }
    } else {
      unbindListeners()
    }
  },
  { immediate: true },
)

onBeforeUnmount(unbindListeners)

const triggerAttrs = computed(() => ({
  'aria-haspopup': 'menu' as const,
  'aria-expanded': isOpen.value,
  'aria-controls': isOpen.value ? menuId : undefined,
}))

defineExpose({ open: () => setOpen(true), close, toggle, updatePosition })
</script>

<template>
  <span v-if="$slots.trigger" ref="triggerWrapRef" class="inline-flex">
    <slot
      name="trigger"
      :open="isOpen"
      :toggle="toggle"
      :close="() => close(false)"
      :menu-id="menuId"
      :trigger-attrs="triggerAttrs"
    />
  </span>

  <Teleport :to="resolvedTeleportTarget">
    <Transition name="md-dialog">
      <div
        v-if="isOpen"
        :id="menuId"
        ref="menuRef"
        role="menu"
        tabindex="-1"
        :aria-label="ariaLabel"
        :style="menuStyle"
        class="z-[var(--z-popover)] overflow-y-auto rounded-corner-xs bg-surface-container py-2 px-1 text-on-surface shadow-elevation-2 outline-none"
        @keydown="onMenuKeydown"
      >
        <slot :close="() => close(true)" />
      </div>
    </Transition>
  </Teleport>
</template>
