<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { LuDisc3, LuHeart, LuPlay } from 'vue-icons-plus/lu'
import { MdIconButton, MediaCoverImage } from '@/components/ui'
import { useSnackbarStore } from '@/stores/snackbar'
import { api, buildCoverUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { AlbumDto } from '../types'

const props = defineProps<{
  album: AlbumDto
}>()

const { t } = useI18n()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const snackbarStore = useSnackbarStore()

const playingLoading = ref(false)

const coverSrc = computed(() =>
  props.album.cover_key ? buildCoverUrl(props.album.id, 320) : null,
)
const albumTo = computed(() => `/music/item/${encodeURIComponent(props.album.id)}`)
const artistTo = computed(() => ({
  path: '/music/library',
  query: { artist: props.album.album_artist_id },
}))

async function quickPlayAlbum(ev: MouseEvent): Promise<void> {
  ev.preventDefault()
  ev.stopPropagation()
  if (playingLoading.value) return
  playingLoading.value = true
  try {
    const detail = await api.getAlbum(props.album.id)
    if (detail.tracks.length > 0) {
      playerStore.playQueue(detail.tracks, 0)
    }
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    playingLoading.value = false
  }
}

async function onStar(ev: MouseEvent): Promise<void> {
  ev.preventDefault()
  ev.stopPropagation()
  try {
    await musicStore.toggleAlbumStar(props.album)
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}
</script>

<template>
  <article class="group relative flex flex-col text-on-surface">
    <div class="album-shadow relative overflow-hidden rounded-corner-md bg-surface-container-high">
      <MediaCoverImage
        :src="coverSrc"
        :alt="album.title"
        aspect-class="aspect-square"
        rounded="rounded-corner-md"
        bg-class="bg-surface-container-high"
        :fallback-icon="LuDisc3"
        fallback-icon-class="h-10 w-10 text-on-surface-variant/50"
      />
    </div>

    <div class="mt-2 flex items-start justify-between gap-1.5">
      <div class="min-w-0 flex-1">
        <RouterLink
          :to="albumTo"
          class="block outline-none after:absolute after:inset-0 after:rounded-corner-md focus-visible:after:outline-3 focus-visible:after:outline-offset-2 focus-visible:after:outline-secondary"
        >
          <h3 class="text-title-small truncate text-on-surface" :title="album.title">
            {{ album.title }}
          </h3>
        </RouterLink>

        <div class="text-body-small mt-0.5 flex items-center gap-1.5 text-on-surface-variant">
          <RouterLink
            :to="artistTo"
            class="relative z-10 min-w-0 truncate hover:text-primary hover:underline"
          >
            {{ album.album_artist_name }}
          </RouterLink>
          <span v-if="album.year" class="shrink-0 tabular-nums">· {{ album.year }}</span>
        </div>
      </div>

      <div class="relative z-20 flex shrink-0 items-center gap-0.5">
        <MdIconButton
          size="sm"
          variant="standard"
          :selected="Boolean(album.starred_at)"
          :aria-label="album.starred_at ? t('common.unfavorite') : t('common.favorite')"
          @click="onStar"
        >
          <LuHeart
            class="h-4 w-4"
            :class="album.starred_at ? 'fill-current text-tertiary' : ''"
          />
        </MdIconButton>
        <MdIconButton
          size="sm"
          variant="tonal"
          :loading="playingLoading"
          :disabled="playingLoading"
          :title="t('common.playAll')"
          :aria-label="t('common.playAll')"
          @click="quickPlayAlbum"
        >
          <LuPlay class="ml-0.5 h-4 w-4 fill-current" aria-hidden="true" />
        </MdIconButton>
      </div>
    </div>
  </article>
</template>
