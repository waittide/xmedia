<script setup lang="ts">
import {
  computed,
  onBeforeUnmount,
  onMounted,
  ref,
  shallowRef,
  watch,
  type Component,
} from 'vue'
import { useI18n } from 'vue-i18n'
import { LuArrowLeft } from 'vue-icons-plus/lu'
import { MdButton, MdSheet, type MdSheetSize } from '@/components/ui'
import { useEscapeStack } from '@/composables/useEscapeStack'

export type ImmersiveTapMode = 'center-zone' | 'full-stage' | 'disabled'

const INTERACTIVE_SELECTOR =
  'button, a, input, select, textarea, label, [role="button"], [role="combobox"], [role="slider"], [role="menuitem"], [role="tab"], [data-immersive-ignore]'

const props = withDefaults(
  defineProps<{
    title?: string
    subtitle?: string
    /** Apply `.md-scheme-dark` to stage & HUD (e.g. cinema video player). */
    darkScheme?: boolean
    stageBgClass?: string
    /** Controlled HUD visibility. Omit for internal auto-hide management. */
    hudVisible?: boolean
    autoHide?: boolean
    /** Auto-hide delay in ms (MD3 immersive spec: 2500–3000ms). */
    autoHideDelayMs?: number
    /** Externally lock HUD visible (e.g. while paused, buffering, or menu open). */
    lockHud?: boolean
    /** Hide mouse cursor on stage while HUD is hidden (ideal for video playback). */
    hideCursorWhenIdle?: boolean
    showBack?: boolean
    backLabel?: string
    /** Tap behavior on stage: `center-zone` (25% prev / 50% HUD / 25% next), `full-stage` (toggle HUD), or `disabled`. */
    centerTapMode?: ImmersiveTapMode
    /** Single mutually-exclusive active sheet key (e.g. `'toc' | 'settings' | 'episodes' | null`). */
    activeSheet?: string | null
    sheetTitle?: string
    sheetSubtitle?: string
    sheetSize?: MdSheetSize
    sheetTeleportTo?: string | HTMLElement | null
    /** Controlled transient centered OSD message. */
    osdText?: string | null
    osdIcon?: Component
    /** Register Escape handler with priority: close sheet -> hide HUD -> exit. */
    escapeToExit?: boolean
  }>(),
  {
    title: '',
    subtitle: '',
    darkScheme: false,
    stageBgClass: '',
    hudVisible: undefined,
    autoHide: true,
    autoHideDelayMs: 2800,
    lockHud: false,
    hideCursorWhenIdle: false,
    showBack: true,
    backLabel: '',
    centerTapMode: 'center-zone',
    activeSheet: null,
    sheetTitle: '',
    sheetSubtitle: '',
    sheetSize: 'md',
    sheetTeleportTo: undefined,
    osdText: null,
    osdIcon: undefined,
    escapeToExit: true,
  },
)

const emit = defineEmits<{
  back: []
  exit: []
  'update:hudVisible': [visible: boolean]
  'update:activeSheet': [sheet: string | null]
  'tap-prev': []
  'tap-next': []
  'stage-tap': [ev: MouseEvent]
}>()

const { t } = useI18n()
const escapeStack = useEscapeStack()

const stageRef = ref<HTMLElement | null>(null)
const headerRef = ref<HTMLElement | null>(null)
const footerRef = ref<HTMLElement | null>(null)

const internalHudVisible = ref(true)
const pointerInHud = ref(false)
const internalOsdText = ref<string | null>(null)
const internalOsdIcon = shallowRef<Component | undefined>(undefined)

let hideTimer: ReturnType<typeof setTimeout> | null = null
let osdTimer: ReturnType<typeof setTimeout> | null = null

const isControlledHud = computed(() => props.hudVisible !== undefined)
const isHudVisible = computed(() =>
  isControlledHud.value ? Boolean(props.hudVisible) : internalHudVisible.value,
)
const isSheetOpen = computed(() => Boolean(props.activeSheet))

const resolvedBackLabel = computed(() => props.backLabel || t('common.back'))

const resolvedStageBgClass = computed(() => {
  if (props.stageBgClass) return props.stageBgClass
  return props.darkScheme ? 'bg-black text-on-surface' : 'bg-surface-dim text-on-surface'
})

const hudBarClass = computed(() =>
  props.darkScheme
    ? 'bg-scrim/70 text-on-surface backdrop-blur-md'
    : 'bg-surface-container-high/90 text-on-surface shadow-elevation-2 backdrop-blur-md',
)

const activeOsdText = computed(() => props.osdText || internalOsdText.value)
const activeOsdIcon = computed(() => (props.osdText ? props.osdIcon : internalOsdIcon.value))

const cursorHidden = computed(
  () => props.hideCursorWhenIdle && !isHudVisible.value && !isSheetOpen.value,
)

function hasOpenHudPopup(): boolean {
  const checkContainer = (el: HTMLElement | null): boolean =>
    Boolean(el?.querySelector('[aria-expanded="true"]'))
  return checkContainer(headerRef.value) || checkContainer(footerRef.value)
}

