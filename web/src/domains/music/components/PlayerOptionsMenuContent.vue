<script setup lang="ts">
import {
  LuBookmark,
  LuCheck,
  LuDisc3,
  LuDownload,
  LuLibrary,
  LuUser,
} from 'vue-icons-plus/lu'
import { MdMenuItem } from '@/components/ui'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import type { TranscodeProfile } from '../types'

withDefaults(
  defineProps<{
    qualityOptions: Array<{ value: TranscodeProfile; label: string; trailing: string }>
    onlineQualityOptions: Array<{ value: string; label: string }>
    hasQualityMenuItems: boolean
    savingBookmark: boolean
    downloadingCurrent: boolean
    showEntityLinks?: boolean
  }>(),
  {
    showEntityLinks: false,
  },
)

const emit = defineEmits<{
  download: []
  bookmark: []
  navigate: [path: string]
}>()

const { t } = useI18n()
const playerStore = usePlayerStore()
</script>

<template>
  <template v-if="hasQualityMenuItems">
    <div
      class="text-label-small flex items-center justify-between px-3 py-1.5 text-on-surface-variant"
    >
      <span>{{ t('player.qualityHeading') }}</span>
      <span v-if="playerStore.qualitySummary" class="font-mono">
        {{ playerStore.qualitySummary }}
      </span>
    </div>

    <template v-if="playerStore.currentLocalTrack">
      <MdMenuItem
        v-for="opt in qualityOptions"
        :key="opt.value"
        :label="opt.label"
        :trailing-text="opt.trailing"
        :selected="playerStore.transcodeProfile === opt.value"
        @select="playerStore.setTranscodeProfile(opt.value)"
      />
    </template>

    <template v-else-if="playerStore.isCurrentOnline">
      <MdMenuItem
        v-for="opt in onlineQualityOptions"
        :key="opt.value"
        :label="opt.label"
        :selected="
          (playerStore.currentOnlineItem?.selectedQuality ?? onlineQualityOptions[0]?.value) ===
          opt.value
        "
        @select="playerStore.setOnlineQuality(opt.value)"
      />
    </template>
  </template>

  <div v-if="hasQualityMenuItems" class="my-1 border-t border-outline-variant" role="separator" />
  <MdMenuItem
    v-if="playerStore.isCurrentOnline"
    :icon="playerStore.isCurrentOnlineTracked ? LuCheck : LuLibrary"
    :label="
      playerStore.isCurrentOnlineTracked
        ? t('online.trackedInLibrary')
        : t('online.trackToLibrary')
    "
    :disabled="playerStore.trackingCurrentOnline"
    @select="playerStore.trackCurrentOnlineToLibrary()"
  />
  <MdMenuItem
    :icon="LuDownload"
    :label="t('common.download')"
    :disabled="downloadingCurrent"
    @select="emit('download')"
  />
  <template v-if="playerStore.currentLocalTrack">
    <MdMenuItem
      :icon="LuBookmark"
      :label="t('player.bookmarkBtn')"
      :disabled="savingBookmark"
      @select="emit('bookmark')"
    />
    <template v-if="showEntityLinks">
      <MdMenuItem
        :icon="LuUser"
        :label="t('common.viewArtist')"
        @select="emit('navigate', `/music/library?artist=${playerStore.currentLocalTrack.artist_id}`)"
      />
      <MdMenuItem
        :icon="LuDisc3"
        :label="t('common.viewAlbum')"
        @select="emit('navigate', `/music/item/${playerStore.currentLocalTrack.album_id}`)"
      />
    </template>
  </template>
</template>
