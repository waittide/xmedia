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
import { formatDuration } from '@/utils/format'
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

function handleMenuKeydown(ev: KeyboardEvent): void {
  if (ev.key === 'Escape' && menuOpen.value) {
    ev.stopPropagation()
    menuOpen.value = false
  }
}

watch(menuOpen, (isOpen) => {
  if (isOpen) {
    document.addEventListener('mousedown', handleOutside)
    window.addEventListener('keydown', handleMenuKeydown)
  } else {
    document.removeEventListener('mousedown', handleOutside)
    window.removeEventListener('keydown', handleMenuKeydown)
  }
})
onBeforeUnmount(() => {
  document.removeEventListener('mousedown', handleOutside)
  window.removeEventListener('keydown', handleMenuKeydown)
})
</script>

<template>
  <div
    role="link"
    tabindex="0"
    :aria-label="video.title"
    class="group flex cursor-pointer flex-col rounded-2xl p-2.5 transition hover:bg-[var(--md-sys-color-surface-container-high)] focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-[var(--md-sys-color-secondary)]"
    @click="router.push(`/video/watch/${video.id}`)"
    @keydown.enter.self="router.push(`/video/watch/${video.id}`)"
    @keydown.space.self.prevent="router.push(`/video/watch/${video.id}`)"
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
            class="rounded-md bg-[var(--md-sys-color-primary)] px-1.5 py-0.5 font-mono text-[10px] font-bold text-[var(--md-sys-color-on-primary)] shadow-xs"
          >
            {{ episodeTag }}
          </span>
          <span
            v-if="video.progress?.completed"
            class="inline-flex items-center gap-0.5 rounded-md bg-[var(--md-sys-color-success-container)] px-1.5 py-0.5 text-[10px] font-bold text-[var(--md-sys-color-on-success-container)]"
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
            class="flex h-11 w-11 items-center justify-center rounded-full bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)] shadow-[var(--md-sys-elevation-2)] transition hover:scale-105"
            :title="t('common.play')"
            :aria-label="t('common.play')"
          >
            <LuPlay class="ml-0.5 h-5 w-5 fill-current" />
          </button>

          <button
            type="button"
            class="absolute top-2 right-2 flex h-7 w-7 items-center justify-center rounded-full bg-black/50 text-white backdrop-blur-xs transition hover:scale-105"
            :aria-label="video.is_favorite ? t('common.unfavorite') : t('common.favorite')"
            :aria-pressed="video.is_favorite"
            @click.stop="videoStore.toggleVideoFavorite(video)"
          >
            <LuHeart
              class="h-3.5 w-3.5"
              :class="video.is_favorite ? 'fill-[var(--md-sys-color-error)] text-[var(--md-sys-color-error)]' : ''"
            />
          </button>
        </div>

        <!-- Watch Progress Bar -->
        <div
          v-if="progressPercent > 0"
          class="pointer-events-none absolute right-0 bottom-0 left-0 h-1 bg-black/45"
        >
          <div
            class="h-full bg-[var(--md-sys-color-primary)]"
            :style="{ width: `${progressPercent}%` }"
          />
        </div>
      </PosterImage>
    </div>

    <div class="flex items-start justify-between gap-1.5">
      <div class="min-w-0 flex-1">
        <h3 class="truncate text-xs font-bold text-[var(--md-sys-color-on-surface)]">
          {{ video.title }}
        </h3>
        <div class="mt-0.5 flex items-center gap-1.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
          <button
            v-if="video.show_id && video.show_title"
            type="button"
            class="truncate text-left hover:text-[var(--md-sys-color-primary)] hover:underline"
            @click.stop="router.push(`/video/shows/${video.show_id}`)"
          >
            {{ video.show_title }}
          </button>
          <span v-else class="font-mono text-[11px] uppercase">
            {{ video.container }}
          </span>
          <span v-if="video.year" class="shrink-0">· {{ video.year }}</span>
        </div>
      </div>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        :aria-label="t('common.moreActions')"
        :aria-expanded="menuOpen"
        aria-haspopup="menu"
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
      role="menu"
      :style="menuPos"
      class="fixed z-[var(--z-popover)] w-56 rounded-2xl border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-high)] p-1.5 text-xs text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
    >
      <button
        type="button"
        role="menuitem"
        class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left transition hover:bg-[var(--md-sys-color-surface-container-highest)]"
        @click="toggleWatched"
      >
        <LuCheck class="h-3.5 w-3.5 text-[var(--md-sys-color-success)]" />
        <span>
          {{ video.progress?.completed ? t('common.markUnwatched') : t('common.markWatched') }}
        </span>
      </button>

      <button
        v-if="authStore.isAdmin"
        type="button"
        role="menuitem"
        class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left transition hover:bg-[var(--md-sys-color-surface-container-highest)]"
        @click="menuOpen = false; emit('edit', video)"
      >
        <LuPencil class="h-3.5 w-3.5 text-[var(--md-sys-color-on-surface-variant)]" />
        <span>{{ t('common.editMetadata') }}</span>
      </button>

      <button
        type="button"
        role="menuitem"
        class="md-state flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left transition hover:bg-[var(--md-sys-color-surface-container-highest)]"
        @click="createShelfWithVideo"
      >
        <LuPlus class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
        <span>{{ t('nav.newShelf') }}</span>
      </button>

      <div
        v-if="videoStore.shelves.length > 0"
        class="my-1 max-h-28 overflow-y-auto border-t border-[var(--md-sys-color-outline-variant)]/60 pt-1"
      >
        <button
          v-for="sh in videoStore.shelves"
          :key="sh.id"
          type="button"
          role="menuitem"
          class="md-state flex w-full items-center justify-between rounded-xl px-3 py-1.5 text-left text-[11px] text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-highest)] hover:text-[var(--md-sys-color-on-surface)]"
          @click="addToShelf(sh.id)"
        >
          <span class="truncate">+ {{ sh.name }}</span>
          <span class="font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ sh.video_count }}
          </span>
        </button>
      </div>

      <a
        :href="buildVideoDownloadUrl(video.id)"
        role="menuitem"
        class="md-state mt-0.5 flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left transition hover:bg-[var(--md-sys-color-surface-container-highest)]"
        @click="menuOpen = false"
      >
        <LuDownload class="h-3.5 w-3.5 text-[var(--md-sys-color-on-surface-variant)]" />
        <span>{{ t('common.download') }}</span>
      </a>

      <button
        v-if="removableFromShelf"
        type="button"
        role="menuitem"
        class="md-state mt-0.5 flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-[var(--md-sys-color-error)] transition hover:bg-[var(--md-sys-color-error-container)] hover:text-[var(--md-sys-color-on-error-container)]"
        @click="menuOpen = false; emit('removeFromShelf', video.id)"
      >
        <LuTrash2 class="h-3.5 w-3.5" />
        <span>{{ t('shelf.removeVideo') }}</span>
      </button>

      <button
        v-if="authStore.isAdmin"
        type="button"
        role="menuitem"
        class="md-state mt-0.5 flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-[var(--md-sys-color-error)] transition hover:bg-[var(--md-sys-color-error-container)] hover:text-[var(--md-sys-color-on-error-container)]"
        @click="confirmDelete"
      >
        <LuTrash2 class="h-3.5 w-3.5" />
        <span>{{ t('common.delete') }}</span>
      </button>
    </div>
  </Teleport>
</template>