const isHudLocked = computed(() => props.lockHud || pointerInHud.value || isSheetOpen.value)

function clearHideTimer(): void {
  if (hideTimer !== null) {
    clearTimeout(hideTimer)
    hideTimer = null
  }
}

function setHudVisible(next: boolean): void {
  if (!isControlledHud.value) {
    internalHudVisible.value = next
  }
  emit('update:hudVisible', next)
}

function scheduleAutoHide(): void {
  clearHideTimer()
  if (!props.autoHide || !isHudVisible.value || isHudLocked.value) return
  const delay = Math.max(800, props.autoHideDelayMs)
  hideTimer = setTimeout(() => {
    hideTimer = null
    if (!props.autoHide || isHudLocked.value || hasOpenHudPopup()) {
      scheduleAutoHide()
      return
    }
    setHudVisible(false)
  }, delay)
}

function showHud(): void {
  setHudVisible(true)
  scheduleAutoHide()
}

function hideHud(): void {
  clearHideTimer()
  setHudVisible(false)
}

function toggleHud(): void {
  if (isHudVisible.value) {
    hideHud()
  } else {
    showHud()
  }
}

function resetHudTimer(): void {
  if (isHudVisible.value) {
    scheduleAutoHide()
  }
}

function closeSheet(): void {
  if (props.activeSheet !== null) {
    emit('update:activeSheet', null)
  }
}

function showOsd(
  text: string,
  opts?: {
    icon?: Component
    durationMs?: number
  },
): void {
  if (osdTimer !== null) {
    clearTimeout(osdTimer)
    osdTimer = null
  }
  internalOsdText.value = text
  internalOsdIcon.value = opts?.icon
  osdTimer = setTimeout(() => {
    osdTimer = null
    internalOsdText.value = null
    internalOsdIcon.value = undefined
  }, opts?.durationMs ?? 1200)
}

function handleHudPointerEnter(): void {
  pointerInHud.value = true
  clearHideTimer()
}

function handleHudPointerLeave(): void {
  pointerInHud.value = false
  scheduleAutoHide()
}

function handleStagePointerMove(ev: PointerEvent): void {
  if (ev.pointerType !== 'mouse' || !props.autoHide) return
  if (!isHudVisible.value) {
    showHud()
  } else {
    scheduleAutoHide()
  }
}

function handleStageClick(ev: MouseEvent): void {
  if (props.centerTapMode === 'disabled') return
  const target = ev.target as HTMLElement | null
  if (!target) return
  if (
    headerRef.value?.contains(target) ||
    footerRef.value?.contains(target) ||
    target.closest(INTERACTIVE_SELECTOR)
  ) {
    return
  }

  emit('stage-tap', ev)

  if (props.centerTapMode === 'full-stage') {
    toggleHud()
    return
  }

  const stage = stageRef.value
  if (!stage) {
    toggleHud()
    return
  }
  const rect = stage.getBoundingClientRect()
  if (rect.width <= 0) {
    toggleHud()
    return
  }
  const ratio = (ev.clientX - rect.left) / rect.width
  if (ratio < 0.25) {
    emit('tap-prev')
  } else if (ratio > 0.75) {
    emit('tap-next')
  } else {
    toggleHud()
  }
}

function handleBackClick(): void {
  emit('back')
}

watch(
  [isHudVisible, isHudLocked, () => props.autoHide, () => props.autoHideDelayMs],
  ([visible, locked, auto]) => {
    if (visible && auto && !locked) {
      scheduleAutoHide()
    } else {
      clearHideTimer()
    }
  },
  { immediate: true },
)

onMounted(() => {
  // Lowest priority inside the immersive view: MdSheet/MdMenu/MdDialog push above this when open.
  escapeStack.push(() => {
    if (isSheetOpen.value) {
      closeSheet()
      return true
    }
    if (isHudVisible.value && props.autoHide) {
      hideHud()
      return true
    }
    if (props.escapeToExit) {
      emit('exit')
      emit('back')
      return true
    }
    return false
  })
})

onBeforeUnmount(() => {
  clearHideTimer()
  if (osdTimer !== null) {
    clearTimeout(osdTimer)
    osdTimer = null
  }
})

defineExpose({
  showHud,
  hideHud,
  toggleHud,
  resetHudTimer,
  showOsd,
  closeSheet,
})
</script>

