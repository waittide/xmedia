<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import {
  LuArrowLeft,
  LuGlobe,
  LuListMusic,
  LuLock,
  LuMoreVertical,
  LuPencil,
  LuPlay,
  LuRefreshCw,
  LuShuffle,
  LuTrash2,
} from 'vue-icons-plus/lu'
import { MediaErrorState } from '@/components/domain'
import {
  MdButton,
  MdEmptyState,
  MdIconButton,
  MdMenu,
  MdMenuItem,
  MdSkeleton,
  MediaCoverImage,
} from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { api, buildCoverUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { PlaylistDetailDto } from '../types'
import TrackTable from './TrackTable.vue'

const props = defineProps<{
  playlistId: string
}>()

const emit = defineEmits<{
  back: []
  deleted: []
}>()

const { t } = useI18n()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const snackbarStore = useSnackbarStore()

const detail = ref<PlaylistDetailDto | null>(null)
const loading = ref(true)
const showSkeleton = useDelayedLoading(() => loading.value && !detail.value)
const error = ref<string | null>(null)
let loadSeq = 0

const canEdit = computed(
  () =>
    detail.value !== null &&
    (detail.value.playlist.owner_id === authStore.user?.id || authStore.isAdmin),
)

const coverSrc = computed(() =>
  detail.value?.playlist.cover_key ? buildCoverUrl(detail.value.playlist.id, 400) : null,
)

async function loadPlaylist(): Promise<void> {
  if (!props.playlistId) return
  const seq = ++loadSeq
  loading.value = true
  error.value = null
  try {
    const res = await api.getPlaylist(props.playlistId)
    if (seq !== loadSeq) return
    detail.value = res
  } catch (err) {
    if (seq !== loadSeq) return
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === loadSeq) {
      loading.value = false
    }
  }
}

