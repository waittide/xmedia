<script setup lang="ts">
import { computed, nextTick, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useBookI18n } from '@/domains/book/i18n'
import { useComicI18n } from '@/domains/comic/i18n'
import { AppSlider, MdCard, MdChip, MdSelect, MdSwitch } from '@/components/ui'
import { useAuthStore } from '@/stores/auth'
import { usePreferencesStore } from '@/stores/preferences'
import type {
  ComicFitMode,
  ComicReaderMode,
  MediaKind,
  ReadingFlow,
  TranscodeProfile,
  VideoQualityProfile,
} from '@/types/api'
import { DOMAIN_ICONS } from './sections'

const props = withDefaults(
  defineProps<{
    anchorHash?: string
    domain?: MediaKind
  }>(),
  {
    anchorHash: '',
    domain: undefined,
  },
)

const { t } = useI18n()
const { t: bookT } = useBookI18n()
const { t: comicT } = useComicI18n()
const authStore = useAuthStore()
const prefsStore = usePreferencesStore()

const enabledDomains = computed<MediaKind[]>(() =>
  props.domain ? [props.domain] : authStore.enabledModules,
)

function showDomain(kind: MediaKind): boolean {
  if (props.domain) return props.domain === kind && authStore.hasModule(kind)
  return authStore.hasModule(kind)
}

