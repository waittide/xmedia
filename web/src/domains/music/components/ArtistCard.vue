<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink } from 'vue-router'
import { LuHeart, LuMic } from 'vue-icons-plus/lu'
import { MdIconButton, MediaCoverImage } from '@/components/ui'
import { useSnackbarStore } from '@/stores/snackbar'
import { buildCoverUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { useMusicStore } from '../store'
import type { ArtistDto } from '../types'

const props = defineProps<{
  artist: ArtistDto
}>()

const { t } = useI18n()
const musicStore = useMusicStore()
const snackbarStore = useSnackbarStore()

const coverSrc = computed(() =>
  props.artist.cover_key ? buildCoverUrl(props.artist.id, 280) : null,
)
const artistTo = computed(() => ({
  path: '/music/library',
  query: { artist: props.artist.id },
}))

async function onStar(ev: MouseEvent): Promise<void> {
  ev.preventDefault()
  ev.stopPropagation()
  try {
    await musicStore.toggleArtistStar(props.artist)
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}
</script>

<template>
  <article
    class="group relative flex flex-col items-center rounded-corner-lg p-3 text-center transition-colors duration-150 ease-standard hover:bg-surface-container-low"
  >
    <div class="mb-3 w-3/4 max-w-40">
      <MediaCoverImage
        :src="coverSrc"
        :alt="artist.name"
        aspect-class="aspect-square"
        rounded="rounded-corner-full"
        bg-class="bg-surface-container-high"
        :fallback-icon="LuMic"
        fallback-icon-class="h-10 w-10 text-on-surface-variant/50"
        class="album-shadow"
      />
    </div>

    <div class="flex w-full items-center justify-center gap-1">
      <RouterLink
        :to="artistTo"
        class="min-w-0 flex-1 outline-none after:absolute after:inset-0 after:rounded-corner-lg focus-visible:after:outline-3 focus-visible:after:outline-offset-2 focus-visible:after:outline-secondary"
      >
        <h3 class="text-title-small truncate text-on-surface" :title="artist.name">
          {{ artist.name }}
        </h3>
        <p class="text-body-small mt-0.5 truncate tabular-nums text-on-surface-variant">
          {{ t('common.albumsCount', { count: artist.album_count }) }} ·
          {{ t('common.tracksCount', { count: artist.track_count }) }}
        </p>
      </RouterLink>
      <MdIconButton
        size="sm"
        variant="standard"
        :selected="Boolean(artist.starred_at)"
        :aria-label="artist.starred_at ? t('common.unfavorite') : t('common.favorite')"
        class="relative z-20 shrink-0"
        @click="onStar"
      >
        <LuHeart
          class="h-4 w-4"
          :class="artist.starred_at ? 'fill-error text-error' : ''"
        />
      </MdIconButton>
    </div>
  </article>
</template>
