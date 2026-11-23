<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { LuTrash2, LuTv } from 'vue-icons-plus/lu'
import { MediaItemCard } from '@/components/domain'
import { MdMenuItem } from '@/components/ui'
import type { MediaItemVM } from '@/sources'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { api, buildPosterUrl } from '../api'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { ShowSummaryDto } from '../types'

const props = defineProps<{
  show: ShowSummaryDto
}>()

const emit = defineEmits<{
  unbound: [showId: string]
}>()

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const videoStore = useVideoStore()
const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()
const favoriteBusy = ref(false)

const progressPercent = computed<number | undefined>(() => {
  if (props.show.episode_count <= 0 || props.show.watched_episode_count <= 0) return undefined
  return Math.min(100, Math.round((props.show.watched_episode_count / props.show.episode_count) * 100))
})

const subtitleText = computed(() => {
  const parts: string[] = [
    t('common.seasonsCount', { count: props.show.season_count }),
    t('common.episodesCount', { count: props.show.episode_count }),
  ]
  if (props.show.year) {
    parts.push(String(props.show.year))
  }
  return parts.join(' · ')
})

const statusText = computed(() => {
  if (props.show.watched_episode_count <= 0) return undefined
  return t('common.watchedProgress', {
    watched: props.show.watched_episode_count,
    total: props.show.episode_count,
  })
})

const cardVM = computed<MediaItemVM>(() => ({
  ref: { scope: 'local', domain: 'video', id: props.show.id },
  title: props.show.title,
  subtitle: subtitleText.value,
  cover: buildPosterUrl(props.show.poster_key, 480) ?? undefined,
  badge: {
    sourceKind: 'local',
    progressPercent: progressPercent.value,
    status: statusText.value,
  },
  to: { path: `/video/item/${encodeURIComponent(props.show.id)}` },
}))

async function handleToggleFavorite(): Promise<void> {
  if (favoriteBusy.value) return
  favoriteBusy.value = true
  try {
    await videoStore.toggleShowFavorite(props.show)
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    favoriteBusy.value = false
  }
}

async function handleUnbindShow(close: () => void): Promise<void> {
  close()
  const ok = await dialogStore.confirm({
    title: globalT('pool.unbindBtn'),
    message: globalT('pool.unbindConfirm', { title: props.show.title }),
    danger: true,
  })
  if (!ok) return
  try {
    await api.unbindItem(props.show.id)
    await videoStore.refreshSidebar(true)
    videoStore.bumpCatalogVersion()
    snackbar.show({ message: globalT('pool.unboundToast') })
    emit('unbound', props.show.id)
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}
</script>

<template>
  <MediaItemCard
    :item="cardVM"
    aspect="poster"
    cover-style="plain"
    :fallback-icon="LuTv"
    :show-favorite="true"
    :favorite="show.is_favorite"
    :favorite-loading="favoriteBusy"
    @toggle-favorite="handleToggleFavorite"
  >
    <template #menu="{ close }">
      <MdMenuItem
        :icon="LuTrash2"
        :label="globalT('pool.unbindBtn')"
        destructive
        @select="handleUnbindShow(close)"
      />
    </template>
  </MediaItemCard>
</template>
