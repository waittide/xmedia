<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  LuArrowLeft,
  LuFolderHeart,
  LuMoreVertical,
  LuPencil,
  LuPlay,
  LuPlus,
  LuTrash2,
} from 'vue-icons-plus/lu'
import {
  MdButton,
  MdIconButton,
  MdMenu,
  MdMenuItem,
  MdTextField,
  MediaCoverImage,
} from '@/components/ui'
import { api, buildPosterUrl } from '@/domains/video/api'
import { useVideoI18n } from '@/domains/video/i18n'
import { useVideoStore } from '@/domains/video/store'
import type { VideoShelfDetailDto, VideoSummaryDto } from '@/domains/video/types'
import type { LibraryTab, SourceScope } from '@/sources'
import { useSnackbarStore } from '@/stores/snackbar'

const props = defineProps<{
  activeScope: SourceScope
  activeTab: LibraryTab
  activeCollectionId: string
  shelfDetail: VideoShelfDetailDto | null
}>()

const emit = defineEmits<{
  back: []
  rename: [shelfId: string, currentName: string]
  delete: [shelfId: string, name: string]
}>()

const { t } = useVideoI18n()
const videoStore = useVideoStore()
const snackbar = useSnackbarStore()

const newShelfName = ref('')
const creatingShelf = ref(false)

const firstPlayableShelfVideo = computed<VideoSummaryDto | null>(() => {
  const list = props.shelfDetail?.videos ?? []
  if (list.length === 0) return null
  return list.find((v) => !v.progress?.completed) ?? list[0] ?? null
})

async function handleCreateShelf(): Promise<void> {
  const trimmed = newShelfName.value.trim()
  if (!trimmed || creatingShelf.value) return
  creatingShelf.value = true
  try {
    await api.createShelf({ name: trimmed })
    newShelfName.value = ''
    await videoStore.refreshSidebar(true)
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    creatingShelf.value = false
  }
}
</script>

<template>
  <section
    v-if="activeCollectionId && shelfDetail"
    class="space-y-4 rounded-corner-lg bg-surface-container-low p-4 sm:p-5"
  >
    <div class="flex items-center">
      <MdButton variant="text" :icon="LuArrowLeft" @click="emit('back')">
        {{ t('shelf.backToCollections') }}
      </MdButton>
    </div>

    <div class="flex flex-col gap-4 sm:flex-row sm:items-center">
      <div class="w-40 shrink-0 overflow-hidden rounded-corner-md bg-surface-container-high">
        <MediaCoverImage
          :src="buildPosterUrl(shelfDetail.shelf.poster_key, 400)"
          :alt="shelfDetail.shelf.name"
          aspect-class="aspect-video"
          rounded="rounded-corner-md"
          :fallback-icon="LuFolderHeart"
          fallback-icon-class="h-8 w-8 text-on-surface-variant/60"
        />
      </div>

      <div class="min-w-0 flex-1 space-y-2">
        <div class="space-y-0.5">
          <h2 class="text-title-large truncate text-on-surface">
            {{ shelfDetail.shelf.name }}
          </h2>
          <p class="text-body-small text-on-surface-variant">
            {{ t('common.videosCount', { count: shelfDetail.shelf.video_count }) }}
          </p>
          <p
            v-if="shelfDetail.shelf.description"
            class="text-body-medium line-clamp-2 text-on-surface-variant"
          >
            {{ shelfDetail.shelf.description }}
          </p>
        </div>

        <div class="flex flex-wrap items-center gap-2 pt-1">
          <MdButton
            v-if="firstPlayableShelfVideo"
            variant="filled"
            :icon="LuPlay"
            :to="{
              path: `/video/play/${encodeURIComponent(firstPlayableShelfVideo.id)}`,
              query: { shelf: shelfDetail.shelf.id },
            }"
          >
            {{ t('common.play') }}
          </MdButton>

          <MdButton
            variant="tonal"
            :icon="LuPencil"
            @click="emit('rename', shelfDetail.shelf.id, shelfDetail.shelf.name)"
          >
            {{ t('common.edit') }}
          </MdButton>

          <MdMenu placement="bottom-end" :aria-label="t('common.moreActions')">
            <template #trigger="{ toggle, triggerAttrs }">
              <MdIconButton
                v-bind="triggerAttrs"
                variant="standard"
                size="sm"
                :icon="LuMoreVertical"
                :aria-label="t('common.moreActions')"
                @click="toggle"
              />
            </template>
            <template #default="{ close }">
              <MdMenuItem
                :icon="LuTrash2"
                :label="t('common.delete')"
                destructive
                @select="
                  close();
                  emit('delete', shelfDetail.shelf.id, shelfDetail.shelf.name)
                "
              />
            </template>
          </MdMenu>
        </div>
      </div>
    </div>
  </section>

  <form
    v-else-if="activeScope === 'local' && activeTab === 'collections' && !activeCollectionId"
    class="flex flex-wrap items-center gap-2"
    @submit.prevent="handleCreateShelf"
  >
    <div class="min-w-52 flex-1 sm:max-w-xs">
      <MdTextField
        v-model="newShelfName"
        size="sm"
        :placeholder="t('nav.shelfNamePlaceholder')"
      />
    </div>
    <MdButton
      type="submit"
      variant="tonal"
      :icon="LuPlus"
      :loading="creatingShelf"
      :disabled="!newShelfName.trim()"
    >
      {{ t('nav.newShelf') }}
    </MdButton>
  </form>
</template>
