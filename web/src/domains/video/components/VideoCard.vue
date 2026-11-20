<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuPlay,
  LuHeart,
  LuMoreHorizontal,
  LuCheck,
  LuPlus,
  LuPencil,
  LuDownload,
  LuTrash2,
} from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { api, buildVideoDownloadUrl } from '../api'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { VideoSummaryDto } from '../types'
import PosterImage from './PosterImage.vue'

const props = withDefaults(
  defineProps<{
    video: VideoSummaryDto
    removableFromShelf?: boolean
  }>(),
  {
    removableFromShelf: false,
  },
)

const emit = defineEmits<{
  edit: [video: VideoSummaryDto]
  deleted: [videoId: string]
  removeFromShelf: [videoId: string]
  updated: [video: VideoSummaryDto]
}>()

const { t } = useI18n()
const router = useRouter()
const authStore = useAuthStore()
const videoStore = useVideoStore()
const dialogStore = useDialogStore()

const menuOpen = ref(false)
const menuPos = ref<{ top: string; left: string }>({ top: '0px', left: '0px' })
const menuRef = ref<HTMLDivElement | null>(null)

function formatDuration(secs: number): string {
  if (!secs || secs <= 0) return ''
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = Math.floor(secs % 60)
  if (h > 0) {
    return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  }
  return `${m}:${String(s).padStart(2, '0')}`
}

const progressPercent = computed(() => {
  const p = props.video.progress
  if (!p) return 0
  if (p.completed) return 100
  const dur = Math.max(1, props.video.duration_secs || p.duration_secs || 1)
  return Math.min(100, Math.round((p.position_secs / dur) * 100))
})

const episodeTag = computed(() => {
  if (props.video.season_number != null && props.video.episode_number != null) {
    return `S${String(props.video.season_number).padStart(2, '0')}E${String(props.video.episode_number).padStart(2, '0')}`
  }
  if (props.video.episode_number != null) {
    return `EP ${props.video.episode_number}`
  }
  return null
})

function openMenu(ev: MouseEvent): void {
  ev.stopPropagation()
  if (menuOpen.value) {
    menuOpen.value = false
    return
  }
  const rect = (ev.currentTarget as HTMLElement).getBoundingClientRect()
  const vw = window.innerWidth
  const vh = window.innerHeight
  const menuW = 220
  const menuH = 250
  const left = Math.min(rect.right - menuW, Math.max(8, vw - menuW - 8))
  const top = rect.bottom + menuH > vh ? Math.max(8, rect.top - menuH) : rect.bottom + 4
  menuPos.value = { top: `${Math.round(top)}px`, left: `${Math.round(left)}px` }
  menuOpen.value = true
}

async function toggleWatched(): Promise<void> {
  menuOpen.value = false
  const nextWatched = !(props.video.progress?.completed ?? false)
  const prog = await api.setWatched(props.video.id, nextWatched)
  const copy: VideoSummaryDto = { ...props.video, progress: prog }
  emit('updated', copy)
  videoStore.bumpCatalogVersion()
}

async function createShelfWithVideo(): Promise<void> {
  menuOpen.value = false
  const name = await dialogStore.prompt({
    title: t('shelf.createTitle'),
    message: t('shelf.createPrompt'),
    placeholder: t('shelf.namePlaceholder'),
  })
  if (!name || !name.trim()) return
  await api.createShelf({ name: name.trim(), video_ids: [props.video.id] })
  await videoStore.refreshSidebar()
}

async function addToShelf(shelfId: string): Promise<void> {
  menuOpen.value = false
  await videoStore.addVideoToShelf(shelfId, props.video.id)
}

async function confirmDelete(): Promise<void> {
  menuOpen.value = false
  const ok = await dialogStore.confirm({
    title: t('common.delete'),
    message: t('metadata.deleteVideoConfirm', { title: props.video.title }),
    danger: true,
  })
  if (!ok) return
  await api.deleteVideo(props.video.id)
  await videoStore.refreshSidebar()
  videoStore.bumpCatalogVersion()
  emit('deleted', props.video.id)
}

function handleOutside(ev: MouseEvent): void {
  if (!menuOpen.value) return
  if (menuRef.value?.contains(ev.target as Node)) return
  menuOpen.value = false
}

watch(menuOpen, (isOpen) => {
  if (isOpen) document.addEventListener('mousedown', handleOutside)
  else document.removeEventListener('mousedown', handleOutside)
})
onBeforeUnmount(() => document.removeEventListener('mousedown', handleOutside))
</script>