async function handleRename(): Promise<void> {
  if (!detail.value) return
  const next = await dialogStore.prompt({
    title: t('playlist.renameTitle'),
    message: t('playlist.renamePrompt'),
    initialValue: detail.value.playlist.name,
  })
  if (!next || !next.trim()) return
  try {
    detail.value = await api.updatePlaylist(detail.value.playlist.id, { name: next.trim() })
    await musicStore.refreshSidebar()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleToggleVisibility(): Promise<void> {
  if (!detail.value) return
  try {
    detail.value = await api.updatePlaylist(detail.value.playlist.id, {
      is_public: !detail.value.playlist.is_public,
    })
    await musicStore.refreshSidebar()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleRemoveTrack(idx: number): Promise<void> {
  if (!detail.value) return
  try {
    detail.value = await api.updatePlaylist(detail.value.playlist.id, { remove_indices: [idx] })
    await musicStore.refreshSidebar()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleDelete(): Promise<void> {
  if (!detail.value) return
  const ok = await dialogStore.confirm({
    title: t('playlist.deleteTitle'),
    message: t('playlist.confirmDelete', { name: detail.value.playlist.name }),
    danger: true,
  })
  if (!ok) return
  try {
    await api.deletePlaylist(detail.value.playlist.id)
    await musicStore.refreshSidebar()
    emit('deleted')
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

onMounted(loadPlaylist)
watch(() => props.playlistId, loadPlaylist)
</script>

<template>
  <div class="space-y-6">
    <div>
      <MdButton variant="text" :icon="LuArrowLeft" @click="emit('back')">
        {{ t('nav.collections') }}
      </MdButton>
    </div>

    <MediaErrorState
      v-if="error"
      :message="error"
      :retrying="loading"
      @retry="loadPlaylist"
    />

    <div v-else-if="loading && !detail" class="space-y-6" aria-busy="true">
      <div v-if="showSkeleton" class="flex items-center gap-5 rounded-corner-xl bg-surface-container-low p-5">
        <MdSkeleton variant="rect" width="7rem" height="7rem" class="shrink-0 rounded-corner-md" />
        <div class="flex-1 space-y-2.5">
          <MdSkeleton variant="text" width="45%" height="1.75rem" />
          <MdSkeleton variant="text" width="30%" height="1rem" />
        </div>
      </div>
    </div>

    <template v-else-if="detail">
      <!-- Clean Playlist Header Card: Play All + Shuffle + Overflow Menu (Rename / Public / Delete) -->
      <section
        class="flex flex-col items-start gap-5 rounded-corner-xl bg-surface-container-low p-5 sm:flex-row sm:items-center"
      >
        <MediaCoverImage
          :src="coverSrc"
          :alt="detail.playlist.name"
          aspect-class="h-28 w-28 sm:h-36 sm:w-36 shrink-0"
          rounded="rounded-corner-md"
          bg-class="bg-primary-container"
          :fallback-icon="LuListMusic"
          fallback-icon-class="h-10 w-10 text-on-primary-container"
          loading="eager"
          class="album-shadow"
        />

        <div class="min-w-0 flex-1 space-y-2">
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-body-small text-on-surface-variant">
              {{ t('playlist.ownerBy', { owner: detail.playlist.owner_username }) }} ·
              {{ t('common.tracksCount', { count: detail.playlist.track_count }) }}
            </span>
            <span
              class="text-label-small inline-flex items-center gap-1 rounded-corner-xs bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
            >
              <LuGlobe v-if="detail.playlist.is_public" class="h-3 w-3" aria-hidden="true" />
              <LuLock v-else class="h-3 w-3" aria-hidden="true" />
              <span>
                {{ detail.playlist.is_public ? t('playlist.publicLabel') : t('playlist.privateLabel') }}
              </span>
            </span>
          </div>

          <h2 class="text-headline-medium truncate text-on-surface">
            {{ detail.playlist.name }}
          </h2>

          <p v-if="detail.playlist.comment" class="text-body-medium text-on-surface-variant">
            {{ detail.playlist.comment }}
          </p>

          <div class="flex flex-wrap items-center gap-2 pt-1">
            <MdButton
              v-if="detail.tracks.length > 0"
              variant="filled"
              :icon="LuPlay"
              @click="playerStore.playQueue(detail.tracks, 0)"
            >
              {{ t('common.playAll') }}
            </MdButton>

            <MdButton
              v-if="detail.tracks.length > 0"
              variant="tonal"
              :icon="LuShuffle"
              @click="playerStore.shuffleAndPlay(detail.tracks)"
            >
              {{ t('common.shufflePlay') }}
            </MdButton>

            <MdMenu placement="bottom-end" :min-width="200" :aria-label="t('common.moreActions')">
              <template #trigger="{ toggle, triggerAttrs }">
                <MdIconButton
                  v-bind="triggerAttrs"
                  variant="standard"
                  :icon="LuMoreVertical"
                  :aria-label="t('common.moreActions')"
                  @click="toggle"
                />
              </template>

              <MdMenuItem
                v-if="canEdit"
                :icon="LuPencil"
                :label="t('playlist.renameTitle')"
                @select="handleRename"
              />
              <MdMenuItem
                v-if="canEdit"
                :icon="detail.playlist.is_public ? LuLock : LuGlobe"
                :label="
                  detail.playlist.is_public ? t('playlist.makePrivate') : t('playlist.makePublic')
                "
                @select="handleToggleVisibility"
              />
              <MdMenuItem
                :icon="LuRefreshCw"
                :label="$t('common.refresh')"
                @select="loadPlaylist"
              />
              <div v-if="canEdit" class="my-1 border-t border-outline-variant pt-1">
                <MdMenuItem
                  :icon="LuTrash2"
                  :label="t('playlist.deleteTitle')"
                  destructive
                  @select="handleDelete"
                />
              </div>
            </MdMenu>
          </div>
        </div>
      </section>

      <MdEmptyState
        v-if="detail.tracks.length === 0"
        compact
        :icon="LuListMusic"
        :title="t('playlist.emptyTracks')"
      />

      <TrackTable
        v-else
        :tracks="detail.tracks"
        :removable="canEdit"
        @remove="handleRemoveTrack"
      />
    </template>
  </div>
</template>
