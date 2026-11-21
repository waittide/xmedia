<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuPlay, LuTrash2 } from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import { usePreferencesStore } from '@/stores/preferences'
import { useSnackbarStore } from '@/stores/snackbar'
import { formatTime } from '@/utils/format'
import { api as musicApi } from '@/domains/music/api'
import { usePlayerStore } from '@/domains/music/player'
import type { BookmarkDto } from '@/domains/music/types'
import { useBookI18n } from '@/domains/book/i18n'
import { useComicI18n } from '@/domains/comic/i18n'
import type {
  ComicFitMode,
  ComicReaderMode,
  MediaKind,
  ReadingFlow,
  TranscodeProfile,
  VideoQualityProfile,
} from '@/types/api'
import LayoutCustomizerModal from '@/components/LayoutCustomizerModal.vue'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSlider from '@/components/ui/AppSlider.vue'

const props = defineProps<{
  domain: MediaKind
}>()

const { t } = useI18n()
const { t: bookT } = useBookI18n()
const { t: comicT } = useComicI18n()
const authStore = useAuthStore()
const playerStore = usePlayerStore()
const prefsStore = usePreferencesStore()

const bookmarks = ref<BookmarkDto[]>([])
const bookmarksLoaded = ref(false)

const bookFontOptions = computed(() => [
  { value: 'system-ui', label: bookT('reader.fontSystem') },
  { value: 'serif', label: bookT('reader.fontSerif') },
  { value: 'mono', label: bookT('reader.fontMono') },
])

const bookFlowOptions = computed(() => [
  { value: 'paginated', label: t('settings.bookFlowPaginated') },
  { value: 'scrolled', label: t('settings.bookFlowScrolled') },
])

const comicModeOptions = computed(() => [
  { value: 'rtl_double', label: t('settings.comicModeRtlDouble') },
  { value: 'ltr_single', label: t('settings.comicModeLtrSingle') },
  { value: 'ltr_double', label: t('settings.comicModeLtrDouble') },
  { value: 'webtoon', label: t('settings.comicModeWebtoon') },
])

const comicFitOptions = computed(() => [
  { value: 'fit_height', label: t('settings.comicFitHeight') },
  { value: 'fit_width', label: t('settings.comicFitWidth') },
  { value: 'original', label: t('settings.comicFitOriginal') },
])

const musicQualityOptions = computed(() => [
  { value: 'raw', label: t('player.qualityRaw') },
  { value: 'opus128', label: t('player.qualityOpus128') },
  { value: 'mp3192', label: t('player.qualityMp3192') },
  { value: 'mp3320', label: t('player.qualityMp3320') },
  { value: 'aac256', label: t('player.qualityAac256') },
])

const videoQualityOptions = computed(() => [
  { value: 'auto', label: t('settings.videoQualityAuto') },
  { value: 'direct', label: t('settings.videoQualityDirect') },
  { value: 'p1080', label: t('settings.videoQuality1080p') },
  { value: 'p720', label: t('settings.videoQuality720p') },
  { value: 'p480', label: t('settings.videoQuality480p') },
])

async function ensureMusicBookmarks(): Promise<void> {
  if (props.domain !== 'music' || !authStore.hasModule('music') || bookmarksLoaded.value) return
  bookmarksLoaded.value = true
  try {
    bookmarks.value = await musicApi.listBookmarks()
  } catch {
    bookmarksLoaded.value = false
  }
}

onMounted(() => {
  void ensureMusicBookmarks()
})

watch(
  () => props.domain,
  () => {
    void ensureMusicBookmarks()
  },
)

function formatBookmarkPos(ms: number): string {
  return formatTime(ms / 1000)
}

function playBookmark(bm: BookmarkDto): void {
  playerStore.playTrackAt(bm.track, bm.position_ms / 1000)
}