<template>
  <div
    class="group flex cursor-pointer flex-col rounded-2xl p-2.5 transition hover:bg-[var(--ctp-mantle)]"
    @click="router.push(`/video/watch/${video.id}`)"
  >
    <div class="relative mb-2.5">
      <PosterImage
        :poster-key="video.poster_key"
        :size="560"
        :alt="video.title"
        aspect="video"
        rounded="rounded-2xl"
        class="album-shadow"
      >
        <!-- Top Badges -->
        <div class="pointer-events-none absolute top-2 left-2 flex items-center gap-1.5">
          <span
            v-if="video.resolution"
            class="rounded-md bg-black/65 px-1.5 py-0.5 font-mono text-[10px] font-bold text-white backdrop-blur-xs"
          >
            {{ video.resolution }}
          </span>
          <span
            v-if="episodeTag"
            class="rounded-md bg-[var(--ctp-accent)] px-1.5 py-0.5 font-mono text-[10px] font-bold text-[var(--ctp-on-accent)] shadow-xs"
          >
            {{ episodeTag }}
          </span>
          <span
            v-if="video.progress?.completed"
            class="inline-flex items-center gap-0.5 rounded-md bg-[var(--ctp-green)]/90 px-1.5 py-0.5 text-[10px] font-bold text-black"
          >
            <LuCheck class="h-3 w-3" />
          </span>
        </div>

        <!-- Duration Pill -->
        <span
          v-if="video.duration_secs > 0"
          class="pointer-events-none absolute right-2 bottom-2.5 rounded-md bg-black/70 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-white backdrop-blur-xs"
        >
          {{ formatDuration(video.duration_secs) }}
        </span>

        <!-- Hover Overlay -->
        <div
          class="absolute inset-0 flex items-center justify-center bg-black/35 opacity-0 transition-opacity duration-150 group-hover:opacity-100"
        >
          <button
            type="button"
            class="flex h-11 w-11 items-center justify-center rounded-full bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-lg transition hover:scale-105"
            :title="t('common.play')"
          >
            <LuPlay class="ml-0.5 h-5 w-5 fill-current" />
          </button>

          <button
            type="button"
            class="absolute top-2 right-2 flex h-7 w-7 items-center justify-center rounded-full bg-black/50 text-white backdrop-blur-xs transition hover:scale-105"
            @click.stop="videoStore.toggleVideoFavorite(video)"
          >
            <LuHeart
              class="h-3.5 w-3.5"
              :class="video.is_favorite ? 'fill-[var(--ctp-red)] text-[var(--ctp-red)]' : ''"
            />
          </button>
        </div>

        <!-- Watch Progress Bar -->
        <div
          v-if="progressPercent > 0"
          class="pointer-events-none absolute right-0 bottom-0 left-0 h-1 bg-black/45"
        >
          <div
            class="h-full bg-[var(--ctp-accent)]"
            :style="{ width: `${progressPercent}%` }"
          />
        </div>
      </PosterImage>
    </div>

    <div class="flex items-start justify-between gap-1.5">
      <div class="min-w-0 flex-1">
        <h3 class="truncate text-xs font-bold text-[var(--ctp-text)]">
          {{ video.title }}
        </h3>
        <div class="mt-0.5 flex items-center gap-1.5 text-[11px] text-[var(--ctp-subtext0)]">
          <span
            v-if="video.show_id && video.show_title"
            class="truncate hover:text-[var(--ctp-accent)] hover:underline"
            @click.stop="router.push(`/video/shows/${video.show_id}`)"
          >
            {{ video.show_title }}
          </span>
          <span v-else class="uppercase font-mono text-[10px]">
            {{ video.container }}
          </span>
          <span v-if="video.year" class="shrink-0">· {{ video.year }}</span>
        </div>
      </div>

      <button
        type="button"
        class="shrink-0 rounded-lg p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
        @click="openMenu"
      >
        <LuMoreHorizontal class="h-4 w-4" />
      </button>
    </div>
  </div>

  <Teleport to="body">
    <div
      v-if="menuOpen"
      ref="menuRef"
      :style="menuPos"
      class="fixed z-[115] w-56 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-1.5 text-xs text-[var(--ctp-text)] shadow-2xl"
    >
      <button
        type="button"
        class="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left transition hover:bg-[var(--ctp-mantle)]"
        @click="toggleWatched"
      >
        <LuCheck class="h-3.5 w-3.5 text-[var(--ctp-green)]" />
        <span>
          {{ video.progress?.completed ? t('common.markUnwatched') : t('common.markWatched') }}
        </span>
      </button>

      <button
        v-if="authStore.isAdmin"
        type="button"
        class="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left transition hover:bg-[var(--ctp-mantle)]"
        @click="menuOpen = false; emit('edit', video)"
      >
        <LuPencil class="h-3.5 w-3.5 text-[var(--ctp-subtext0)]" />
        <span>{{ t('common.editMetadata') }}</span>
      </button>

      <button
        type="button"
        class="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left transition hover:bg-[var(--ctp-mantle)]"
        @click="createShelfWithVideo"
      >
        <LuPlus class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
        <span>{{ t('nav.newShelf') }}</span>
      </button>

      <div
        v-if="videoStore.shelves.length > 0"
        class="my-1 max-h-28 overflow-y-auto border-t border-[var(--ctp-surface0)]/60 pt-1"
      >
        <button
          v-for="sh in videoStore.shelves"
          :key="sh.id"
          type="button"
          class="flex w-full items-center justify-between rounded-xl px-3 py-1.5 text-left text-[11px] text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]"
          @click="addToShelf(sh.id)"
        >
          <span class="truncate">+ {{ sh.name }}</span>
          <span class="font-mono text-[10px] text-[var(--ctp-subtext0)]">{{ sh.video_count }}</span>
        </button>
      </div>

      <a
        :href="buildVideoDownloadUrl(video.id)"
        class="mt-0.5 flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left transition hover:bg-[var(--ctp-mantle)]"
        @click="menuOpen = false"
      >
        <LuDownload class="h-3.5 w-3.5 text-[var(--ctp-subtext0)]" />
        <span>{{ t('common.download') }}</span>
      </a>

      <button
        v-if="removableFromShelf"
        type="button"
        class="mt-0.5 flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-[var(--ctp-red)] transition hover:bg-[var(--ctp-red)]/10"
        @click="menuOpen = false; emit('removeFromShelf', video.id)"
      >
        <LuTrash2 class="h-3.5 w-3.5" />
        <span>{{ t('shelf.removeVideo') }}</span>
      </button>

      <button
        v-if="authStore.isAdmin"
        type="button"
        class="mt-0.5 flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-[var(--ctp-red)] transition hover:bg-[var(--ctp-red)]/10"
        @click="confirmDelete"
      >
        <LuTrash2 class="h-3.5 w-3.5" />
        <span>{{ t('common.delete') }}</span>
      </button>
    </div>
  </Teleport>
</template>
