<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuAlertCircle, LuCheck, LuCheckCircle2, LuCopy, LuPlay, LuTrash2 } from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import { useThemeStore } from '@/stores/theme'
import { api as musicApi } from '@/domains/music/api'
import { usePlayerStore } from '@/domains/music/player'
import type { BookmarkDto } from '@/domains/music/types'
import { useBookI18n } from '@/domains/book/i18n'
import { useComicI18n } from '@/domains/comic/i18n'
import type {
  ComicFitMode,
  ComicReaderMode,
  ReadingFlow,
  TranscodeProfile,
  VideoQualityProfile,
} from '@/types/api'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSlider from '@/components/ui/AppSlider.vue'
import { useCopyFeedback } from './clipboard'

const { t } = useI18n()
const { t: bookT } = useBookI18n()
const { t: comicT } = useComicI18n()
const authStore = useAuthStore()
const playerStore = usePlayerStore()
const themeStore = useThemeStore()
const { copiedKey, copyText } = useCopyFeedback()

const originUrl = computed(() => window.location.origin)
const bookmarks = ref<BookmarkDto[]>([])

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

const protocolEndpoints = computed(() => {
  const list: Array<{ key: string; label: string; url: string }> = []
  if (authStore.hasModule('book')) {
    list.push(
      { key: 'book12', label: t('settings.bookOpds12Url'), url: `${originUrl.value}/opds/book/v1.2` },
      { key: 'book20', label: t('settings.bookOpds20Url'), url: `${originUrl.value}/opds/book/v2` },
    )
  }
  if (authStore.hasModule('comic')) {
    list.push({
      key: 'comicPse',
      label: t('settings.comicOpdsPseUrl'),
      url: `${originUrl.value}/opds/comic/v1.2`,
    })
  }
  if (authStore.hasModule('music')) {
    list.push(
      { key: 'subServer', label: t('settings.subsonicServerUrl'), url: originUrl.value },
      { key: 'subRest', label: t('settings.subsonicEndpointUrl'), url: `${originUrl.value}/rest` },
    )
  }
  return list
})

onMounted(async () => {
  if (authStore.hasModule('music')) {
    try {
      bookmarks.value = await musicApi.listBookmarks()
    } catch {
      // ignore
    }
  }
})

function formatBookmarkPos(ms: number): string {
  const totalSecs = Math.floor(ms / 1000)
  const m = Math.floor(totalSecs / 60)
  const s = totalSecs % 60
  return `${m}:${String(s).padStart(2, '0')}`
}

function playBookmark(bm: BookmarkDto): void {
  playerStore.playTrackAt(bm.track, bm.position_ms / 1000)
}

async function removeBookmark(trackId: string): Promise<void> {
  await musicApi.deleteBookmark(trackId)
  bookmarks.value = bookmarks.value.filter((b) => b.track.id !== trackId)
}
</script>

