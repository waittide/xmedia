<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuBookOpen,
  LuCheck,
  LuClapperboard,
  LuCopy,
  LuDisc3,
  LuFileSignature,
  LuFileText,
  LuImage,
  LuLayers,
  LuLock,
  LuRefreshCw,
  LuSave,
  LuSparkles,
  LuUnlock,
  LuUpload,
  LuWand2,
  LuX,
} from 'vue-icons-plus/lu'
import AppSelect from '@/components/ui/AppSelect.vue'
import { useDialogStore } from '@/stores/dialog'
import { buildArtworkUrl, toolApi } from '../api'
import { formatBytes, formatDuration, useToolText } from '../i18n'
import type {
  MediaFileDetail,
  MediaFileEntry,
  MediaMetadataPatchEnvelope,
} from '../types'

const props = defineProps<{
  detail: MediaFileDetail
  artworkVersion: number
}>()

const emit = defineEmits<{
  close: []
  updated: [detail: MediaFileDetail, bumpArtwork: boolean]
  openScrape: [detail: MediaFileDetail]
  openContentEditor: [entry: MediaFileEntry]
  openComicPages: [entry: MediaFileEntry]
  openMusicTools: [entry: MediaFileEntry]
  openVideoTools: [entry: MediaFileEntry]
}>()

const txt = useToolText()
const dialogStore = useDialogStore()

const saving = ref(false)
const saveFeedback = ref<string | null>(null)
const errorMsg = ref<string | null>(null)
const uploadingArtwork = ref(false)
const coverInputRef = ref<HTMLInputElement | null>(null)
const pendingSlot = ref<'cover' | 'series_cover' | 'poster' | 'fanart' | 'thumb'>('cover')
const videoArtworkSlot = ref<'poster' | 'fanart' | 'thumb'>('poster')
const lyricsTab = ref<'embedded' | 'sidecar'>('embedded')
const baselineSnapshot = ref('')
const skipNextHydrate = ref(false)

// Book form
const bookTitle = ref('')
const bookAuthors = ref('')
const bookSeries = ref('')
const bookSubseries = ref('')
const bookSeriesIndex = ref('')
const bookIsbn = ref('')
const bookPublisher = ref('')
const bookPublishedAt = ref('')
const bookLanguage = ref('')
const bookTags = ref('')
const bookDescription = ref('')
const bookLockedFields = ref<string[]>([])
const polishingBook = ref(false)

function isBookFieldLocked(field: string): boolean {
  return bookLockedFields.value.includes(field)
}

function toggleBookFieldLock(field: string): void {
  if (bookLockedFields.value.includes(field)) {
    bookLockedFields.value = bookLockedFields.value.filter((f) => f !== field)
  } else {
    bookLockedFields.value = [...bookLockedFields.value, field]
  }
}

function lockFilledBookFields(): void {
  const next = new Set(bookLockedFields.value)
  if (bookTitle.value.trim()) next.add('title')
  if (bookAuthors.value.trim()) next.add('authors')
  if (bookSeries.value.trim()) next.add('series')
  if (bookSubseries.value.trim()) next.add('subseries')
  if (bookSeriesIndex.value.trim()) next.add('series_index')
  if (bookIsbn.value.trim()) next.add('isbn')
  if (bookPublisher.value.trim()) next.add('publisher')
  if (bookPublishedAt.value.trim()) next.add('published_at')
  if (bookLanguage.value.trim()) next.add('language')
  if (bookTags.value.trim()) next.add('tags')
  if (bookDescription.value.trim()) next.add('description')
  if (props.detail.metadata.kind === 'book' && props.detail.metadata.data.has_cover) {
    next.add('cover')
  }
  bookLockedFields.value = Array.from(next)
}

async function handlePolishBook(): Promise<void> {
  if (polishingBook.value || props.detail.entry.extension !== 'epub') return
  polishingBook.value = true
  errorMsg.value = null
  saveFeedback.value = null
  try {
    if (isDirty.value) {
      await saveMetadata(false)
    }
    const res = await toolApi.polishBook(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
    )
    emit('updated', res.detail, false)
    saveFeedback.value =
      res.removed_orphan_files > 0
        ? `已更新内封简介页，清理 ${res.removed_orphan_files} 项冗余资源`
        : '已生成/更新 EPUB 内封简介页并同步双目录'
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    polishingBook.value = false
  }
}

// Comic form
const comicSeries = ref('')
const comicSeriesGroup = ref('')
const comicTitle = ref('')
const comicNumber = ref('')
const comicVolume = ref('')
const comicYear = ref('')
const comicLanguage = ref('')
const comicDirection = ref('rtl')
const comicWriters = ref('')
const comicPencillers = ref('')
const comicPublisher = ref('')
const comicGenres = ref('')
const comicTags = ref('')
const comicSummary = ref('')

// Music form
const musicTitle = ref('')
const musicArtist = ref('')
const musicAlbumArtist = ref('')
const musicAlbum = ref('')
const musicTrack = ref('')
const musicDisc = ref('')
const musicYear = ref('')
const musicGenre = ref('')
const musicComment = ref('')
const musicEmbeddedLyrics = ref('')
const musicSidecarLrc = ref('')

// Video form
const videoIsEpisode = ref(false)
const videoTitle = ref('')
const videoOriginalTitle = ref('')
const videoShowTitle = ref('')
const videoSeason = ref('')
const videoEpisode = ref('')
const videoYear = ref('')
const videoRating = ref('')
const videoStudio = ref('')
const videoDirectors = ref('')
const videoActors = ref('')
const videoGenres = ref('')
const videoTags = ref('')
const videoTmdbId = ref('')
const videoBangumiId = ref('')
const videoSummary = ref('')

