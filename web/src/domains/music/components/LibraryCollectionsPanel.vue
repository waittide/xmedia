<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import {
  LuListMusic,
  LuLoader2,
  LuMoreVertical,
  LuPencil,
  LuPlay,
  LuPlus,
  LuTrash2,
} from 'vue-icons-plus/lu'
import {
  MdButton,
  MdEmptyState,
  MdIconButton,
  MdMenu,
  MdMenuItem,
  MediaCoverImage,
} from '@/components/ui'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { api, buildCoverUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { PlaylistSummaryDto } from '../types'

const props = withDefaults(
  defineProps<{
    query?: string
  }>(),
  {
    query: '',
  },
)

const { t } = useI18n()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const snackbarStore = useSnackbarStore()

const playingId = ref<string | null>(null)

const filteredPlaylists = computed(() => {
  const needle = props.query.trim().toLowerCase()
  if (!needle) return musicStore.playlists
  return musicStore.playlists.filter(
    (pl) =>
      pl.name.toLowerCase().includes(needle) ||
      pl.owner_username.toLowerCase().includes(needle),
  )
})

function canEdit(pl: PlaylistSummaryDto): boolean {
  return pl.owner_id === authStore.user?.id || authStore.isAdmin
}

async function handleCreatePlaylist(): Promise<void> {
  const name = await dialogStore.prompt({
    title: t('playlist.createTitle'),
    message: t('playlist.createPrompt'),
    placeholder: t('playlist.namePlaceholder'),
  })
  if (!name || !name.trim()) return
  try {
    await api.createPlaylist({ name: name.trim() })
    await musicStore.refreshSidebar()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleQuickPlay(pl: PlaylistSummaryDto): Promise<void> {
  if (playingId.value === pl.id) return
  playingId.value = pl.id
  try {
    const detail = await api.getPlaylist(pl.id)
    if (detail.tracks.length > 0) {
      playerStore.playQueue(detail.tracks, 0)
    }
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    if (playingId.value === pl.id) playingId.value = null
  }
}

async function handleRename(pl: PlaylistSummaryDto): Promise<void> {
  const nextName = await dialogStore.prompt({
    title: t('playlist.renameTitle'),
    message: t('playlist.renamePrompt'),
    initialValue: pl.name,
  })
  if (!nextName || !nextName.trim() || nextName.trim() === pl.name) return
  try {
    await api.updatePlaylist(pl.id, { name: nextName.trim() })
    await musicStore.refreshSidebar()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleDelete(pl: PlaylistSummaryDto): Promise<void> {
  const ok = await dialogStore.confirm({
    title: t('playlist.deleteTitle'),
    message: t('playlist.confirmDelete', { name: pl.name }),
    danger: true,
  })
  if (!ok) return
  try {
    await api.deletePlaylist(pl.id)
    await musicStore.refreshSidebar()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center justify-between gap-2">
      <span class="text-body-small text-on-surface-variant">
        {{ filteredPlaylists.length }}
      </span>
      <MdButton variant="tonal" :icon="LuPlus" @click="handleCreatePlaylist">
        {{ t('playlist.createBtn') }}
      </MdButton>
    </div>

    <MdEmptyState
      v-if="filteredPlaylists.length === 0"
      compact
      :icon="LuListMusic"
      :title="t('playlist.emptyPlaylists')"
    >
      <template #action>
        <MdButton variant="tonal" :icon="LuPlus" @click="handleCreatePlaylist">
          {{ t('playlist.createBtn') }}
        </MdButton>
      </template>
    </MdEmptyState>

    <div v-else class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
      <article
        v-for="pl in filteredPlaylists"
        :key="pl.id"
        class="group relative flex items-center gap-3.5 rounded-corner-lg bg-surface-container-low p-3 transition-colors hover:bg-surface-container"
      >
        <MediaCoverImage
          :src="pl.cover_key ? buildCoverUrl(pl.id, 160) : null"
          :alt="pl.name"
          aspect-class="h-14 w-14 shrink-0"
          rounded="rounded-corner-md"
          bg-class="bg-primary-container"
          :fallback-icon="LuListMusic"
          fallback-icon-class="h-6 w-6 text-on-primary-container"
        />

        <RouterLink
          :to="{ path: '/music/library', query: { collection: pl.id } }"
          class="min-w-0 flex-1 outline-none after:absolute after:inset-0 after:rounded-corner-lg focus-visible:after:outline-3 focus-visible:after:outline-offset-2 focus-visible:after:outline-secondary"
        >
          <h3 class="text-title-small truncate text-on-surface group-hover:text-primary">
            {{ pl.name }}
          </h3>
          <p class="text-body-small mt-0.5 truncate text-on-surface-variant">
            {{ t('common.tracksCount', { count: pl.track_count }) }} · {{ pl.owner_username }}
          </p>
        </RouterLink>

        <div class="relative z-10 flex shrink-0 items-center gap-0.5">
          <MdIconButton
            v-if="pl.track_count > 0"
            size="sm"
            variant="tonal"
            :loading="playingId === pl.id"
            :aria-label="t('common.playAll')"
            @click.stop="handleQuickPlay(pl)"
          >
            <LuLoader2 v-if="playingId === pl.id" class="h-4 w-4 animate-spin" />
            <LuPlay v-else class="ml-0.5 h-4 w-4 fill-current" />
          </MdIconButton>

          <MdMenu v-if="canEdit(pl)" placement="bottom-end" :aria-label="t('common.moreActions')">
            <template #trigger="{ toggle, triggerAttrs }">
              <MdIconButton
                v-bind="triggerAttrs"
                size="sm"
                variant="standard"
                :icon="LuMoreVertical"
                :aria-label="t('common.moreActions')"
                @click.stop="toggle"
              />
            </template>
            <MdMenuItem
              :icon="LuPencil"
              :label="t('playlist.renameTitle')"
              @select="handleRename(pl)"
            />
            <div class="my-1 border-t border-outline-variant pt-1">
              <MdMenuItem
                :icon="LuTrash2"
                :label="t('playlist.deleteTitle')"
                destructive
                @select="handleDelete(pl)"
              />
            </div>
          </MdMenu>
        </div>
      </article>
    </div>
  </div>
</template>