<template>
  <div
    ref="stageRef"
    class="relative flex h-dvh w-screen flex-col overflow-hidden select-none"
    :class="[
      darkScheme ? 'md-scheme-dark' : '',
      resolvedStageBgClass,
      cursorHidden ? 'cursor-none' : '',
    ]"
    @pointermove="handleStagePointerMove"
    @click="handleStageClick"
  >
    <!-- 100vw x 100vh Content Stage -->
    <div class="relative h-full w-full flex-1 overflow-hidden">
      <slot :hud-visible="isHudVisible" :toggle-hud="toggleHud" :show-hud="showHud" />
    </div>

    <!-- Floating Top HUD Bar (budget <= 5 controls; keeps <header> mounted for combobox/title queries) -->
    <header
      ref="headerRef"
      :aria-hidden="!isHudVisible"
      :inert="!isHudVisible"
      class="safe-pt safe-px fixed top-0 right-0 left-0 z-[var(--z-base-overlay)] transition-[opacity,transform] duration-200 ease-standard"
      :class="[
        hudBarClass,
        darkScheme ? 'border-b border-white/10' : 'border-b border-outline-variant/60',
        isHudVisible
          ? 'translate-y-0 opacity-100'
          : 'pointer-events-none -translate-y-2 opacity-0',
      ]"
      @pointerenter="handleHudPointerEnter"
      @pointerleave="handleHudPointerLeave"
      @click.stop
    >
      <div class="flex h-14 items-center justify-between gap-3 px-3 sm:px-4">
        <div class="flex min-w-0 items-center gap-2">
          <MdButton
            v-if="showBack"
            variant="text"
            size="sm"
            :icon="LuArrowLeft"
            :aria-label="resolvedBackLabel"
            class="shrink-0"
            @click="handleBackClick"
          >
            <span class="hidden sm:inline">{{ resolvedBackLabel }}</span>
          </MdButton>

          <div v-if="title || subtitle || $slots['header-meta']" class="min-w-0">
            <div class="flex items-center gap-2">
              <span v-if="title" class="text-title-small truncate text-on-surface">
                {{ title }}
              </span>
              <slot name="header-meta" />
            </div>
            <p v-if="subtitle" class="text-body-small truncate text-on-surface-variant">
              {{ subtitle }}
            </p>
          </div>
        </div>

        <!-- Right HUD Controls (<= 4 items; supports header button[role="combobox"]) -->
        <div
          v-if="$slots['header-controls']"
          class="flex shrink-0 items-center gap-1.5"
        >
          <slot name="header-controls" :hide-hud="hideHud" :reset-timer="resetHudTimer" />
        </div>
      </div>
    </header>

    <!-- Floating Bottom HUD Bar (budget <= 6 controls) -->
    <footer
      v-if="$slots.footer"
      ref="footerRef"
      :aria-hidden="!isHudVisible"
      :inert="!isHudVisible"
      class="safe-pb safe-px fixed right-0 bottom-0 left-0 z-[var(--z-base-overlay)] transition-[opacity,transform] duration-200 ease-standard"
      :class="[
        hudBarClass,
        darkScheme ? 'border-t border-white/10' : 'border-t border-outline-variant/60',
        isHudVisible
          ? 'translate-y-0 opacity-100'
          : 'pointer-events-none translate-y-2 opacity-0',
      ]"
      @pointerenter="handleHudPointerEnter"
      @pointerleave="handleHudPointerLeave"
      @click.stop
    >
      <div class="px-3 py-2.5 sm:px-4">
        <slot name="footer" :hide-hud="hideHud" :reset-timer="resetHudTimer" />
      </div>
    </footer>

    <!-- Transient Centered OSD Feedback (seek / volume / speed / page) -->
    <Transition name="md-fade">
      <div
        v-if="activeOsdText"
        role="status"
        aria-live="polite"
        class="pointer-events-none fixed top-1/2 left-1/2 z-[var(--z-base-overlay)] flex -translate-x-1/2 -translate-y-1/2 items-center gap-2 rounded-corner-full bg-inverse-surface/92 px-4 py-2 text-inverse-on-surface shadow-elevation-2"
      >
        <component :is="activeOsdIcon" v-if="activeOsdIcon" class="h-4 w-4 shrink-0" aria-hidden="true" />
        <span class="text-label-large whitespace-nowrap tabular-nums">{{ activeOsdText }}</span>
      </div>
    </Transition>

    <!-- Optional Stage Overlay (buffering / error / playback prompt) -->
    <slot name="overlay" />

    <!-- Single Mutually-Exclusive Sheet (Side Sheet >=600px / Bottom Sheet <600px) -->
    <MdSheet
      v-if="$slots.sheet"
      :open="isSheetOpen"
      :title="sheetTitle"
      :subtitle="sheetSubtitle"
      :size="sheetSize"
      :dark-scheme="darkScheme"
      :teleport-to="sheetTeleportTo"
      @close="closeSheet"
    >
      <template v-if="$slots['sheet-header-actions']" #headerActions>
        <slot name="sheet-header-actions" :active-sheet="activeSheet" :close="closeSheet" />
      </template>
      <template v-if="$slots['sheet-subheader']" #subheader>
        <slot name="sheet-subheader" :active-sheet="activeSheet" :close="closeSheet" />
      </template>

      <slot name="sheet" :active-sheet="activeSheet" :close="closeSheet" />

      <template v-if="$slots['sheet-footer']" #footer>
        <slot name="sheet-footer" :active-sheet="activeSheet" :close="closeSheet" />
      </template>
    </MdSheet>
  </div>
</template>
