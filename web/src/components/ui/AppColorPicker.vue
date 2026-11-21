<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { normalizeHex, parseHexRgb, rgbToHex } from '@/theme/catppuccin'

const props = defineProps<{
  modelValue: string
}>()

const emit = defineEmits<{
  'update:modelValue': [hex: string]
}>()

const { t } = useI18n()
const escapeStack = useEscapeStack()

const open = ref(false)
const triggerRef = ref<HTMLButtonElement | null>(null)

escapeStack.register(
  () => open.value,
  () => {
    open.value = false
    triggerRef.value?.focus()
  },
)
const panelRef = ref<HTMLDivElement | null>(null)
const svBoxRef = ref<HTMLDivElement | null>(null)
const hueBarRef = ref<HTMLDivElement | null>(null)
const panelStyle = ref<Record<string, string>>({})

const hue = ref(215)
const sat = ref(0.75)
const val = ref(0.95)
const hexDraft = ref(props.modelValue)

const QUICK_SWATCHES = [
  '#1e66f5',
  '#89b4fa',
  '#7287fd',
  '#cba6f7',
  '#ea76cb',
  '#d20f39',
  '#fe640b',
  '#df8e1d',
  '#40a02b',
  '#179299',
  '#eff1f5',
  '#1e1e2e',
  '#313244',
  '#cdd6f4',
]

function rgbToHsv(r: number, g: number, b: number): [number, number, number] {
  const rn = r / 255
  const gn = g / 255
  const bn = b / 255
  const max = Math.max(rn, gn, bn)
  const min = Math.min(rn, gn, bn)
  const d = max - min
  let h = 0
  if (d !== 0) {
    if (max === rn) h = ((gn - bn) / d + (gn < bn ? 6 : 0)) * 60
    else if (max === gn) h = ((bn - rn) / d + 2) * 60
    else h = ((rn - gn) / d + 4) * 60
  }
  const s = max === 0 ? 0 : d / max
  return [h, s, max]
}

function hsvToHex(h: number, s: number, v: number): string {
  const c = v * s
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1))
  const m = v - c
  let r1 = 0
  let g1 = 0
  let b1 = 0
  if (h < 60) [r1, g1, b1] = [c, x, 0]
  else if (h < 120) [r1, g1, b1] = [x, c, 0]
  else if (h < 180) [r1, g1, b1] = [0, c, x]
  else if (h < 240) [r1, g1, b1] = [0, x, c]
  else if (h < 300) [r1, g1, b1] = [x, 0, c]
  else [r1, g1, b1] = [c, 0, x]
  return rgbToHex((r1 + m) * 255, (g1 + m) * 255, (b1 + m) * 255)
}

function syncFromHex(hex: string): void {
  const [r, g, b] = parseHexRgb(hex)
  const [h, s, v] = rgbToHsv(r, g, b)
  if (s > 0.01) hue.value = h
  sat.value = s
  val.value = v
  hexDraft.value = hex
}

watch(
  () => props.modelValue,
  (next) => {
    syncFromHex(next)
  },
  { immediate: true },
)

const pureHueHex = computed(() => hsvToHex(hue.value, 1, 1))

function updatePosition(): void {
  if (!open.value) return
  const el = triggerRef.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const vw = window.innerWidth
  const vh = window.innerHeight
  const width = 248
  const estHeight = 276
  const left = Math.min(rect.left, Math.max(8, vw - width - 8))
  const openUp = rect.bottom + estHeight + 12 > vh && rect.top > estHeight
  panelStyle.value = {
    position: 'fixed',
    width: `${width}px`,
    left: `${Math.round(left)}px`,
    ...(openUp
      ? { bottom: `${Math.round(vh - rect.top + 6)}px` }
      : { top: `${Math.round(rect.bottom + 6)}px` }),
  }
}

async function toggleOpen(): Promise<void> {
  open.value = !open.value
  if (open.value) {
    syncFromHex(props.modelValue)
    updatePosition()
    await nextTick()
    updatePosition()
  }
}

function emitHsv(): void {
  const hex = hsvToHex(hue.value, sat.value, val.value)
  hexDraft.value = hex
  emit('update:modelValue', hex)
}

let draggingSv = false
function onSvPointerDown(ev: PointerEvent): void {
  ev.preventDefault()
  svBoxRef.value?.setPointerCapture(ev.pointerId)
  draggingSv = true
  updateSvFromEvent(ev)
}
function onSvPointerMove(ev: PointerEvent): void {
  if (!draggingSv) return
  updateSvFromEvent(ev)
}
function onSvPointerUp(ev: PointerEvent): void {
  if (!draggingSv) return
  draggingSv = false
  svBoxRef.value?.releasePointerCapture(ev.pointerId)
}
function updateSvFromEvent(ev: PointerEvent): void {
  const box = svBoxRef.value
  if (!box) return
  const rect = box.getBoundingClientRect()
  sat.value = Math.max(0, Math.min(1, (ev.clientX - rect.left) / rect.width))
  val.value = Math.max(0, Math.min(1, 1 - (ev.clientY - rect.top) / rect.height))
  emitHsv()
}

