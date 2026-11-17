<script setup lang="ts">
import { useRouter } from 'vue-router'
import { LuHeart, LuPlay } from 'vue-icons-plus/lu'
import { api } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { AlbumDto } from '../types'
import CoverImage from './CoverImage.vue'

const props = defineProps<{
  album: AlbumDto
}>()

const { t } = useI18n()
const router = useRouter()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()

async function quickPlayAlbum(ev: MouseEvent): Promise<void> {
  ev.stopPropagation()
  const detail = await api.getAlbum(props.album.id)
  playerStore.playQueue(detail.tracks, 0)
}

async function onStar(ev: MouseEvent): Promise<void> {
  ev.stopPropagation()
  await musicStore.toggleAlbumStar(props.album)
}
</script>

<template>
  <div
    class="group flex cursor-pointer flex-col rounded-2xl p-2.5 transition hover:bg-[var(--ctp-mantle)]"
    @click="router.push(`/music/albums/${album.id}`)"
  >
    <div class="relative mb-2.5">
      <CoverImage
        :cover-id="album.id"
        :has-cover="Boolean(album.cover_key)"
        :size="320"
        :alt="album.title"
        rounded="rounded-2xl"
        class="album-shadow"
      >
        <div
          class="absolute inset-0 flex items-end justify-between bg-gradient-to-t from-black/55 via-transparent to-transparent p-2.5 opacity-0 transition-opacity duration-150 group-hover:opacity-100"
        >
          <button
            type="button"
            class="flex h-8 w-8 items-center justify-center rounded-full bg-black/45 text-white backdrop-blur-sm transition hover:scale-105"
            @click="onStar"
          >
            <LuHeart
              class="h-4 w-4"
              :class="album.starred_at ? 'fill-[var(--ctp-red)] text-[var(--ctp-red)]' : ''"
            />
          </button>

          <button
            type="button"
            class="flex h-10 w-10 items-center justify-center rounded-full bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-lg transition hover:scale-105"
            :title="t('common.playAll')"
            @click="quickPlayAlbum"
          >
            <LuPlay class="ml-0.5 h-4 w-4 fill-current" />
          </button>
        </div>
      </CoverImage>
    </div>

    <h3 class="truncate text-xs font-bold text-[var(--ctp-text)]">
      {{ album.title }}
    </h3>
    <div class="mt-0.5 flex items-center justify-between gap-1 text-[11px] text-[var(--ctp-subtext0)]">
      <span
        class="truncate hover:text-[var(--ctp-accent)] hover:underline"
        @click.stop="router.push(`/music/artists/${album.album_artist_id}`)"
      >
        {{ album.album_artist_name }}
      </span>
      <span v-if="album.year" class="shrink-0">{{ album.year }}</span>
    </div>
  </div>
</template>