function splitListTokens(raw: string): string[] {
  return raw
    .split(/[/；;、\n]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
}

function serializeFormSnapshot(): string {
  return JSON.stringify([
    bookTitle.value,
    bookAuthors.value,
    bookSeries.value,
    bookSubseries.value,
    bookSeriesIndex.value,
    bookIsbn.value,
    bookPublisher.value,
    bookPublishedAt.value,
    bookLanguage.value,
    bookTags.value,
    bookDescription.value,
    [...bookLockedFields.value].sort().join(','),
    comicSeries.value,
    comicSeriesGroup.value,
    comicTitle.value,
    comicNumber.value,
    comicVolume.value,
    comicYear.value,
    comicLanguage.value,
    comicDirection.value,
    comicWriters.value,
    comicPencillers.value,
    comicPublisher.value,
    comicGenres.value,
    comicTags.value,
    comicSummary.value,
    musicTitle.value,
    musicArtist.value,
    musicAlbumArtist.value,
    musicAlbum.value,
    musicTrack.value,
    musicDisc.value,
    musicYear.value,
    musicGenre.value,
    musicComment.value,
    musicEmbeddedLyrics.value,
    musicSidecarLrc.value,
    videoIsEpisode.value,
    videoTitle.value,
    videoOriginalTitle.value,
    videoShowTitle.value,
    videoSeason.value,
    videoEpisode.value,
    videoYear.value,
    videoRating.value,
    videoStudio.value,
    videoDirectors.value,
    videoActors.value,
    videoGenres.value,
    videoTags.value,
    videoTmdbId.value,
    videoBangumiId.value,
    videoSummary.value,
  ])
}

const isDirty = computed(
  () => baselineSnapshot.value !== '' && serializeFormSnapshot() !== baselineSnapshot.value,
)

function hydrateForm(d: MediaFileDetail): void {
  errorMsg.value = null
  saveFeedback.value = null
  const m = d.metadata
  if (m.kind === 'book') {
    const b = m.data
    bookTitle.value = b.title
    bookAuthors.value = b.authors.join(' / ')
    bookSeries.value = b.series ?? ''
    bookSubseries.value = b.subseries ?? ''
    bookSeriesIndex.value = b.series_index ?? ''
    bookIsbn.value = b.isbn ?? ''
    bookPublisher.value = b.publisher ?? ''
    bookPublishedAt.value = b.published_at ?? ''
    bookLanguage.value = b.language ?? ''
    bookTags.value = b.tags.join(' / ')
    bookDescription.value = b.description ?? ''
    bookLockedFields.value = [...(b.locked_fields ?? [])]
  } else if (m.kind === 'comic') {
    const c = m.data
    comicSeries.value = c.series ?? ''
    comicSeriesGroup.value = c.series_group ?? ''
    comicTitle.value = c.title ?? ''
    comicNumber.value = c.number ?? ''
    comicVolume.value = c.volume !== null ? String(c.volume) : ''
    comicYear.value = c.year !== null ? String(c.year) : ''
    comicLanguage.value = c.language_iso ?? ''
    comicDirection.value = c.reading_direction || 'rtl'
    comicWriters.value = c.writers.join(' / ')
    comicPencillers.value = c.pencillers.join(' / ')
    comicPublisher.value = c.publisher ?? ''
    comicGenres.value = c.genres.join(' / ')
    comicTags.value = c.tags.join(' / ')
    comicSummary.value = c.summary ?? ''
  } else if (m.kind === 'music') {
    const mu = m.data
    musicTitle.value = mu.title
    musicArtist.value = mu.artist
    musicAlbumArtist.value = mu.album_artist ?? ''
    musicAlbum.value = mu.album
    musicTrack.value = mu.track_number !== null ? String(mu.track_number) : ''
    musicDisc.value = mu.disc_number !== null ? String(mu.disc_number) : ''
    musicYear.value = mu.year !== null ? String(mu.year) : ''
    musicGenre.value = mu.genre ?? ''
    musicComment.value = mu.comment ?? ''
    musicEmbeddedLyrics.value = mu.embedded_lyrics ?? ''
    musicSidecarLrc.value = mu.sidecar_lrc ?? ''
    if (!mu.embedded_lyrics && mu.sidecar_lrc) {
      lyricsTab.value = 'sidecar'
    } else {
      lyricsTab.value = 'embedded'
    }
  } else if (m.kind === 'video') {
    const v = m.data
    videoIsEpisode.value = v.is_episode
    videoTitle.value = v.title
    videoOriginalTitle.value = v.original_title ?? ''
    videoShowTitle.value = v.show_title ?? ''
    videoSeason.value = v.season_number !== null ? String(v.season_number) : ''
    videoEpisode.value = v.episode_number !== null ? String(v.episode_number) : ''
    videoYear.value = v.year !== null ? String(v.year) : ''
    videoRating.value = v.rating !== null ? String(v.rating) : ''
    videoStudio.value = v.studio ?? ''
    videoDirectors.value = v.directors.join(' / ')
    videoActors.value = v.actors.join(' / ')
    videoGenres.value = v.genres.join(' / ')
    videoTags.value = v.tags.join(' / ')
    videoTmdbId.value = v.tmdb_id ?? ''
    videoBangumiId.value = v.bangumi_id ?? ''
    videoSummary.value = v.summary ?? ''
    if (v.has_poster) {
      videoArtworkSlot.value = 'poster'
    } else if (v.has_fanart) {
      videoArtworkSlot.value = 'fanart'
    } else if (v.has_thumb) {
      videoArtworkSlot.value = 'thumb'
    } else {
      videoArtworkSlot.value = 'poster'
    }
  }
  baselineSnapshot.value = serializeFormSnapshot()
}

watch(
  () => props.detail,
  (next) => {
    if (skipNextHydrate.value) {
      skipNextHydrate.value = false
      return
    }
    hydrateForm(next)
  },
  { immediate: true },
)

const artworkSrc = computed(() => {
  if (props.detail.metadata.kind === 'video') {
    const v = props.detail.metadata.data
    const slot = videoArtworkSlot.value
    const exists =
      slot === 'poster' ? v.has_poster : slot === 'fanart' ? v.has_fanart : v.has_thumb
    if (!exists) return null
    return buildArtworkUrl(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
      props.detail.entry.media_kind,
      props.artworkVersion,
      slot,
    )
  }
  if (!props.detail.entry.completeness.has_cover) return null
  return buildArtworkUrl(
    props.detail.entry.root_id,
    props.detail.entry.rel_path,
    props.detail.entry.media_kind,
    props.artworkVersion,
  )
})

const canSaveMetadata = computed(() => {
  if (props.detail.metadata.kind === 'book') {
    return props.detail.metadata.data.writable
  }
  return true
})

const directionOptions = [
  { value: 'rtl', label: '日漫右翻 (RTL)' },
  { value: 'ltr', label: '美漫左翻 (LTR)' },
  { value: 'webtoon', label: '条漫竖滑 (Webtoon)' },
]

function triggerArtworkUpload(
  slot: 'cover' | 'series_cover' | 'poster' | 'fanart' | 'thumb',
): void {
  pendingSlot.value = slot
  coverInputRef.value?.click()
}

async function handleArtworkPicked(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  uploadingArtwork.value = true
  errorMsg.value = null
  try {
    const next = await toolApi.uploadArtwork(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
      file,
      pendingSlot.value,
      props.detail.entry.media_kind,
    )
    if (pendingSlot.value === 'poster' || pendingSlot.value === 'fanart' || pendingSlot.value === 'thumb') {
      videoArtworkSlot.value = pendingSlot.value
    }
    // Preserve any unsaved text edits while refreshing artwork preview & server flags
    skipNextHydrate.value = true
    emit('updated', next, true)
    saveFeedback.value = txt.value.actions.saved
    setTimeout(() => {
      saveFeedback.value = null
    }, 2200)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    uploadingArtwork.value = false
  }
}

function syncLyricsBothWays(): void {
  if (lyricsTab.value === 'embedded') {
    musicSidecarLrc.value = musicEmbeddedLyrics.value
  } else {
    musicEmbeddedLyrics.value = musicSidecarLrc.value
  }
}

function parseOptInt(raw: string, fallbackZero = 0): number | undefined {
  const trimmed = raw.trim()
  if (!trimmed) return fallbackZero
  const n = Number.parseInt(trimmed, 10)
  return Number.isFinite(n) ? n : undefined
}

function parseClearableInt(raw: string): number {
  const trimmed = raw.trim()
  if (!trimmed) return -1
  const n = Number.parseInt(trimmed, 10)
  return Number.isFinite(n) && n >= 0 ? n : -1
}

async function confirmDiscardIfDirty(): Promise<boolean> {
  if (!isDirty.value) return true
  return dialogStore.confirm({
    title: '放弃未保存的修改？',
    message: '当前右侧表单有尚未写入原文件的更改，切换或关闭将丢失这些改动。',
    confirmText: '放弃改动',
    danger: true,
  })
}

async function requestClose(): Promise<void> {
  if (await confirmDiscardIfDirty()) {
    emit('close')
  }
}

defineExpose({
  isDirty,
  confirmDiscardIfDirty,
})

async function saveMetadata(syncArchivePath = false): Promise<void> {
  if (!canSaveMetadata.value || saving.value) return
  saving.value = true
  errorMsg.value = null
  saveFeedback.value = null

  try {
    let envelope: MediaMetadataPatchEnvelope
    const kind = props.detail.metadata.kind
    if (kind === 'book') {
      envelope = {
        kind: 'book',
        patch: {
          title: bookTitle.value,
          authors: splitListTokens(bookAuthors.value),
          series: bookSeries.value,
          subseries: bookSubseries.value,
          series_index: bookSeriesIndex.value,
          isbn: bookIsbn.value,
          publisher: bookPublisher.value,
          published_at: bookPublishedAt.value,
          language: bookLanguage.value,
          tags: splitListTokens(bookTags.value),
          description: bookDescription.value,
          locked_fields: bookLockedFields.value,
        },
      }
    } else if (kind === 'comic') {
      envelope = {
        kind: 'comic',
        patch: {
          series: comicSeries.value,
          series_group: comicSeriesGroup.value,
          title: comicTitle.value,
          number: comicNumber.value,
          volume: comicVolume.value.trim() ? Number.parseInt(comicVolume.value, 10) : -1,
          year: parseOptInt(comicYear.value, 0),
          language_iso: comicLanguage.value,
          reading_direction: comicDirection.value,
          writers: splitListTokens(comicWriters.value),
          pencillers: splitListTokens(comicPencillers.value),
          publisher: comicPublisher.value,
          genres: splitListTokens(comicGenres.value),
          tags: splitListTokens(comicTags.value),
          summary: comicSummary.value,
        },
      }
    } else if (kind === 'music') {
      envelope = {
        kind: 'music',
        patch: {
          title: musicTitle.value,
          artist: musicArtist.value,
          album_artist: musicAlbumArtist.value,
          album: musicAlbum.value,
          track_number: parseOptInt(musicTrack.value, 0),
          disc_number: parseOptInt(musicDisc.value, 0),
          year: parseOptInt(musicYear.value, 0),
          genre: musicGenre.value,
          comment: musicComment.value,
          embedded_lyrics: musicEmbeddedLyrics.value,
          sidecar_lrc: musicSidecarLrc.value,
        },
      }
    } else {
      const parsedRating = videoRating.value.trim()
        ? Number.parseFloat(videoRating.value)
        : 0
      envelope = {
        kind: 'video',
        patch: {
          is_episode: videoIsEpisode.value,
          title: videoTitle.value,
          original_title: videoOriginalTitle.value,
          show_title: videoShowTitle.value,
          season_number: parseClearableInt(videoSeason.value),
          episode_number: parseOptInt(videoEpisode.value, 0),
          year: parseOptInt(videoYear.value, 0),
          rating: Number.isFinite(parsedRating) && parsedRating > 0 ? parsedRating : 0,
          studio: videoStudio.value,
          directors: splitListTokens(videoDirectors.value),
          actors: splitListTokens(videoActors.value),
          genres: splitListTokens(videoGenres.value),
          tags: splitListTokens(videoTags.value),
          tmdb_id: videoTmdbId.value,
          bangumi_id: videoBangumiId.value,
          summary: videoSummary.value,
        },
      }
    }

    let updated = await toolApi.writeMetadata(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
      envelope,
    )

    if (syncArchivePath) {
      const plans = await toolApi.previewRename(
        updated.entry.root_id,
        [updated.entry.rel_path],
        null,
        updated.entry.media_kind,
      )
      const plan = plans[0]
      if (plan && !plan.unchanged) {
        if (plan.conflict) {
          throw new Error(`无法归档路径：${plan.conflict}`)
        }
        await toolApi.executeRename(updated.entry.root_id, [plan])
        updated = await toolApi.inspectFile(
          updated.entry.root_id,
          plan.to_rel_path,
          updated.entry.media_kind,
        )
      }
    }

    emit('updated', updated, false)
    saveFeedback.value = syncArchivePath ? '已保存并同步归档目录' : txt.value.actions.saved
    setTimeout(() => {
      saveFeedback.value = null
    }, 2200)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <aside
    class="flex h-full w-full flex-col border-l border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/45 lg:w-[440px] xl:w-[480px]"
  >
    <input
      ref="coverInputRef"
      type="file"
      accept="image/jpeg,image/png,image/webp"
      class="hidden"
      @change="handleArtworkPicked"
    />

    <!-- Inspector Top Header -->
    <div
      class="flex shrink-0 items-center justify-between gap-2 border-b border-[var(--ctp-surface0)]/70 px-4 py-3"
    >
      <div class="min-w-0">
        <div class="flex items-center gap-1.5">
          <span class="truncate text-xs font-bold text-[var(--ctp-text)]">
            {{ detail.entry.file_name }}
          </span>
          <span
            v-if="isDirty"
            class="shrink-0 rounded-full bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-amber-500"
          >
            未保存
          </span>
        </div>
        <div class="mt-0.5 truncate text-[11px] text-[var(--ctp-subtext0)]">
          {{ detail.entry.rel_path }} · {{ formatBytes(detail.entry.file_size) }}
        </div>
      </div>

      <div class="flex shrink-0 items-center gap-1.5">
        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent-soft)] px-2.5 py-1.5 text-xs font-semibold text-[var(--ctp-accent)] transition hover:opacity-90"
          @click="emit('openScrape', detail)"
        >
          <LuSparkles class="h-3.5 w-3.5" />
          <span>{{ txt.actions.onlineScrape }}</span>
        </button>
        <button
          type="button"
          class="flex h-8 w-8 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
          @click="requestClose"
        >
          <LuX class="h-4 w-4" />
        </button>
      </div>
    </div>

    <!-- Scrollable Inspector Body -->
    <div class="flex-1 space-y-4 overflow-y-auto p-4">
      <!-- Artwork & Media Specs Card -->
      <div
        class="flex gap-3.5 rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-canvas)] p-3"
      >
        <div class="flex shrink-0 flex-col items-center gap-1.5">
          <div
            class="relative overflow-hidden rounded-xl bg-[var(--ctp-mantle)]"
            :class="detail.entry.media_kind === 'music' ? 'h-24 w-24 album-shadow' : 'h-32 w-24 book-shadow'"
          >
            <img
              v-if="artworkSrc"
              :src="artworkSrc"
              alt="artwork"
              class="h-full w-full object-cover"
            />
            <div
              v-else
              class="flex h-full w-full flex-col items-center justify-center gap-1 text-[var(--ctp-subtext0)]"
            >
              <LuBookOpen v-if="detail.entry.media_kind === 'book'" class="h-6 w-6 opacity-55" />
              <LuLayers v-else-if="detail.entry.media_kind === 'comic'" class="h-6 w-6 opacity-55" />
              <LuDisc3 v-else-if="detail.entry.media_kind === 'music'" class="h-6 w-6 opacity-55" />
              <LuClapperboard v-else class="h-6 w-6 opacity-55" />
              <span class="text-[10px]">无封面</span>
            </div>
          </div>

          <!-- Video Multi-Slot Artwork Preview Tabs -->
          <div
            v-if="detail.metadata.kind === 'video'"
            class="flex items-center gap-0.5 rounded-lg bg-[var(--ctp-mantle)] p-0.5 text-[10px]"
          >
            <button
              v-for="slotTab in [
                { id: 'poster' as const, label: '海报', ok: detail.metadata.data.has_poster },
                { id: 'fanart' as const, label: '背景', ok: detail.metadata.data.has_fanart },
                { id: 'thumb' as const, label: '剧照', ok: detail.metadata.data.has_thumb },
              ]"
              :key="slotTab.id"
              type="button"
              class="inline-flex items-center gap-0.5 rounded-md px-1.5 py-0.5 font-medium transition"
              :class="
                videoArtworkSlot === slotTab.id
                  ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                  : slotTab.ok
                    ? 'text-[var(--ctp-text)]'
                    : 'text-[var(--ctp-subtext0)]/60'
              "
              @click="videoArtworkSlot = slotTab.id"
            >
              <span>{{ slotTab.label }}</span>
            </button>
          </div>
        </div>

        <div class="flex min-w-0 flex-1 flex-col justify-between gap-2">
          <div>
            <div class="flex flex-wrap items-center gap-1.5">
              <span
                class="rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 font-mono text-[10px] font-bold uppercase text-[var(--ctp-accent)]"
              >
                {{ detail.entry.extension }}
              </span>

              <!-- Music Specs -->
              <template v-if="detail.metadata.kind === 'music'">
                <span class="rounded-md bg-[var(--ctp-mantle)] px-2 py-0.5 text-[10px] text-[var(--ctp-subtext1)]">
                  {{ formatDuration(detail.metadata.data.duration_secs) }}
                </span>
                <span
                  v-if="detail.metadata.data.bit_rate_kbps"
                  class="rounded-md bg-[var(--ctp-mantle)] px-2 py-0.5 text-[10px] text-[var(--ctp-subtext1)]"
                >
                  {{ detail.metadata.data.bit_rate_kbps }} kbps
                </span>
                <span
                  v-if="detail.metadata.data.sample_rate_hz"
                  class="rounded-md bg-[var(--ctp-mantle)] px-2 py-0.5 text-[10px] text-[var(--ctp-subtext1)]"
                >
                  {{ (detail.metadata.data.sample_rate_hz / 1000).toFixed(1) }} kHz
                </span>
              </template>

              <!-- Comic Specs -->
              <template v-else-if="detail.metadata.kind === 'comic'">
                <span class="rounded-md bg-[var(--ctp-mantle)] px-2 py-0.5 text-[10px] text-[var(--ctp-subtext1)]">
                  {{ detail.metadata.data.page_count }} 页
                </span>
                <span class="rounded-md bg-[var(--ctp-mantle)] px-2 py-0.5 text-[10px] text-[var(--ctp-subtext1)]">
                  {{ detail.metadata.data.has_comic_info_xml ? '含 ComicInfo.xml' : '无 ComicInfo.xml' }}
                </span>
              </template>

              <!-- Video Specs -->
              <template v-else-if="detail.metadata.kind === 'video'">
                <span
                  v-if="detail.metadata.data.width && detail.metadata.data.height"
                  class="rounded-md bg-[var(--ctp-mantle)] px-2 py-0.5 text-[10px] text-[var(--ctp-subtext1)]"
                >
                  {{ detail.metadata.data.width }}×{{ detail.metadata.data.height }}
                </span>
                <span
                  v-if="detail.metadata.data.video_codec"
                  class="rounded-md bg-[var(--ctp-mantle)] px-2 py-0.5 font-mono text-[10px] uppercase text-[var(--ctp-subtext1)]"
                >
                  {{ detail.metadata.data.video_codec }}
                </span>
                <span class="rounded-md bg-[var(--ctp-mantle)] px-2 py-0.5 text-[10px] text-[var(--ctp-subtext1)]">
                  {{ detail.metadata.data.has_nfo ? detail.metadata.data.nfo_filename : '未生成 .nfo' }}
                </span>
              </template>

              <!-- Book Specs -->
              <template v-else-if="detail.metadata.kind === 'book'">
                <span class="rounded-md bg-[var(--ctp-mantle)] px-2 py-0.5 text-[10px] text-[var(--ctp-subtext1)]">
                  {{
                    detail.metadata.data.format === 'epub'
                      ? 'EPUB OPF 内封读写'
                      : `${detail.metadata.data.format.toUpperCase()} · 旁路 .opf 元数据与封面`
                  }}
                </span>
              </template>
            </div>

            <!-- Video Audio & Subtitle Stream Summary -->
            <div
              v-if="detail.metadata.kind === 'video'"
              class="mt-2 space-y-1 text-[11px] text-[var(--ctp-subtext0)]"
            >
              <div v-if="detail.metadata.data.audio_streams.length > 0" class="truncate">
                音轨 ({{ detail.metadata.data.audio_streams.length }}):
                {{
                  detail.metadata.data.audio_streams
                    .map((a) => `${a.codec.toUpperCase()}${a.language ? ` [${a.language}]` : ''}`)
                    .join(' · ')
                }}
              </div>
              <div
                v-if="
                  detail.metadata.data.subtitle_streams.length > 0 ||
                  detail.metadata.data.sidecar_subtitles.length > 0
                "
                class="truncate"
              >
                字幕: 内封 {{ detail.metadata.data.subtitle_streams.length }} 轨 · 外挂
                {{ detail.metadata.data.sidecar_subtitles.length }} 个
              </div>
            </div>
          </div>

          <div class="flex flex-wrap gap-1.5">
            <template v-if="detail.entry.media_kind !== 'video'">
              <button
                type="button"
                :disabled="uploadingArtwork"
                class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2.5 py-1.5 text-[11px] font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
                @click="triggerArtworkUpload('cover')"
              >
                <LuUpload class="h-3.5 w-3.5" />
                <span>{{
                  detail.entry.media_kind === 'comic' ? '替换单话封面' : txt.actions.uploadCover
                }}</span>
              </button>
              <button
                v-if="
                  detail.entry.media_kind === 'comic' && detail.entry.rel_path.includes('/')
                "
                type="button"
                :disabled="uploadingArtwork"
                class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2.5 py-1.5 text-[11px] font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
                @click="triggerArtworkUpload('series_cover')"
              >
                <LuImage class="h-3.5 w-3.5" />
                <span>上传系列封面</span>
              </button>
              <button
                v-if="detail.metadata.kind === 'book' && detail.metadata.data.writable"
                type="button"
                class="inline-flex items-center gap-1 rounded-xl border px-2.5 py-1.5 text-[11px] font-medium transition"
                :class="
                  isBookFieldLocked('cover')
                    ? 'border-amber-500/45 bg-amber-500/12 text-amber-500'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)] hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]'
                "
                title="锁定封面后，单本或批量刮削将跳过覆盖当前封面"
                @click="toggleBookFieldLock('cover')"
              >
                <LuLock v-if="isBookFieldLocked('cover')" class="h-3.5 w-3.5" />
                <LuUnlock v-else class="h-3.5 w-3.5" />
                <span>{{ isBookFieldLocked('cover') ? '封面已锁' : '锁封面' }}</span>
              </button>
              <button
                v-if="
                  detail.entry.media_kind === 'book' &&
                  (detail.entry.extension === 'txt' || detail.entry.extension === 'epub')
                "
                type="button"
                class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent-soft)] px-2.5 py-1.5 text-[11px] font-semibold text-[var(--ctp-accent)] transition hover:opacity-90"
                @click="emit('openContentEditor', detail.entry)"
              >
                <LuFileText class="h-3.5 w-3.5" />
                <span>{{
                  detail.entry.extension === 'txt'
                    ? '编辑目录与正文 · 转 EPUB'
                    : '编辑目录与正文'
                }}</span>
              </button>

              <button
                v-if="detail.entry.media_kind === 'comic'"
                type="button"
                class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent-soft)] px-2.5 py-1.5 text-[11px] font-semibold text-[var(--ctp-accent)] transition hover:opacity-90"
                @click="emit('openComicPages', detail.entry)"
              >
                <LuImage class="h-3.5 w-3.5" />
                <span>页面管理与瘦身</span>
              </button>

              <button
                v-if="detail.entry.media_kind === 'music'"
                type="button"
                class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent-soft)] px-2.5 py-1.5 text-[11px] font-semibold text-[var(--ctp-accent)] transition hover:opacity-90"
                @click="emit('openMusicTools', detail.entry)"
              >
                <LuMusic class="h-3.5 w-3.5" />
                <span>歌词 / 响度 / 封面工具</span>
              </button>

              <button
                v-if="detail.entry.media_kind === 'book' && detail.entry.extension === 'epub'"
                type="button"
                :disabled="polishingBook"
                class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-accent)]/35 bg-[var(--ctp-mantle)] px-2.5 py-1.5 text-[11px] font-semibold text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent-soft)] disabled:opacity-50"
                title="自动在正文前生成/更新内封书籍简介页，同步 EPUB2/3 双目录，并清理未引用冗余资源"
                @click="handlePolishBook"
              >
                <LuRefreshCw v-if="polishingBook" class="h-3.5 w-3.5 animate-spin" />
                <LuWand2 v-else class="h-3.5 w-3.5" />
                <span>{{ polishingBook ? '抛光中…' : '书籍抛光' }}</span>
              </button>
            </template>
            <template v-else>
              <button
                type="button"
                class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent-soft)] px-2.5 py-1.5 text-[11px] font-semibold text-[var(--ctp-accent)] transition hover:opacity-90"
                @click="emit('openVideoTools', detail.entry)"
              >
                <LuFilm class="h-3.5 w-3.5" />
                <span>字幕精校 / 抽取 / 剧集工具</span>
              </button>
              <button
                type="button"
                :disabled="uploadingArtwork"
                class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2.5 py-1.5 text-[11px] font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
                @click="triggerArtworkUpload('poster')"
              >
                <LuImage class="h-3.5 w-3.5" />
                <span>{{ txt.actions.uploadPoster }}</span>
              </button>
              <button
                type="button"
                :disabled="uploadingArtwork"
                class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2.5 py-1.5 text-[11px] font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
                @click="triggerArtworkUpload('fanart')"
              >
                <LuUpload class="h-3.5 w-3.5" />
                <span>{{ txt.actions.uploadFanart }}</span>
              </button>
              <button
                v-if="videoIsEpisode"
                type="button"
                :disabled="uploadingArtwork"
                class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2.5 py-1.5 text-[11px] font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
                @click="triggerArtworkUpload('thumb')"
              >
                <LuImage class="h-3.5 w-3.5" />
                <span>单集剧照</span>
              </button>
            </template>
          </div>
        </div>
      </div>

      <div
        v-if="errorMsg"
        class="rounded-xl border border-red-500/35 bg-red-500/10 px-3 py-2 text-xs text-red-400"
      >
        {{ errorMsg }}
      </div>

      <!-- 1. MUSIC FORM -->
      <div v-if="detail.metadata.kind === 'music'" class="space-y-3">
        <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">曲目标题 (Title)</span>
            <input
              v-model="musicTitle"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">歌手 / 艺术家 (Artist)</span>
            <input
              v-model="musicArtist"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">专辑艺术家 (Album Artist)</span>
            <input
              v-model="musicAlbumArtist"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">专辑名称 (Album)</span>
            <input
              v-model="musicAlbum"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <div class="grid grid-cols-3 gap-2 sm:col-span-2">
            <label class="block">
              <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">音轨号</span>
              <input
                v-model="musicTrack"
                type="number"
                min="0"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">碟号</span>
              <input
                v-model="musicDisc"
                type="number"
                min="0"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">发行年份</span>
              <input
                v-model="musicYear"
                type="number"
                min="0"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
          </div>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">音乐流派 (Genre)</span>
            <input
              v-model="musicGenre"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">备注 (Comment)</span>
            <input
              v-model="musicComment"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
        </div>

        <!-- Dual Lyrics Editor (Embedded + Sidecar .lrc) -->
        <div class="rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-canvas)] p-3">
          <div class="mb-2 flex items-center justify-between gap-2">
            <div class="flex items-center gap-1 rounded-xl bg-[var(--ctp-mantle)] p-1">
              <button
                type="button"
                class="rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
                :class="
                  lyricsTab === 'embedded'
                    ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                "
                @click="lyricsTab = 'embedded'"
              >
                内嵌歌词 ({{ musicEmbeddedLyrics.trim() ? '已有' : '空' }})
              </button>
              <button
                type="button"
                class="rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
                :class="
                  lyricsTab === 'sidecar'
                    ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                "
                @click="lyricsTab = 'sidecar'"
              >
                外挂 .lrc ({{ musicSidecarLrc.trim() ? '已有' : '空' }})
              </button>
            </div>

            <button
              type="button"
              class="inline-flex items-center gap-1 rounded-lg px-2 py-1 text-[11px] text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent-soft)]"
              title="将当前面板歌词同步复制到另一侧"
              @click="syncLyricsBothWays"
            >
              <LuCopy class="h-3 w-3" />
              <span>同步到{{ lyricsTab === 'embedded' ? ' .lrc' : '内嵌' }}</span>
            </button>
          </div>

          <textarea
            v-if="lyricsTab === 'embedded'"
            v-model="musicEmbeddedLyrics"
            rows="7"
            placeholder="[00:12.30] 支持直接写入音频文件内嵌 USLT / Vorbis Lyrics 标签…"
            class="w-full resize-y rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/45 p-2.5 font-mono text-xs leading-relaxed text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
          />
          <textarea
            v-else
            v-model="musicSidecarLrc"
            rows="7"
            :placeholder="`保存时将原子写入同名歌词文件 ${detail.metadata.data.sidecar_lrc_filename}（留空则删除外挂 .lrc）…`"
            class="w-full resize-y rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/45 p-2.5 font-mono text-xs leading-relaxed text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
          />
        </div>
      </div>

      <!-- 2. VIDEO FORM -->
      <div v-else-if="detail.metadata.kind === 'video'" class="space-y-3">
        <div class="flex items-center gap-1 rounded-xl bg-[var(--ctp-canvas)] p-1 border border-[var(--ctp-surface0)]/75">
          <button
            type="button"
            class="flex-1 rounded-lg py-1.5 text-xs font-semibold transition"
            :class="
              !videoIsEpisode
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
            "
            @click="videoIsEpisode = false"
          >
            电影 NFO (&lt;movie&gt;)
          </button>
          <button
            type="button"
            class="flex-1 rounded-lg py-1.5 text-xs font-semibold transition"
            :class="
              videoIsEpisode
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
            "
            @click="videoIsEpisode = true"
          >
            剧集单集 NFO (&lt;episodedetails&gt;)
          </button>
        </div>

        <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">
              {{ videoIsEpisode ? '单集标题 (Episode Title)' : '电影标题 (Title)' }}
            </span>
            <input
              v-model="videoTitle"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label v-if="videoIsEpisode" class="block sm:col-span-2">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">所属剧集名 (Show Title)</span>
            <input
              v-model="videoShowTitle"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">原名 (Original Title)</span>
            <input
              v-model="videoOriginalTitle"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>

          <div v-if="videoIsEpisode" class="grid grid-cols-2 gap-2 sm:col-span-2">
            <label class="block">
              <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">季号 (Season)</span>
              <input
                v-model="videoSeason"
                type="number"
                min="0"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">集号 (Episode)</span>
              <input
                v-model="videoEpisode"
                type="number"
                min="0"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
          </div>

          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">年份 (Year)</span>
            <input
              v-model="videoYear"
              type="number"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">评分 (Rating)</span>
            <input
              v-model="videoRating"
              type="number"
              step="0.1"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">TMDB ID</span>
            <input
              v-model="videoTmdbId"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">Bangumi ID</span>
            <input
              v-model="videoBangumiId"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">导演（`/` 分隔）</span>
            <input
              v-model="videoDirectors"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">制片厂 / 动画制作</span>
            <input
              v-model="videoStudio"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">演员 / 声优（`/` 分隔）</span>
            <input
              v-model="videoActors"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">类型 (Genres)</span>
            <input
              v-model="videoGenres"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">标签 (Tags)</span>
            <input
              v-model="videoTags"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">剧情简介 (Plot)</span>
            <textarea
              v-model="videoSummary"
              rows="5"
              class="w-full resize-y rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-2.5 text-xs leading-relaxed text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
        </div>
      </div>

      <!-- 3. COMIC FORM -->
      <div v-else-if="detail.metadata.kind === 'comic'" class="space-y-2.5">
        <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">系列 / 部名 (Series，对应一级连载目录)</span>
            <input
              v-model="comicSeries"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">总系列 / 系列组 (SeriesGroup，分多部作品选填)</span>
            <input
              v-model="comicSeriesGroup"
              type="text"
              placeholder="如：JOJO的奇妙冒险（单部作品留空）"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">话/卷标题 (Title)</span>
            <input
              v-model="comicTitle"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <div class="grid grid-cols-3 gap-2 sm:col-span-2">
            <label class="block">
              <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">话号 (Number)</span>
              <input
                v-model="comicNumber"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">卷号 (Volume)</span>
              <input
                v-model="comicVolume"
                type="number"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">年份</span>
              <input
                v-model="comicYear"
                type="number"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
          </div>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">原作者 / 编剧 (Writer)</span>
            <input
              v-model="comicWriters"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">作画 (Penciller)</span>
            <input
              v-model="comicPencillers"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">出版社</span>
            <input
              v-model="comicPublisher"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <div class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">阅读方向</span>
            <AppSelect v-model="comicDirection" :options="directionOptions" />
          </div>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">分类 (Genre)</span>
            <input
              v-model="comicGenres"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">标签 (Tags)</span>
            <input
              v-model="comicTags"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
          <label class="block sm:col-span-2">
            <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">简介 (Summary)</span>
            <textarea
              v-model="comicSummary"
              rows="5"
              class="w-full resize-y rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-2.5 text-xs leading-relaxed text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
          </label>
        </div>
      </div>

      <!-- 4. BOOK FORM -->
      <div v-else-if="detail.metadata.kind === 'book'" class="space-y-2.5">
        <div
          v-if="detail.metadata.data.format !== 'epub'"
          class="flex items-center gap-2 rounded-xl border border-[var(--ctp-accent)]/35 bg-[var(--ctp-accent-soft)]/50 px-3 py-2 text-xs text-[var(--ctp-accent)]"
        >
          <LuFileText class="h-4 w-4 shrink-0" />
          <span>当前为 {{ detail.metadata.data.format.toUpperCase() }} 文件，单书目录按 Calibre 规范写入 metadata.opf / cover，多书混放目录写入同名旁路文件，归档移动自动同步。</span>
        </div>

        <div
          v-if="detail.metadata.data.writable"
          class="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/55 px-3 py-1.5 text-[11px]"
        >
          <span class="flex items-center gap-1.5 text-[var(--ctp-subtext1)]">
            <LuLock class="h-3.5 w-3.5 text-amber-500" />
            <span>
              防刮削字段锁：已锁
              <strong class="text-[var(--ctp-text)]">{{ bookLockedFields.length }}</strong>
              项（保存后单本/批量刮削均跳过被锁字段）
            </span>
          </span>
          <div class="flex items-center gap-1.5">
            <button
              type="button"
              class="rounded-md px-2 py-0.5 font-medium text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent-soft)]"
              @click="lockFilledBookFields"
            >
              锁非空项
            </button>
            <button
              v-if="bookLockedFields.length > 0"
              type="button"
              class="rounded-md px-2 py-0.5 font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
              @click="bookLockedFields = []"
            >
              全部解锁
            </button>
          </div>
        </div>

        <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
          <div class="block sm:col-span-2">
            <div class="mb-1 flex items-center justify-between">
              <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">书名 (dc:title)</span>
              <button
                v-if="detail.metadata.data.writable"
                type="button"
                class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
                :class="isBookFieldLocked('title') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
                title="锁定/解锁书名字段"
                @click="toggleBookFieldLock('title')"
              >
                <LuLock v-if="isBookFieldLocked('title')" class="h-3 w-3" />
                <LuUnlock v-else class="h-3 w-3" />
              </button>
            </div>
            <input
              v-model="bookTitle"
              :disabled="!detail.metadata.data.writable"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
            />
          </div>
          <div class="block sm:col-span-2">
            <div class="mb-1 flex items-center justify-between">
              <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">作者 (dc:creator，`/` 分隔)</span>
              <button
                v-if="detail.metadata.data.writable"
                type="button"
                class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
                :class="isBookFieldLocked('authors') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
                title="锁定/解锁作者字段"
                @click="toggleBookFieldLock('authors')"
              >
                <LuLock v-if="isBookFieldLocked('authors')" class="h-3 w-3" />
                <LuUnlock v-else class="h-3 w-3" />
              </button>
            </div>
            <input
              v-model="bookAuthors"
              :disabled="!detail.metadata.data.writable"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
            />
          </div>
          <div class="grid grid-cols-1 gap-2 sm:col-span-2 sm:grid-cols-12">
            <div class="block sm:col-span-5">
              <div class="mb-1 flex items-center justify-between">
                <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">系列 / 丛书 (Series)</span>
                <button
                  v-if="detail.metadata.data.writable"
                  type="button"
                  class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
                  :class="isBookFieldLocked('series') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
                  title="锁定/解锁系列字段"
                  @click="toggleBookFieldLock('series')"
                >
                  <LuLock v-if="isBookFieldLocked('series')" class="h-3 w-3" />
                  <LuUnlock v-else class="h-3 w-3" />
                </button>
              </div>
              <input
                v-model="bookSeries"
                :disabled="!detail.metadata.data.writable"
                type="text"
                placeholder="如：三体 / 魔法禁书目录"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
              />
            </div>
            <div class="block sm:col-span-4">
              <div class="mb-1 flex items-center justify-between">
                <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">部 / 子系列 (Subseries)</span>
                <button
                  v-if="detail.metadata.data.writable"
                  type="button"
                  class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
                  :class="isBookFieldLocked('subseries') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
                  title="锁定/解锁子系列字段"
                  @click="toggleBookFieldLock('subseries')"
                >
                  <LuLock v-if="isBookFieldLocked('subseries')" class="h-3 w-3" />
                  <LuUnlock v-else class="h-3 w-3" />
                </button>
              </div>
              <input
                v-model="bookSubseries"
                :disabled="!detail.metadata.data.writable"
                type="text"
                placeholder="选填，如：新约"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
              />
            </div>
            <div class="block sm:col-span-3">
              <div class="mb-1 flex items-center justify-between">
                <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">卷 / 册号 (Volume)</span>
                <button
                  v-if="detail.metadata.data.writable"
                  type="button"
                  class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
                  :class="isBookFieldLocked('series_index') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
                  title="锁定/解锁卷册号字段"
                  @click="toggleBookFieldLock('series_index')"
                >
                  <LuLock v-if="isBookFieldLocked('series_index')" class="h-3 w-3" />
                  <LuUnlock v-else class="h-3 w-3" />
                </button>
              </div>
              <input
                v-model="bookSeriesIndex"
                :disabled="!detail.metadata.data.writable"
                type="text"
                placeholder="如：1 / 2.5"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
              />
            </div>
          </div>
          <div class="block">
            <div class="mb-1 flex items-center justify-between">
              <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">ISBN</span>
              <button
                v-if="detail.metadata.data.writable"
                type="button"
                class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
                :class="isBookFieldLocked('isbn') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
                title="锁定/解锁 ISBN"
                @click="toggleBookFieldLock('isbn')"
              >
                <LuLock v-if="isBookFieldLocked('isbn')" class="h-3 w-3" />
                <LuUnlock v-else class="h-3 w-3" />
              </button>
            </div>
            <input
              v-model="bookIsbn"
              :disabled="!detail.metadata.data.writable"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
            />
          </div>
          <div class="block">
            <div class="mb-1 flex items-center justify-between">
              <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">出版社 (dc:publisher)</span>
              <button
                v-if="detail.metadata.data.writable"
                type="button"
                class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
                :class="isBookFieldLocked('publisher') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
                title="锁定/解锁出版社"
                @click="toggleBookFieldLock('publisher')"
              >
                <LuLock v-if="isBookFieldLocked('publisher')" class="h-3 w-3" />
                <LuUnlock v-else class="h-3 w-3" />
              </button>
            </div>
            <input
              v-model="bookPublisher"
              :disabled="!detail.metadata.data.writable"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
            />
          </div>
          <div class="block">
            <div class="mb-1 flex items-center justify-between">
              <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">出版日期 (dc:date)</span>
              <button
                v-if="detail.metadata.data.writable"
                type="button"
                class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
                :class="isBookFieldLocked('published_at') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
                title="锁定/解锁出版日期"
                @click="toggleBookFieldLock('published_at')"
              >
                <LuLock v-if="isBookFieldLocked('published_at')" class="h-3 w-3" />
                <LuUnlock v-else class="h-3 w-3" />
              </button>
            </div>
            <input
              v-model="bookPublishedAt"
              :disabled="!detail.metadata.data.writable"
              type="text"
              placeholder="YYYY-MM-DD"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
            />
          </div>
          <div class="block">
            <div class="mb-1 flex items-center justify-between">
              <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">语言 (dc:language)</span>
              <button
                v-if="detail.metadata.data.writable"
                type="button"
                class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
                :class="isBookFieldLocked('language') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
                title="锁定/解锁语言"
                @click="toggleBookFieldLock('language')"
              >
                <LuLock v-if="isBookFieldLocked('language')" class="h-3 w-3" />
                <LuUnlock v-else class="h-3 w-3" />
              </button>
            </div>
            <input
              v-model="bookLanguage"
              :disabled="!detail.metadata.data.writable"
              type="text"
              placeholder="zh / en / ja"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
            />
          </div>
          <div class="block sm:col-span-2">
            <div class="mb-1 flex items-center justify-between">
              <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">标签 / 主题 (dc:subject，`/` 分隔)</span>
              <button
                v-if="detail.metadata.data.writable"
                type="button"
                class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
                :class="isBookFieldLocked('tags') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
                title="锁定/解锁标签"
                @click="toggleBookFieldLock('tags')"
              >
                <LuLock v-if="isBookFieldLocked('tags')" class="h-3 w-3" />
                <LuUnlock v-else class="h-3 w-3" />
              </button>
            </div>
            <input
              v-model="bookTags"
              :disabled="!detail.metadata.data.writable"
              type="text"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
            />
          </div>
          <div class="block sm:col-span-2">
            <div class="mb-1 flex items-center justify-between">
              <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">内容简介 (dc:description)</span>
              <button
                v-if="detail.metadata.data.writable"
                type="button"
                class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
                :class="isBookFieldLocked('description') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
                title="锁定/解锁内容简介"
                @click="toggleBookFieldLock('description')"
              >
                <LuLock v-if="isBookFieldLocked('description')" class="h-3 w-3" />
                <LuUnlock v-else class="h-3 w-3" />
              </button>
            </div>
            <textarea
              v-model="bookDescription"
              :disabled="!detail.metadata.data.writable"
              rows="5"
              class="w-full resize-y rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-2.5 text-xs leading-relaxed text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] disabled:opacity-55"
            />
          </div>
        </div>
      </div>
    </div>

    <!-- Bottom Sticky Save Bar -->
    <div
      class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-[var(--ctp-surface0)]/70 bg-[var(--ctp-canvas)]/85 px-4 py-3"
    >
      <span
        v-if="saveFeedback"
        class="inline-flex items-center gap-1.5 text-xs font-semibold text-emerald-500"
      >
        <LuCheck class="h-4 w-4" />
        <span>{{ saveFeedback }}</span>
      </span>
      <span v-else class="text-[11px] text-[var(--ctp-subtext0)]">
        原子落盘 · 支持同步归档路径
      </span>

      <div class="flex items-center gap-2">
        <button
          type="button"
          :disabled="!canSaveMetadata || saving"
          class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-3 py-2 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
          title="保存元信息并按默认模板同步重命名文件名与所属目录"
          @click="saveMetadata(true)"
        >
          <LuFileSignature class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
          <span>保存并归档路径</span>
        </button>

        <button
          type="button"
          :disabled="!canSaveMetadata || saving"
          class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-3.5 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
          @click="saveMetadata(false)"
        >
          <LuRefreshCw v-if="saving" class="h-3.5 w-3.5 animate-spin" />
          <LuSave v-else class="h-3.5 w-3.5" />
          <span>{{ saving ? txt.actions.saving : txt.actions.save }}</span>
        </button>
      </div>
    </div>
  </aside>
</template>