async function removeBookmark(trackId: string): Promise<void> {
  try {
    await musicApi.deleteBookmark(trackId)
    bookmarks.value = bookmarks.value.filter((b) => b.track.id !== trackId)
  } catch (err) {
    useSnackbarStore().show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}
</script>

<template>
  <!-- Book Reader Preferences -->
  <section v-if="domain === 'book'" class="md-card-outlined space-y-4 p-6">
    <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t('settings.bookPrefsTitle') }}
    </h2>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ bookT('reader.fontFamily') }}
        </label>
        <AppSelect
          :model-value="prefsStore.reader.font_family"
          :options="bookFontOptions"
          @update:model-value="prefsStore.updateReaderPreferences({ font_family: $event })"
        />
      </div>

      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('settings.bookFlowLabel') }}
        </label>
        <AppSelect
          :model-value="prefsStore.reader.flow"
          :options="bookFlowOptions"
          @update:model-value="prefsStore.updateReaderPreferences({ flow: $event as ReadingFlow })"
        />
      </div>

      <div>
        <div class="mb-1.5 flex items-center justify-between text-xs">
          <span class="md-label-medium text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('settings.bookFontSize') }}
          </span>
          <span class="font-mono font-medium text-[var(--md-sys-color-on-surface)]">
            {{ prefsStore.reader.font_size }} px
          </span>
        </div>
        <AppSlider
          :model-value="prefsStore.reader.font_size"
          :min="13"
          :max="28"
          :step="1"
          :aria-label="t('settings.bookFontSize')"
          always-show-thumb
          @update:model-value="prefsStore.updateReaderPreferences({ font_size: $event })"
        />
      </div>

      <div>
        <div class="mb-1.5 flex items-center justify-between text-xs">
          <span class="md-label-medium text-[var(--md-sys-color-on-surface-variant)]">
            {{ bookT('reader.lineHeight') }}
          </span>
          <span class="font-mono font-medium text-[var(--md-sys-color-on-surface)]">
            {{ prefsStore.reader.line_height }}
          </span>
        </div>
        <AppSlider
          :model-value="prefsStore.reader.line_height"
          :min="1.3"
          :max="2.4"
          :step="0.1"
          :aria-label="bookT('reader.lineHeight')"
          always-show-thumb
          @update:model-value="prefsStore.updateReaderPreferences({ line_height: $event })"
        />
      </div>
    </div>
  </section>

  <!-- Comic Reader Preferences -->
  <section v-else-if="domain === 'comic'" class="md-card-outlined space-y-4 p-6">
    <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t('settings.comicPrefsTitle') }}
    </h2>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ comicT('settings.defaultReaderMode') }}
        </label>
        <AppSelect
          :model-value="prefsStore.comic.default_reader_mode"
          :options="comicModeOptions"
          @update:model-value="
            prefsStore.updateComicPreferences({ default_reader_mode: $event as ComicReaderMode })
          "
        />
      </div>

      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ comicT('settings.defaultFitMode') }}
        </label>
        <AppSelect
          :model-value="prefsStore.comic.default_fit_mode"
          :options="comicFitOptions"
          @update:model-value="
            prefsStore.updateComicPreferences({ default_fit_mode: $event as ComicFitMode })
          "
        />
      </div>
    </div>

    <div class="flex flex-wrap gap-6 pt-1">
      <AppCheckbox
        :model-value="prefsStore.comic.offset_cover_page"
        :label="t('settings.comicOffsetCover')"
        @update:model-value="prefsStore.updateComicPreferences({ offset_cover_page: $event })"
      />
      <AppCheckbox
        :model-value="prefsStore.comic.dim_images_in_dark"
        :label="comicT('reader.eyeCareDim')"
        @update:model-value="prefsStore.updateComicPreferences({ dim_images_in_dark: $event })"
      />
    </div>
  </section>

  <!-- Music Playback Preferences & Bookmarks -->
  <section v-else-if="domain === 'music'" class="md-card-outlined space-y-4 p-6">
    <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t('settings.playbackDefaultQuality') }}
    </h2>

    <div class="max-w-sm">
      <AppSelect
        :model-value="prefsStore.playback.transcode_profile"
        :options="musicQualityOptions"
        @update:model-value="
          prefsStore.updatePlaybackPreferences({ transcode_profile: $event as TranscodeProfile })
        "
      />
    </div>

    <div
      v-if="bookmarks.length > 0"
      class="border-t border-[var(--md-sys-color-outline-variant)]/60 pt-4"
    >
      <h3 class="md-label-medium mb-2.5 text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('settings.bookmarksTitle') }}
      </h3>
      <div class="space-y-2">
        <div
          v-for="bm in bookmarks"
          :key="bm.track.id"
          class="md-card-filled flex items-center justify-between px-3.5 py-2.5 text-xs"
        >
          <div class="min-w-0 flex-1">
            <p class="truncate font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ bm.track.title }}
            </p>
            <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ bm.track.artist_name }} · {{ formatBookmarkPos(bm.position_ms) }}
            </p>
          </div>
          <div class="ml-3 flex items-center gap-1">
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm bg-[var(--md-sys-color-secondary-container)] !text-[var(--md-sys-color-on-secondary-container)]"
              @click="playBookmark(bm)"
            >
              <LuPlay class="h-3.5 w-3.5 fill-current" />
            </button>
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm hover:!text-[var(--md-sys-color-error)]"
              @click="removeBookmark(bm.track.id)"
            >
              <LuTrash2 class="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </div>
    </div>
  </section>

  <!-- Video Playback Preferences -->
  <section v-else-if="domain === 'video'" class="md-card-outlined space-y-4 p-6">
    <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t('settings.videoPrefsTitle') }}
    </h2>

    <div class="max-w-md">
      <AppSelect
        :model-value="prefsStore.video.default_quality"
        :options="videoQualityOptions"
        @update:model-value="
          prefsStore.updateVideoPreferences({ default_quality: $event as VideoQualityProfile })
        "
      />
    </div>

    <AppCheckbox
      :model-value="prefsStore.video.autoplay_next"
      :label="t('settings.videoAutoplayNext')"
      @update:model-value="prefsStore.updateVideoPreferences({ autoplay_next: $event })"
    />
  </section>

  <!-- Domain Workspace Layout Customizer -->
  <section class="md-card-outlined p-6">
    <h2 class="md-title-small mb-4 text-[var(--md-sys-color-on-surface)]">
      {{ t('layoutCustomizer.title') }}
    </h2>
    <LayoutCustomizerModal :domain="domain" inline hide-domain-tabs />
  </section>
</template>