const activeAnchorDomain = computed<MediaKind | null>(() => {
  const clean = props.anchorHash.replace(/^#/, '')
  if (clean === 'book' || clean === 'comic' || clean === 'music' || clean === 'video') {
    return clean
  }
  return null
})

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

function scrollToDomainSection(kind: MediaKind, smooth = true): void {
  const target = document.getElementById(kind)
  target?.scrollIntoView({ behavior: smooth ? 'smooth' : 'auto', block: 'start' })
}

function syncHashScroll(smooth: boolean): void {
  if (!activeAnchorDomain.value) return
  const target = activeAnchorDomain.value
  void nextTick(() => scrollToDomainSection(target, smooth))
}

onMounted(() => {
  syncHashScroll(false)
})

watch(
  () => props.anchorHash,
  () => {
    syncHashScroll(true)
  },
)
</script>

<template>
  <div class="flex flex-col gap-5">
    <!-- Quick anchor chips across enabled domains -->
    <div
      v-if="!domain && enabledDomains.length > 1"
      class="no-scrollbar flex flex-nowrap items-center gap-2 overflow-x-auto py-0.5"
    >
      <MdChip
        v-for="kind in enabledDomains"
        :key="kind"
        variant="filter"
        hide-check
        :icon="DOMAIN_ICONS[kind]"
        :label="t(`app.domains.${kind}`)"
        :selected="activeAnchorDomain === kind"
        @click="scrollToDomainSection(kind, true)"
      />
    </div>

    <!-- #book: Book Reader Defaults -->
    <MdCard
      v-if="showDomain('book')"
      id="book"
      as="section"
      variant="tonal"
      padding="lg"
      class="scroll-mt-20 space-y-5"
    >
      <div class="flex items-center gap-2.5">
        <div
          class="flex h-9 w-9 shrink-0 items-center justify-center rounded-corner-md bg-secondary-container text-on-secondary-container"
        >
          <component :is="DOMAIN_ICONS.book" class="h-4 w-4" />
        </div>
        <div>
          <h2 class="text-title-medium text-on-surface">
            {{ t('settings.bookPrefsTitle') }}
          </h2>
          <p class="text-body-small text-on-surface-variant">
            {{ t('settings.bookPrefsDesc') }}
          </p>
        </div>
      </div>

      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div>
          <label class="text-label-medium mb-1.5 block text-on-surface-variant">
            {{ bookT('reader.fontFamily') }}
          </label>
          <MdSelect
            :model-value="prefsStore.reader.font_family"
            :options="bookFontOptions"
            @update:model-value="
              prefsStore.updateReaderPreferences(
                { font_family: $event },
                { immediate: true, notifyOnError: true },
              )
            "
          />
        </div>

        <div>
          <label class="text-label-medium mb-1.5 block text-on-surface-variant">
            {{ t('settings.bookFlowLabel') }}
          </label>
          <MdSelect
            :model-value="prefsStore.reader.flow"
            :options="bookFlowOptions"
            @update:model-value="
              prefsStore.updateReaderPreferences(
                { flow: $event as ReadingFlow },
                { immediate: true, notifyOnError: true },
              )
            "
          />
        </div>

        <div>
          <div class="mb-1.5 flex items-center justify-between">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('settings.bookFontSize') }}
            </span>
            <span class="text-label-medium font-mono tabular-nums text-on-surface">
              {{ prefsStore.reader.font_size }} px
            </span>
          </div>
          <AppSlider
            :model-value="prefsStore.reader.font_size"
            :min="14"
            :max="30"
            :step="1"
            :aria-label="t('settings.bookFontSize')"
            always-show-thumb
            @update:model-value="prefsStore.updateReaderPreferences({ font_size: $event })"
            @commit="void prefsStore.flushPendingSync()"
          />
        </div>

        <div>
          <div class="mb-1.5 flex items-center justify-between">
            <span class="text-label-medium text-on-surface-variant">
              {{ bookT('reader.lineHeight') }}
            </span>
            <span class="text-label-medium font-mono tabular-nums text-on-surface">
              {{ prefsStore.reader.line_height.toFixed(2) }}
            </span>
          </div>
          <AppSlider
            :model-value="prefsStore.reader.line_height"
            :min="1.3"
            :max="2.4"
            :step="0.05"
            :aria-label="bookT('reader.lineHeight')"
            always-show-thumb
            @update:model-value="prefsStore.updateReaderPreferences({ line_height: $event })"
            @commit="void prefsStore.flushPendingSync()"
          />
        </div>

        <div class="sm:col-span-2 sm:max-w-sm">
          <div class="mb-1.5 flex items-center justify-between">
            <span class="text-label-medium text-on-surface-variant">
              {{ bookT('reader.margin') }}
            </span>
            <span class="text-label-medium font-mono tabular-nums text-on-surface">
              {{ prefsStore.reader.margin_px }} px
            </span>
          </div>
          <AppSlider
            :model-value="prefsStore.reader.margin_px"
            :min="16"
            :max="120"
            :step="8"
            :aria-label="bookT('reader.margin')"
            always-show-thumb
            @update:model-value="prefsStore.updateReaderPreferences({ margin_px: $event })"
            @commit="void prefsStore.flushPendingSync()"
          />
        </div>
      </div>
    </MdCard>

    <!-- #comic: Comic Reader Defaults -->
    <MdCard
      v-if="showDomain('comic')"
      id="comic"
      as="section"
      variant="tonal"
      padding="lg"
      class="scroll-mt-20 space-y-5"
    >
      <div class="flex items-center gap-2.5">
        <div
          class="flex h-9 w-9 shrink-0 items-center justify-center rounded-corner-md bg-secondary-container text-on-secondary-container"
        >
          <component :is="DOMAIN_ICONS.comic" class="h-4 w-4" />
        </div>
        <div>
          <h2 class="text-title-medium text-on-surface">
            {{ t('settings.comicPrefsTitle') }}
          </h2>
          <p class="text-body-small text-on-surface-variant">
            {{ t('settings.comicPrefsDesc') }}
          </p>
        </div>
      </div>

      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div>
          <label class="text-label-medium mb-1.5 block text-on-surface-variant">
            {{ comicT('settings.defaultReaderMode') }}
          </label>
          <MdSelect
            :model-value="prefsStore.comic.default_reader_mode"
            :options="comicModeOptions"
            @update:model-value="
              prefsStore.updateComicPreferences(
                { default_reader_mode: $event as ComicReaderMode },
                { immediate: true, notifyOnError: true },
              )
            "
          />
        </div>

        <div>
          <label class="text-label-medium mb-1.5 block text-on-surface-variant">
            {{ comicT('settings.defaultFitMode') }}
          </label>
          <MdSelect
            :model-value="prefsStore.comic.default_fit_mode"
            :options="comicFitOptions"
            @update:model-value="
              prefsStore.updateComicPreferences(
                { default_fit_mode: $event as ComicFitMode },
                { immediate: true, notifyOnError: true },
              )
            "
          />
        </div>
      </div>

      <div class="flex flex-col gap-3 rounded-corner-md bg-surface-container px-4 py-3 sm:flex-row sm:flex-wrap sm:gap-8">
        <MdSwitch
          :model-value="prefsStore.comic.offset_cover_page"
          :label="t('settings.comicOffsetCover')"
          @update:model-value="
            prefsStore.updateComicPreferences(
              { offset_cover_page: $event },
              { immediate: true, notifyOnError: true },
            )
          "
        />
        <MdSwitch
          :model-value="prefsStore.comic.dim_images_in_dark"
          :label="comicT('reader.eyeCareDim')"
          @update:model-value="
            prefsStore.updateComicPreferences(
              { dim_images_in_dark: $event },
              { immediate: true, notifyOnError: true },
            )
          "
        />
      </div>
    </MdCard>

    <!-- #music: Music Playback Defaults (Bookmarks removed per §3.4) -->
    <MdCard
      v-if="showDomain('music')"
      id="music"
      as="section"
      variant="tonal"
      padding="lg"
      class="scroll-mt-20 space-y-5"
    >
      <div class="flex items-center gap-2.5">
        <div
          class="flex h-9 w-9 shrink-0 items-center justify-center rounded-corner-md bg-secondary-container text-on-secondary-container"
        >
          <component :is="DOMAIN_ICONS.music" class="h-4 w-4" />
        </div>
        <div>
          <h2 class="text-title-medium text-on-surface">
            {{ t('settings.playbackDefaultQuality') }}
          </h2>
          <p class="text-body-small text-on-surface-variant">
            {{ t('settings.musicPrefsDesc') }}
          </p>
        </div>
      </div>

      <div class="max-w-sm">
        <MdSelect
          :model-value="prefsStore.playback.transcode_profile"
          :options="musicQualityOptions"
          @update:model-value="
            prefsStore.updatePlaybackPreferences(
              { transcode_profile: $event as TranscodeProfile },
              { immediate: true, notifyOnError: true },
            )
          "
        />
      </div>
    </MdCard>

    <!-- #video: Video Playback Defaults -->
    <MdCard
      v-if="showDomain('video')"
      id="video"
      as="section"
      variant="tonal"
      padding="lg"
      class="scroll-mt-20 space-y-5"
    >
      <div class="flex items-center gap-2.5">
        <div
          class="flex h-9 w-9 shrink-0 items-center justify-center rounded-corner-md bg-secondary-container text-on-secondary-container"
        >
          <component :is="DOMAIN_ICONS.video" class="h-4 w-4" />
        </div>
        <div>
          <h2 class="text-title-medium text-on-surface">
            {{ t('settings.videoPrefsTitle') }}
          </h2>
          <p class="text-body-small text-on-surface-variant">
            {{ t('settings.videoPrefsDesc') }}
          </p>
        </div>
      </div>

      <div class="max-w-sm">
        <MdSelect
          :model-value="prefsStore.video.default_quality"
          :options="videoQualityOptions"
          @update:model-value="
            prefsStore.updateVideoPreferences(
              { default_quality: $event as VideoQualityProfile },
              { immediate: true, notifyOnError: true },
            )
          "
        />
      </div>

      <div class="rounded-corner-md bg-surface-container px-4 py-3">
        <MdSwitch
          :model-value="prefsStore.video.autoplay_next"
          :label="t('settings.videoAutoplayNext')"
          @update:model-value="
            prefsStore.updateVideoPreferences(
              { autoplay_next: $event },
              { immediate: true, notifyOnError: true },
            )
          "
        />
      </div>
    </MdCard>
  </div>
</template>
