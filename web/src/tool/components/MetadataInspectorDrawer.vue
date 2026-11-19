<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import {
  LuBookOpen,
  LuCheck,
  LuClapperboard,
  LuDisc3,
  LuFileSignature,
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
import InspectorBookForm from './inspector/InspectorBookForm.vue'
import InspectorComicForm from './inspector/InspectorComicForm.vue'
import InspectorMusicForm from './inspector/InspectorMusicForm.vue'
import InspectorVideoForm from './inspector/InspectorVideoForm.vue'
import {
  isVideoFieldLockedIn,
  toggleVideoFieldLockIn,
  type BookInspectorForm,
  type ComicInspectorForm,
  type MusicInspectorForm,
  type VideoInspectorForm,
} from './inspector/types'
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
const baselineSnapshot = ref('')
const skipNextHydrate = ref(false)

// Domain-specific reactive form models
const polishingBook = ref(false)

const bookForm = reactive<BookInspectorForm>({
  title: '',
  authors: '',
  series: '',
  subseries: '',
  seriesIndex: '',
  isbn: '',
  publisher: '',
  publishedAt: '',
  language: '',
  tags: '',
  description: '',
  lockedFields: [],
})

const comicForm = reactive<ComicInspectorForm>({
  series: '',
  seriesGroup: '',
  title: '',
  number: '',
  volume: '',
  year: '',
  language: '',
  direction: 'rtl',
  writers: '',
  pencillers: '',
  publisher: '',
  genres: '',
  tags: '',
  summary: '',
})

const musicForm = reactive<MusicInspectorForm>({
  title: '',
  artist: '',
  albumArtist: '',
  album: '',
  track: '',
  disc: '',
  year: '',
  genre: '',
  comment: '',
  embeddedLyrics: '',
  sidecarLrc: '',
})

const videoForm = reactive<VideoInspectorForm>({
  isEpisode: false,
  title: '',
  originalTitle: '',
  showTitle: '',
  season: '',
  episode: '',
  year: '',
  rating: '',
  studio: '',
  directors: '',
  actors: '',
  genres: '',
  tags: '',
  tmdbId: '',
  bangumiId: '',
  summary: '',
  lockedFields: [],
})

function isBookFieldLocked(field: string): boolean {
  return bookForm.lockedFields.includes(field)
}

function toggleBookFieldLock(field: string): void {
  if (bookForm.lockedFields.includes(field)) {
    bookForm.lockedFields = bookForm.lockedFields.filter((f) => f !== field)
  } else {
    bookForm.lockedFields = [...bookForm.lockedFields, field]
  }
}

function isVideoFieldLocked(field: string): boolean {
  return isVideoFieldLockedIn(videoForm.lockedFields, field)
}

function toggleVideoFieldLock(field: string): void {
  videoForm.lockedFields = toggleVideoFieldLockIn(videoForm.lockedFields, field)
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

function splitListTokens(raw: string): string[] {
  return raw
    .split(/[/；;、\n]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
}

function serializeFormSnapshot(): string {
  return JSON.stringify([
    bookForm,
    [...bookForm.lockedFields].sort().join(','),
    comicForm,
    musicForm,
    videoForm,
    [...videoForm.lockedFields].sort().join(','),
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
    bookForm.title = b.title
    bookForm.authors = b.authors.join(' / ')
    bookForm.series = b.series ?? ''
    bookForm.subseries = b.subseries ?? ''
    bookForm.seriesIndex = b.series_index ?? ''
    bookForm.isbn = b.isbn ?? ''
    bookForm.publisher = b.publisher ?? ''
    bookForm.publishedAt = b.published_at ?? ''
    bookForm.language = b.language ?? ''
    bookForm.tags = b.tags.join(' / ')
    bookForm.description = b.description ?? ''
    bookForm.lockedFields = [...(b.locked_fields ?? [])]
  } else if (m.kind === 'comic') {
    const c = m.data
    comicForm.series = c.series ?? ''
    comicForm.seriesGroup = c.series_group ?? ''
    comicForm.title = c.title ?? ''
    comicForm.number = c.number ?? ''
    comicForm.volume = c.volume !== null ? String(c.volume) : ''
    comicForm.year = c.year !== null ? String(c.year) : ''
    comicForm.language = c.language_iso ?? ''
    comicForm.direction = c.reading_direction || 'rtl'
    comicForm.writers = c.writers.join(' / ')
    comicForm.pencillers = c.pencillers.join(' / ')
    comicForm.publisher = c.publisher ?? ''
    comicForm.genres = c.genres.join(' / ')
    comicForm.tags = c.tags.join(' / ')
    comicForm.summary = c.summary ?? ''
  } else if (m.kind === 'music') {
    const mu = m.data
    musicForm.title = mu.title
    musicForm.artist = mu.artist
    musicForm.albumArtist = mu.album_artist ?? ''
    musicForm.album = mu.album
    musicForm.track = mu.track_number !== null ? String(mu.track_number) : ''
    musicForm.disc = mu.disc_number !== null ? String(mu.disc_number) : ''
    musicForm.year = mu.year !== null ? String(mu.year) : ''
    musicForm.genre = mu.genre ?? ''
    musicForm.comment = mu.comment ?? ''
    musicForm.embeddedLyrics = mu.embedded_lyrics ?? ''
    musicForm.sidecarLrc = mu.sidecar_lrc ?? ''
  } else if (m.kind === 'video') {
    const v = m.data
    videoForm.isEpisode = v.is_episode
    videoForm.title = v.title
    videoForm.originalTitle = v.original_title ?? ''
    videoForm.showTitle = v.show_title ?? ''
    videoForm.season = v.season_number !== null ? String(v.season_number) : ''
    videoForm.episode = v.episode_number !== null ? String(v.episode_number) : ''
    videoForm.year = v.year !== null ? String(v.year) : ''
    videoForm.rating = v.rating !== null ? String(v.rating) : ''
    videoForm.studio = v.studio ?? ''
    videoForm.directors = v.directors.join(' / ')
    videoForm.actors = v.actors.join(' / ')
    videoForm.genres = v.genres.join(' / ')
    videoForm.tags = v.tags.join(' / ')
    videoForm.tmdbId = v.tmdb_id ?? ''
    videoForm.bangumiId = v.bangumi_id ?? ''
    videoForm.summary = v.summary ?? ''
    videoForm.lockedFields = [...(v.locked_fields ?? [])]
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
  requestClose,
  save: () => saveMetadata(false),
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
          title: bookForm.title,
          authors: splitListTokens(bookForm.authors),
          series: bookForm.series,
          subseries: bookForm.subseries,
          series_index: bookForm.seriesIndex,
          isbn: bookForm.isbn,
          publisher: bookForm.publisher,
          published_at: bookForm.publishedAt,
          language: bookForm.language,
          tags: splitListTokens(bookForm.tags),
          description: bookForm.description,
          locked_fields: bookForm.lockedFields,
        },
      }
    } else if (kind === 'comic') {
      envelope = {
        kind: 'comic',
        patch: {
          series: comicForm.series,
          series_group: comicForm.seriesGroup,
          title: comicForm.title,
          number: comicForm.number,
          volume: comicForm.volume.trim() ? Number.parseInt(comicForm.volume, 10) : -1,
          year: parseOptInt(comicForm.year, 0),
          language_iso: comicForm.language,
          reading_direction: comicForm.direction,
          writers: splitListTokens(comicForm.writers),
          pencillers: splitListTokens(comicForm.pencillers),
          publisher: comicForm.publisher,
          genres: splitListTokens(comicForm.genres),
          tags: splitListTokens(comicForm.tags),
          summary: comicForm.summary,
        },
      }
    } else if (kind === 'music') {
      envelope = {
        kind: 'music',
        patch: {
          title: musicForm.title,
          artist: musicForm.artist,
          album_artist: musicForm.albumArtist,
          album: musicForm.album,
          track_number: parseOptInt(musicForm.track, 0),
          disc_number: parseOptInt(musicForm.disc, 0),
          year: parseOptInt(musicForm.year, 0),
          genre: musicForm.genre,
          comment: musicForm.comment,
          embedded_lyrics: musicForm.embeddedLyrics,
          sidecar_lrc: musicForm.sidecarLrc,
        },
      }
    } else {
      const parsedRating = videoForm.rating.trim()
        ? Number.parseFloat(videoForm.rating)
        : 0
      envelope = {
        kind: 'video',
        patch: {
          is_episode: videoForm.isEpisode,
          title: videoForm.title,
          original_title: videoForm.originalTitle,
          show_title: videoForm.showTitle,
          season_number: parseClearableInt(videoForm.season),
          episode_number: parseOptInt(videoForm.episode, 0),
          year: parseOptInt(videoForm.year, 0),
          rating: Number.isFinite(parsedRating) && parsedRating > 0 ? parsedRating : 0,
          studio: videoForm.studio,
          directors: splitListTokens(videoForm.directors),
          actors: splitListTokens(videoForm.actors),
          genres: splitListTokens(videoForm.genres),
          tags: splitListTokens(videoForm.tags),
          tmdb_id: videoForm.tmdbId,
          bangumi_id: videoForm.bangumiId,
          summary: videoForm.summary,
          locked_fields: videoForm.lockedFields,
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
                v-if="videoForm.isEpisode"
                type="button"
                :disabled="uploadingArtwork"
                class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2.5 py-1.5 text-[11px] font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-50"
                @click="triggerArtworkUpload('thumb')"
              >
                <LuImage class="h-3.5 w-3.5" />
                <span>单集剧照</span>
              </button>
              <button
                type="button"
                class="inline-flex items-center gap-1 rounded-xl border px-2.5 py-1.5 text-[11px] font-medium transition"
                :class="
                  isVideoFieldLocked('poster')
                    ? 'border-amber-500/45 bg-amber-500/12 text-amber-500'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)] hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]'
                "
                title="锁定海报后，后续刮削将跳过覆盖当前海报与背景图"
                @click="toggleVideoFieldLock('poster')"
              >
                <LuLock v-if="isVideoFieldLocked('poster')" class="h-3.5 w-3.5" />
                <LuUnlock v-else class="h-3.5 w-3.5" />
                <span>{{ isVideoFieldLocked('poster') ? '海报已锁' : '锁海报' }}</span>
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

      <!-- Domain Metadata Forms -->
      <InspectorMusicForm
        v-if="detail.metadata.kind === 'music'"
        :form="musicForm"
        :sidecar-lrc-filename="detail.metadata.data.sidecar_lrc_filename"
        :file-key="`${detail.entry.root_id}:${detail.entry.rel_path}`"
      />

      <InspectorVideoForm
        v-else-if="detail.metadata.kind === 'video'"
        :form="videoForm"
      />

      <InspectorComicForm
        v-else-if="detail.metadata.kind === 'comic'"
        :form="comicForm"
      />

      <InspectorBookForm
        v-else-if="detail.metadata.kind === 'book'"
        :form="bookForm"
        :book-format="detail.metadata.data.format"
        :writable="detail.metadata.data.writable"
        :is-epub="detail.entry.extension === 'epub'"
        :has-cover="detail.metadata.data.has_cover"
        :polishing-book="polishingBook"
        @polish="handlePolishBook"
      />
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
