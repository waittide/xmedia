<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuArrowLeft, LuSkipForward } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useMusicPlayerStore } from '@/stores/musicPlayer'
import { useThemeStore } from '@/stores/theme'
import type { VideoDetailDto, VideoQualityProfile } from '@/types/api'

const route = useRoute()
const router = useRouter()
const theme = useThemeStore()
const musicPlayer = useMusicPlayerStore()

const videoEl = ref<HTMLVideoElement | null>(null)
const detail = ref<VideoDetailDto | null>(null)
const quality = ref<VideoQualityProfile>(theme.video.default_quality)
const selectedAudio = ref<number | null>(null)
const selectedSubtitle = ref<string>('')
const startOffsetSecs = ref(0)

let progressTimer: ReturnType<typeof setInterval> | null = null

const streamSrc = computed(() => {
  if (!detail.value) return ''
  return api.videoStreamUrl(detail.value.video.id, {
    quality: quality.value,
    audioStream: selectedAudio.value,
    startSecs: startOffsetSecs.value,
  })
})

const isDirectStream = computed(() => {
  if (!detail.value) return true
  if (quality.value === 'direct') return startOffsetSecs.value === 0 && selectedAudio.value === null
  if (quality.value === 'auto') {
    return (
      detail.value.video.can_direct_play &&
      startOffsetSecs.value === 0 &&
      selectedAudio.value === null
    )
  }
  return false
})

async function loadVideo(id: string): Promise<void> {
  musicPlayer.pause()
  const d = await api.getVideo(id)
  detail.value = d
  selectedAudio.value = d.video.progress?.selected_audio_stream ?? null
  selectedSubtitle.value =
    d.video.progress?.selected_subtitle ?? d.subtitles.find((s) => s.is_default)?.id ?? ''

  const savedPos =
    d.video.progress && !d.video.progress.completed ? d.video.progress.position_secs : 0
  if (d.video.can_direct_play && (quality.value === 'auto' || quality.value === 'direct')) {
    startOffsetSecs.value = 0
    setTimeout(() => {
      if (videoEl.value && savedPos > 2) {
        videoEl.value.currentTime = savedPos
      }
    }, 250)
  } else {
    startOffsetSecs.value = Math.floor(savedPos)
  }
}

function reportProgress(completed?: boolean): void {
  if (!detail.value || !videoEl.value) return
  const pos = startOffsetSecs.value + (videoEl.value.currentTime || 0)
  const dur =
    detail.value.video.duration_secs ||
    (Number.isFinite(videoEl.value.duration) ? Math.round(videoEl.value.duration) : 0)
  void api
    .saveVideoProgress(detail.value.video.id, {
      position_secs: pos,
      duration_secs: dur > 0 ? dur : undefined,
      completed,
      selected_audio_stream: selectedAudio.value,
      selected_subtitle: selectedSubtitle.value || null,
    })
    .catch(() => {})
}

function onQualityOrAudioChange(): void {
  if (!videoEl.value) return
  const currentPos = startOffsetSecs.value + (videoEl.value.currentTime || 0)
  if (!isDirectStream.value) {
    startOffsetSecs.value = Math.floor(currentPos)
  } else {
    startOffsetSecs.value = 0
    setTimeout(() => {
      if (videoEl.value) videoEl.value.currentTime = currentPos
    }, 200)
  }
}

function onEnded(): void {
  reportProgress(true)
  if (theme.video.autoplay_next && detail.value?.next_episode_id) {
    void router.push(`/videos/watch/${detail.value.next_episode_id}`)
  }
}

onMounted(() => {
  void loadVideo(String(route.params.id))
  progressTimer = setInterval(() => {
    if (videoEl.value && !videoEl.value.paused) {
      reportProgress()
    }
  }, 8000)
})

watch(
  () => route.params.id,
  (nextId) => {
    if (nextId) void loadVideo(String(nextId))
  },
)

onBeforeUnmount(() => {
  reportProgress()
  if (progressTimer) clearInterval(progressTimer)
})
</script>

<template>
  <div v-if="detail" class="flex h-full w-full flex-col bg-black text-white overflow-hidden">
    <!-- Top Bar -->
    <div class="flex h-12 shrink-0 items-center justify-between border-b border-white/10 bg-zinc-900 px-4 text-xs">
      <div class="flex items-center gap-3 min-w-0">
        <button
          type="button"
          class="inline-flex items-center gap-1 rounded px-2 py-1 hover:bg-white/10"
          @click="
            detail.video.show_id
              ? router.push(`/videos/shows/${detail.video.show_id}`)
              : router.push('/videos')
          "
        >
          <LuArrowLeft class="h-4 w-4" /> Back
        </button>
        <span class="truncate font-semibold">{{ detail.video.title }}</span>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <select
          v-model="quality"
          class="rounded bg-zinc-800 px-2 py-1 text-xs"
          @change="onQualityOrAudioChange"
        >
          <option value="auto">Auto (Direct / Remux)</option>
          <option value="direct">Direct Raw</option>
          <option value="p1080">1080p H.264</option>
          <option value="p720">720p H.264</option>
          <option value="p480">480p H.264</option>
        </select>

        <select
          v-if="detail.audio_streams.length > 1"
          v-model="selectedAudio"
          class="rounded bg-zinc-800 px-2 py-1 text-xs"
          @change="onQualityOrAudioChange"
        >
          <option :value="null">Default Audio</option>
          <option v-for="a in detail.audio_streams" :key="a.stream_index" :value="a.stream_index">
            #{{ a.stream_index }} {{ a.language || a.codec }} ({{ a.channels }}ch)
          </option>
        </select>

        <select
          v-if="detail.subtitles.length > 0"
          v-model="selectedSubtitle"
          class="rounded bg-zinc-800 px-2 py-1 text-xs"
        >
          <option value="">Subtitles Off</option>
          <option v-for="s in detail.subtitles" :key="s.id" :value="s.id">
            {{ s.label }} ({{ s.source_kind }})
          </option>
        </select>

        <button
          v-if="detail.next_episode_id"
          type="button"
          class="inline-flex items-center gap-1 rounded bg-[var(--ctp-accent)] px-2.5 py-1 font-semibold text-[var(--ctp-on-accent)]"
          @click="router.push(`/videos/watch/${detail.next_episode_id}`)"
        >
          Next Ep <LuSkipForward class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>

    <!-- Video Element -->
    <div class="flex-1 min-h-0 flex items-center justify-center bg-black">
      <video
        ref="videoEl"
        :src="streamSrc"
        controls
        autoplay
        crossorigin="use-credentials"
        class="max-h-full max-w-full"
        @play="musicPlayer.pause()"
        @pause="reportProgress()"
        @ended="onEnded"
      >
        <track
          v-if="selectedSubtitle"
          :key="selectedSubtitle"
          kind="subtitles"
          :src="api.videoSubtitleUrl(detail.video.id, selectedSubtitle)"
          default
        />
      </video>
    </div>
  </div>
</template>