<template>
  <!-- FFmpeg Engine Status -->
  <section class="md-card-outlined p-6">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
          {{ t('settings.ffmpegStatusTitle') }}
        </h2>
        <p
          v-if="authStore.capabilities?.ffmpeg.ffmpeg_version"
          class="md-body-small mt-1 font-mono text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ authStore.capabilities.ffmpeg.ffmpeg_version }}
        </p>
      </div>
      <span
        v-if="authStore.capabilities?.ffmpeg.ffmpeg_available"
        class="md-label-medium inline-flex items-center gap-1.5 rounded-full bg-[var(--md-sys-color-success-container)] px-3 py-1 text-[var(--md-sys-color-on-success-container)]"
      >
        <LuCheckCircle2 class="h-4 w-4" />
        <span>{{ t('settings.ffmpegAvailable') }}</span>
      </span>
      <span
        v-else
        class="md-label-medium inline-flex items-center gap-1.5 rounded-full bg-[var(--md-sys-color-warning-container)] px-3 py-1 text-[var(--md-sys-color-on-warning-container)]"
      >
        <LuAlertCircle class="h-4 w-4" />
        <span>{{ t('settings.ffmpegMissing') }}</span>
      </span>
    </div>
  </section>

  <!-- Book Reader Preferences -->
  <section v-if="authStore.hasModule('book')" class="md-card-outlined space-y-4 p-6">
    <div>
      <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
        {{ t('settings.bookPrefsTitle') }}
      </h2>
      <p
        v-if="t('settings.bookPrefsDesc')"
        class="md-body-small mt-1 text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('settings.bookPrefsDesc') }}
      </p>
    </div>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ bookT('reader.fontFamily') }}
        </label>
        <AppSelect
          :model-value="themeStore.reader.font_family"
          :options="bookFontOptions"
          @update:model-value="themeStore.updateReaderPreferences({ font_family: $event })"
        />
      </div>

      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('settings.bookFlowLabel') }}
        </label>
        <AppSelect
          :model-value="themeStore.reader.flow"
          :options="bookFlowOptions"
          @update:model-value="themeStore.updateReaderPreferences({ flow: $event as ReadingFlow })"
        />
      </div>

      <div>
        <div class="mb-1.5 flex items-center justify-between text-xs">
          <span class="md-label-medium text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('settings.bookFontSize') }}
          </span>
          <span class="font-mono font-medium text-[var(--md-sys-color-on-surface)]">
            {{ themeStore.reader.font_size }} px
          </span>
        </div>
        <AppSlider
          :model-value="themeStore.reader.font_size"
          :min="13"
          :max="28"
          :step="1"
          :aria-label="t('settings.bookFontSize')"
          always-show-thumb
          @update:model-value="themeStore.updateReaderPreferences({ font_size: $event })"
        />
      </div>

      <div>
        <div class="mb-1.5 flex items-center justify-between text-xs">
          <span class="md-label-medium text-[var(--md-sys-color-on-surface-variant)]">
            {{ bookT('reader.lineHeight') }}
          </span>
          <span class="font-mono font-medium text-[var(--md-sys-color-on-surface)]">
            {{ themeStore.reader.line_height }}
          </span>
        </div>
        <AppSlider
          :model-value="themeStore.reader.line_height"
          :min="1.3"
          :max="2.4"
          :step="0.1"
          :aria-label="bookT('reader.lineHeight')"
          always-show-thumb
          @update:model-value="themeStore.updateReaderPreferences({ line_height: $event })"
        />
      </div>
    </div>
  </section>

  <!-- Comic Reader Preferences -->
  <section v-if="authStore.hasModule('comic')" class="md-card-outlined space-y-4 p-6">
    <div>
      <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
        {{ t('settings.comicPrefsTitle') }}
      </h2>
      <p
        v-if="t('settings.comicPrefsDesc')"
        class="md-body-small mt-1 text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('settings.comicPrefsDesc') }}
      </p>
    </div>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ comicT('settings.defaultReaderMode') }}
        </label>
        <AppSelect
          :model-value="themeStore.comic.default_reader_mode"
          :options="comicModeOptions"
          @update:model-value="
            themeStore.updateComicPreferences({ default_reader_mode: $event as ComicReaderMode })
          "
        />
      </div>

      <div>
        <label class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ comicT('settings.defaultFitMode') }}
        </label>
        <AppSelect
          :model-value="themeStore.comic.default_fit_mode"
          :options="comicFitOptions"
          @update:model-value="
            themeStore.updateComicPreferences({ default_fit_mode: $event as ComicFitMode })
          "
        />
      </div>
    </div>

    <div class="flex flex-wrap gap-6 pt-1">
      <AppCheckbox
        :model-value="themeStore.comic.offset_cover_page"
        :label="t('settings.comicOffsetCover')"
        @update:model-value="themeStore.updateComicPreferences({ offset_cover_page: $event })"
      />
      <AppCheckbox
        :model-value="themeStore.comic.dim_images_in_dark"
        :label="comicT('reader.eyeCareDim')"
        @update:model-value="themeStore.updateComicPreferences({ dim_images_in_dark: $event })"
      />
    </div>
  </section>

  <!-- Music Playback Preferences & Bookmarks -->
  <section v-if="authStore.hasModule('music')" class="md-card-outlined space-y-4 p-6">
    <div>
      <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
        {{ t('settings.playbackDefaultQuality') }}
      </h2>
      <p
        v-if="t('settings.playbackDefaultQualityDesc')"
        class="md-body-small mt-1 text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('settings.playbackDefaultQualityDesc') }}
      </p>
    </div>

    <div class="max-w-sm">
      <AppSelect
        :model-value="themeStore.playback.transcode_profile"
        :options="musicQualityOptions"
        @update:model-value="
          themeStore.updatePlaybackPreferences({ transcode_profile: $event as TranscodeProfile })
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
  <section v-if="authStore.hasModule('video')" class="md-card-outlined space-y-4 p-6">
    <div>
      <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
        {{ t('settings.videoPrefsTitle') }}
      </h2>
      <p
        v-if="t('settings.videoPrefsDesc')"
        class="md-body-small mt-1 text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('settings.videoPrefsDesc') }}
      </p>
    </div>

    <div class="max-w-md">
      <AppSelect
        :model-value="themeStore.video.default_quality"
        :options="videoQualityOptions"
        @update:model-value="
          themeStore.updateVideoPreferences({ default_quality: $event as VideoQualityProfile })
        "
      />
    </div>

    <AppCheckbox
      :model-value="themeStore.video.autoplay_next"
      :label="t('settings.videoAutoplayNext')"
      @update:model-value="themeStore.updateVideoPreferences({ autoplay_next: $event })"
    />
  </section>

  <!-- Open Protocols Client Guide (OPDS / PSE / OpenSubsonic) -->
  <section v-if="protocolEndpoints.length > 0" class="md-card-outlined space-y-4 p-6">
    <div>
      <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
        {{ t('settings.protocolsGuideTitle') }}
      </h2>
      <p class="md-body-small mt-1 leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('settings.protocolsGuideDesc') }}
      </p>
    </div>

    <div class="space-y-3">
      <div v-for="ep in protocolEndpoints" :key="ep.key">
        <label class="md-label-medium mb-1 block text-[var(--md-sys-color-on-surface-variant)]">
          {{ ep.label }}
        </label>
        <div class="flex items-center gap-2">
          <input
            readonly
            :value="ep.url"
            class="md-input h-9 min-w-0 flex-1 font-mono text-xs"
          />
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-filled shrink-0"
            @click="copyText(ep.key, ep.url)"
          >
            <LuCheck v-if="copiedKey === ep.key" class="h-3.5 w-3.5" />
            <LuCopy v-else class="h-3.5 w-3.5" />
            <span>{{ copiedKey === ep.key ? t('common.copied') : t('common.copy') }}</span>
          </button>
        </div>
      </div>
    </div>

    <div class="md-card-filled px-3.5 py-2.5 text-xs">
      <span class="font-semibold text-[var(--md-sys-color-on-surface)]">
        {{ t('settings.subsonicAuthModes') }}：
      </span>
      <span class="text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('settings.subsonicAuthModesVal') }}
      </span>
    </div>
  </section>
</template>
