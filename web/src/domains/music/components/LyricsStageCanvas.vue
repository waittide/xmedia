<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { LuMic2 } from 'vue-icons-plus/lu'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'

const props = defineProps<{
  showTranslation: boolean
  active: boolean
}>()

const { t } = useI18n()
const playerStore = usePlayerStore()

const lyricsContainerRef = ref<HTMLDivElement | null>(null)
const lineRefs = ref<(HTMLButtonElement | null)[]>([])
const autoScrollPaused = ref(false)
let autoScrollTimer: ReturnType<typeof setTimeout> | null = null

function clearAutoScrollPause(): void {
  autoScrollPaused.value = false
  if (autoScrollTimer) {
    clearTimeout(autoScrollTimer)
    autoScrollTimer = null
  }
}

function pauseAutoScroll(): void {
  autoScrollPaused.value = true
  if (autoScrollTimer) clearTimeout(autoScrollTimer)
  autoScrollTimer = setTimeout(() => {
    autoScrollPaused.value = false
    autoScrollTimer = null
  }, 4000)
}

function setLineRef(el: unknown, idx: number): void {
  lineRefs.value[idx] = el instanceof HTMLButtonElement ? el : null
}

const matchedTranslations = computed<Array<string | null>>(() => {
  const primary = playerStore.lyrics
  const translated = playerStore.translatedLyrics
  if (!primary?.synced || !translated?.lines.length) {
    return []
  }
  const tLines = translated.lines
  return primary.lines.map((line) => {
    let best: string | null = null
    let bestDiff = 450
    for (const candidate of tLines) {
      const diff = Math.abs(candidate.start_ms - line.start_ms)
      if (diff <= bestDiff && candidate.text.trim()) {
        bestDiff = diff
        best = candidate.text.trim()
      }
    }
    return best
  })
})

async function scrollToActiveLyric(behavior: ScrollBehavior = 'smooth'): Promise<void> {
  const activeIdx = playerStore.activeLyricIndex
  if (!props.active || activeIdx < 0 || autoScrollPaused.value) return
  await nextTick()
  const container = lyricsContainerRef.value
  const targetEl = lineRefs.value[activeIdx]
  if (!container || !targetEl) return
  const containerRect = container.getBoundingClientRect()
  const targetRect = targetEl.getBoundingClientRect()
  const nextScrollTop =
    container.scrollTop +
    (targetRect.top - containerRect.top) -
    container.clientHeight / 2 +
    targetRect.height / 2
  container.scrollTo({
    top: Math.max(0, nextScrollTop),
    behavior,
  })
}

watch(
  () => [playerStore.currentTrack?.id, playerStore.lyrics] as const,
  () => {
    lineRefs.value = []
    clearAutoScrollPause()
    if (lyricsContainerRef.value) {
      lyricsContainerRef.value.scrollTop = 0
    }
  },
)

watch(
  () => [playerStore.activeLyricIndex, props.active, playerStore.lyrics] as const,
  ([activeIdx, isActive]) => {
    if (!isActive || activeIdx < 0) return
    void scrollToActiveLyric('smooth')
  },
)

function handleLyricLineClick(startMs: number): void {
  clearAutoScrollPause()
  playerStore.seekTo(startMs / 1000)
}

onBeforeUnmount(clearAutoScrollPause)
</script>

<template>
  <div
    ref="lyricsContainerRef"
    class="lyrics-feather min-h-0 flex-1 overflow-y-auto px-1 py-12 sm:px-4"
    @wheel.passive="pauseAutoScroll"
    @touchstart.passive="pauseAutoScroll"
  >
    <div
      v-if="playerStore.lyricsLoading"
      class="text-body-medium flex h-full items-center justify-center text-on-surface-variant"
    >
      {{ t('common.loading') }}
    </div>

    <!-- Synced Interactive Lyrics (MUST stay <button> for T1-F15-03) -->
    <div
      v-else-if="
        playerStore.lyrics &&
        playerStore.lyrics.synced &&
        playerStore.lyrics.lines.length > 0
      "
      class="space-y-4 py-16"
    >
      <button
        v-for="(line, idx) in playerStore.lyrics.lines"
        :key="`${line.start_ms}-${idx}`"
        :ref="(el) => setLineRef(el, idx)"
        type="button"
        class="block w-full origin-left cursor-pointer rounded-corner-md px-3 py-2 text-left transition-all duration-200 ease-standard hover:bg-on-surface/5 focus-visible:outline-2 focus-visible:outline-secondary"
        :class="
          idx === playerStore.activeLyricIndex
            ? 'scale-[1.03] text-headline-small text-on-surface sm:text-headline-medium'
            : 'text-title-medium text-on-surface-variant/80 hover:text-on-surface sm:text-title-large'
        "
        @click="handleLyricLineClick(line.start_ms)"
      >
        <span class="block leading-snug">{{ line.text || '♪' }}</span>
        <span
          v-if="showTranslation && matchedTranslations[idx]"
          class="mt-1 block text-body-medium leading-snug"
          :class="
            idx === playerStore.activeLyricIndex
              ? 'text-on-surface-variant'
              : 'text-on-surface-variant/75'
          "
        >
          {{ matchedTranslations[idx] }}
        </span>
      </button>
    </div>

    <div
      v-else-if="playerStore.lyrics && playerStore.lyrics.plain_text"
      class="text-body-large space-y-6 py-8 leading-loose whitespace-pre-line text-on-surface/90"
    >
      <p>{{ playerStore.lyrics.plain_text }}</p>
      <p
        v-if="showTranslation && playerStore.translatedLyrics?.plain_text"
        class="text-body-medium border-t border-outline-variant pt-6 text-on-surface-variant"
      >
        {{ playerStore.translatedLyrics.plain_text }}
      </p>
    </div>

    <div
      v-else
      class="text-body-medium flex h-full flex-col items-center justify-center gap-2 text-center text-on-surface-variant"
    >
      <LuMic2 class="h-8 w-8 opacity-40" aria-hidden="true" />
      <p>{{ t('player.noLyrics') }}</p>
    </div>
  </div>
</template>

<style scoped>
.lyrics-feather {
  mask-image: linear-gradient(
    to bottom,
    transparent 0%,
    var(--md-sys-color-scrim) 10%,
    var(--md-sys-color-scrim) 90%,
    transparent 100%
  );
  -webkit-mask-image: linear-gradient(
    to bottom,
    transparent 0%,
    var(--md-sys-color-scrim) 10%,
    var(--md-sys-color-scrim) 90%,
    transparent 100%
  );
}
</style>