let draggingHue = false
function onHuePointerDown(ev: PointerEvent): void {
  ev.preventDefault()
  hueBarRef.value?.setPointerCapture(ev.pointerId)
  draggingHue = true
  updateHueFromEvent(ev)
}
function onHuePointerMove(ev: PointerEvent): void {
  if (!draggingHue) return
  updateHueFromEvent(ev)
}
function onHuePointerUp(ev: PointerEvent): void {
  if (!draggingHue) return
  draggingHue = false
  hueBarRef.value?.releasePointerCapture(ev.pointerId)
}
function updateHueFromEvent(ev: PointerEvent): void {
  const bar = hueBarRef.value
  if (!bar) return
  const rect = bar.getBoundingClientRect()
  const ratio = Math.max(0, Math.min(1, (ev.clientX - rect.left) / rect.width))
  hue.value = Math.min(359.9, ratio * 360)
  emitHsv()
}

function commitHexDraft(): void {
  const normalized = normalizeHex(hexDraft.value, props.modelValue)
  syncFromHex(normalized)
  emit('update:modelValue', normalized)
}

function selectSwatch(hex: string): void {
  syncFromHex(hex)
  emit('update:modelValue', hex)
}

function handleOutside(ev: MouseEvent): void {
  if (!open.value) return
  const target = ev.target as Node
  if (triggerRef.value?.contains(target) || panelRef.value?.contains(target)) return
  open.value = false
}

function bindGlobalListeners(): void {
  document.addEventListener('mousedown', handleOutside)
  window.addEventListener('resize', updatePosition)
  window.addEventListener('scroll', updatePosition, true)
}

function unbindGlobalListeners(): void {
  document.removeEventListener('mousedown', handleOutside)
  window.removeEventListener('resize', updatePosition)
  window.removeEventListener('scroll', updatePosition, true)
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
    :title="t('theme.pickColor')"
    class="inline-flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-[var(--md-sys-shape-corner-sm)] border border-[var(--md-sys-color-outline)] bg-[var(--md-sys-color-surface)] p-0.5 transition hover:border-[var(--md-sys-color-primary)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--md-sys-color-primary)]"
    @click="toggleOpen"
  >
    <span
      class="h-full w-full rounded-[5px] border border-black/10"
      :style="{ backgroundColor: modelValue }"
    />
  </button>

  <Teleport to="body">
    <div
      v-if="open"
      ref="panelRef"
      :style="panelStyle"
      class="z-[var(--z-popover)] rounded-[var(--md-sys-shape-corner-md)] bg-[var(--md-sys-color-surface-container)] p-3 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-2)] ring-1 ring-[var(--md-sys-color-outline-variant)]/40"
    >
      <!-- 2D Saturation / Value Canvas -->
      <div
        ref="svBoxRef"
        class="relative h-32 w-full cursor-crosshair overflow-hidden rounded-[var(--md-sys-shape-corner-sm)] select-none touch-none"
        :style="{ backgroundColor: pureHueHex }"
        @pointerdown="onSvPointerDown"
        @pointermove="onSvPointerMove"
        @pointerup="onSvPointerUp"
        @pointercancel="onSvPointerUp"
      >
        <div class="absolute inset-0 bg-gradient-to-r from-white to-transparent" />
        <div class="absolute inset-0 bg-gradient-to-t from-black to-transparent" />
        <span
          class="pointer-events-none absolute h-3.5 w-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow-md"
          :style="{ left: `${sat * 100}%`, top: `${(1 - val) * 100}%` }"
        />
      </div>

      <!-- Hue Rainbow Slider -->
      <div
        ref="hueBarRef"
        class="relative mt-2.5 h-3 w-full cursor-pointer rounded-full select-none touch-none"
        style="background: linear-gradient(to right, #f00 0%, #ff0 17%, #0f0 33%, #0ff 50%, #00f 67%, #f0f 83%, #f00 100%)"
        @pointerdown="onHuePointerDown"
        @pointermove="onHuePointerMove"
        @pointerup="onHuePointerUp"
        @pointercancel="onHuePointerUp"
      >
        <span
          class="pointer-events-none absolute top-1/2 h-4 w-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white shadow"
          :style="{ left: `${(hue / 360) * 100}%`, backgroundColor: pureHueHex }"
        />
      </div>

      <!-- Quick Swatches -->
      <div class="mt-2.5 grid grid-cols-7 gap-1.5">
        <button
          v-for="sw in QUICK_SWATCHES"
          :key="sw"
          type="button"
          class="h-5 w-5 cursor-pointer rounded-full border border-black/15 transition hover:scale-110"
          :style="{ backgroundColor: sw }"
          @click="selectSwatch(sw)"
        />
      </div>

      <!-- Hex Input -->
      <div class="mt-2.5 flex items-center gap-2">
        <span class="md-label-small text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('theme.hexInputLabel') }}
        </span>
        <input
          v-model="hexDraft"
          type="text"
          maxlength="7"
          class="md-input h-7 min-w-0 flex-1 !px-2 font-mono text-xs"
          @change="commitHexDraft"
        />
      </div>
    </div>
  </Teleport>
</template>
