<script setup lang="ts">
import { LuFolderHeart, LuMoreVertical, LuPencil, LuTrash2 } from 'vue-icons-plus/lu'
import { MdIconButton, MdMenu, MdMenuItem, MediaCoverImage } from '@/components/ui'
import { buildPosterUrl } from '@/domains/video/api'
import { useVideoI18n } from '@/domains/video/i18n'
import type { VideoShelfDetailDto } from '@/domains/video/types'

type VideoShelfItem = VideoShelfDetailDto['shelf']

defineProps<{
  shelves: readonly VideoShelfItem[]
}>()

const emit = defineEmits<{
  open: [shelfId: string]
  rename: [shelfId: string, currentName: string]
  delete: [shelfId: string, name: string]
}>()

const { t } = useVideoI18n()
</script>

<template>
  <div class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
    <div
      v-for="sh in shelves"
      :key="sh.id"
      class="group flex items-center gap-3.5 rounded-corner-md bg-surface-container-low p-3 transition-colors duration-150 ease-standard hover:bg-surface-container"
    >
      <button
        type="button"
        class="flex min-w-0 flex-1 cursor-pointer items-center gap-3.5 rounded-corner-sm text-left outline-none focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-secondary"
        @click="emit('open', sh.id)"
      >
        <div class="h-12 w-20 shrink-0 overflow-hidden rounded-corner-sm bg-surface-container-high">
          <MediaCoverImage
            :src="buildPosterUrl(sh.poster_key, 240)"
            :alt="sh.name"
            aspect-class="aspect-video"
            rounded="rounded-corner-sm"
            :fallback-icon="LuFolderHeart"
            fallback-icon-class="h-5 w-5 text-on-surface-variant/60"
          />
        </div>

        <div class="min-w-0 flex-1">
          <h3 class="text-title-small truncate text-on-surface">
            {{ sh.name }}
          </h3>
          <p class="text-body-small mt-0.5 truncate text-on-surface-variant">
            {{ t('common.videosCount', { count: sh.video_count }) }}
          </p>
        </div>
      </button>

      <div class="shrink-0">
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
              :icon="LuPencil"
              :label="t('common.edit')"
              @select="
                close();
                emit('rename', sh.id, sh.name)
              "
            />
            <div class="my-1 border-t border-outline-variant pt-1" role="separator">
              <MdMenuItem
                :icon="LuTrash2"
                :label="t('common.delete')"
                destructive
                @select="
                  close();
                  emit('delete', sh.id, sh.name)
                "
              />
            </div>
          </template>
        </MdMenu>
      </div>
    </div>
  </div>
</template>
