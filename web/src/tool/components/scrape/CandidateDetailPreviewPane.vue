<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuImage,
  LuImageOff,
  LuLayers,
  LuLoader2,
  LuRotateCcw,
  LuSave,
  LuSkipForward,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import type { MediaKind } from '@/types/api'
import { resolveScraperCoverUrl } from '../../api'
import type { FieldSourceBadge, ScrapedMetadata } from '../../types'
import {
  splitListTokens,
  type ApplyFillMode,
} from '../inspector/useMetadataForm'

export interface CandidatePreviewState {
  key: string
  isMerged: boolean
  sourceLabel: string
  meta: ScrapedMetadata | null
  loading: boolean
  fieldSources: Record<string, FieldSourceBadge> | null
  fillMode: ApplyFillMode
}

export interface CandidateApplyOptions {
  organizeAfterSave?: boolean
  writeAlbumCover?: boolean
}

const props = withDefaults(
  defineProps<{
    mediaKind: MediaKind
    preview: CandidatePreviewState
    hasNext?: boolean
    closable?: boolean
    saving?: boolean
  }>(),
  {
    hasNext: false,
    closable: true,
    saving: false,
  },
)

const emit = defineEmits<{
  directApply: [meta: ScrapedMetadata, mode: ApplyFillMode, opts?: CandidateApplyOptions]
  directApplyNext: [meta: ScrapedMetadata, mode: ApplyFillMode, opts?: CandidateApplyOptions]
  close: []
}>()

const brokenCoverUrls = ref<Set<string>>(new Set())
const organizeAfterSave = ref(false)
const writeAlbumCover = ref(false)

interface DraftFields {
  title: string
  originalTitle: string
  creators: string
  directors: string
  cast: string
  seriesOrAlbum: string
  publisherOrStudio: string
  dateOrYear: string
  isbn: string
  rating: string
  language: string
  discOrSeason: string
  trackOrEpisode: string
  genres: string
  tags: string
  combinedBookTags: string
  summary: string
  lyrics: string
  useCover: boolean
}

const draft = reactive<DraftFields>({
  title: '',
  originalTitle: '',
  creators: '',
  directors: '',
  cast: '',
  seriesOrAlbum: '',
  publisherOrStudio: '',
  dateOrYear: '',
  isbn: '',
  rating: '',
  language: '',
  discOrSeason: '',
  trackOrEpisode: '',
  genres: '',
  tags: '',
  combinedBookTags: '',
  summary: '',
  lyrics: '',
  useCover: true,
})

const baselineJson = ref('')

function hydrateDraft(m: ScrapedMetadata | null): void {
  if (!m) {
    baselineJson.value = ''
    return
  }
  const combinedTags = Array.from(new Set([...(m.genres ?? []), ...(m.tags ?? [])].filter(Boolean)))
  draft.title = m.title ?? ''
  draft.originalTitle = m.original_title ?? ''
  draft.creators = (m.creators ?? []).join(' / ')
  draft.directors = (m.illustrators_or_directors ?? []).join(' / ')
  draft.cast = (m.cast ?? []).join(' / ')
  draft.seriesOrAlbum = m.series_or_album ?? ''
  draft.publisherOrStudio = m.publisher_or_studio ?? ''
  draft.dateOrYear = m.published_date || (m.year != null ? String(m.year) : '')
  draft.isbn = m.isbn ?? ''
  draft.rating = m.rating != null ? String(m.rating) : ''
  draft.language = m.language ?? ''
  draft.discOrSeason = m.disc_or_season_number != null ? String(m.disc_or_season_number) : ''
  draft.trackOrEpisode = m.track_or_episode_number != null ? String(m.track_or_episode_number) : ''
  draft.genres = (m.genres ?? []).join(' / ')
  draft.tags = (m.tags ?? []).join(' / ')
  draft.combinedBookTags = combinedTags.join(' / ')
  draft.summary = m.summary ?? ''
  draft.lyrics = m.synced_lyrics || m.plain_lyrics || ''
  draft.useCover = Boolean(m.cover_url)
  baselineJson.value = JSON.stringify(draft)
}

watch(
  () => [props.preview.key, props.preview.meta, props.preview.loading] as const,
  ([, meta]) => {
    hydrateDraft(meta)
  },
  { immediate: true },
)

const isDraftEdited = computed(() => {
  if (!baselineJson.value) return false
  return JSON.stringify(draft) !== baselineJson.value
})

function resetDraft(): void {
  hydrateDraft(props.preview.meta)
}

function buildEditedMetadata(): ScrapedMetadata | null {
  const base = props.preview.meta
  if (!base) return null

  const trimmedDate = draft.dateOrYear.trim()
  const yearMatch = trimmedDate.match(/^(\d{4})/)
  const parsedYear = yearMatch ? Number.parseInt(yearMatch[1]!, 10) : null
  const parsedRating = draft.rating.trim() ? Number.parseFloat(draft.rating.trim()) : null
  const parsedDisc = draft.discOrSeason.trim()
    ? Number.parseInt(draft.discOrSeason.trim(), 10)
    : null
  const parsedTrack = draft.trackOrEpisode.trim()
    ? Number.parseInt(draft.trackOrEpisode.trim(), 10)
    : null

  const lyricsText = draft.lyrics.trim()
  const hasTimestamps = /\[\d{1,2}:\d{2}/.test(lyricsText)

  return {
    ...base,
    title: draft.title.trim() || null,
    original_title: draft.originalTitle.trim() || null,
    creators: splitListTokens(draft.creators),
    illustrators_or_directors: splitListTokens(draft.directors),
    cast: splitListTokens(draft.cast),
    series_or_album: draft.seriesOrAlbum.trim() || null,
    publisher_or_studio: draft.publisherOrStudio.trim() || null,
    year: parsedYear !== null && Number.isFinite(parsedYear) ? parsedYear : null,
    published_date: trimmedDate || null,
    isbn: draft.isbn.trim() || null,
    rating: parsedRating !== null && Number.isFinite(parsedRating) ? parsedRating : null,
    language: draft.language.trim() || null,
    disc_or_season_number: parsedDisc !== null && Number.isFinite(parsedDisc) ? parsedDisc : null,
    track_or_episode_number:
      parsedTrack !== null && Number.isFinite(parsedTrack) ? parsedTrack : null,
    genres: props.mediaKind === 'book' ? [] : splitListTokens(draft.genres),
    tags:
      props.mediaKind === 'book'
        ? splitListTokens(draft.combinedBookTags)
        : splitListTokens(draft.tags),
    summary: draft.summary.trim() || null,
    cover_url: draft.useCover && isCoverUsable(base.cover_url) ? base.cover_url : null,
    synced_lyrics: lyricsText && hasTimestamps ? lyricsText : null,
    plain_lyrics: lyricsText && !hasTimestamps ? lyricsText : null,
  }
}

function isCoverUsable(url: string | null | undefined): boolean {
  const trimmed = url?.trim()
  return Boolean(trimmed && !brokenCoverUrls.value.has(trimmed))
}

function markCoverBroken(url: string | null | undefined): void {
  const trimmed = url?.trim()
  if (!trimmed || brokenCoverUrls.value.has(trimmed)) return
  const next = new Set(brokenCoverUrls.value)
  next.add(trimmed)
  brokenCoverUrls.value = next
}

function sourceBadgeFor(...keys: string[]): string | null {
  const map = props.preview.fieldSources
  if (!map) return null
  for (const k of keys) {
    const hit = map[k]
    if (hit?.provider_name) return hit.provider_name
  }
  return null
}

type DraftStringKey = Exclude<keyof DraftFields, 'useCover'>

interface EditableFieldSpec {
  id: string
  draftKey: DraftStringKey
  label: string
  placeholder: string
  multiline?: boolean
  rows?: number
  half?: boolean
  sourceName?: string | null
}

const fieldSpecs = computed<EditableFieldSpec[]>(() => {
  if (!props.preview.meta) return []

  if (props.mediaKind === 'book') {
    return [
      {
        id: 'title',
        draftKey: 'title',
        label: '书名 (dc:title)',
        placeholder: '书名',
        sourceName: sourceBadgeFor('title'),
      },
      {
        id: 'creators',
        draftKey: 'creators',
        label: '作者 (dc:creator，`/` 分隔)',
        placeholder: '作者',
        sourceName: sourceBadgeFor('creators'),
      },
      {
        id: 'series_or_album',
        draftKey: 'seriesOrAlbum',
        label: '系列 / 丛书 (Series)',
        placeholder: '如：三体 / 魔法禁书目录',
        sourceName: sourceBadgeFor('series_or_album'),
      },
      {
        id: 'isbn',
        draftKey: 'isbn',
        label: 'ISBN',
        placeholder: 'ISBN',
        half: true,
        sourceName: sourceBadgeFor('isbn'),
      },
      {
        id: 'publisher_or_studio',
        draftKey: 'publisherOrStudio',
        label: '出版社 (dc:publisher)',
        placeholder: '出版社',
        half: true,
        sourceName: sourceBadgeFor('publisher_or_studio'),
      },
      {
        id: 'year',
        draftKey: 'dateOrYear',
        label: '出版日期 (dc:date)',
        placeholder: 'YYYY-MM-DD',
        half: true,
        sourceName: sourceBadgeFor('published_date', 'year'),
      },
      {
        id: 'language',
        draftKey: 'language',
        label: '语言 (dc:language)',
        placeholder: 'zh / en / ja',
        half: true,
        sourceName: sourceBadgeFor('language'),
      },
      {
        id: 'tags',
        draftKey: 'combinedBookTags',
        label: '标签 / 主题 (dc:subject，`/` 分隔)',
        placeholder: '标签 / 主题',
        sourceName: sourceBadgeFor('tags', 'genres'),
      },
      {
        id: 'summary',
        draftKey: 'summary',
        label: '内容简介 (dc:description)',
        placeholder: '内容简介…',
        multiline: true,
        rows: 6,
        sourceName: sourceBadgeFor('summary'),
      },
    ]
  }

  if (props.mediaKind === 'comic') {
    return [
      {
        id: 'title',
        draftKey: 'title',
        label: '标题',
        placeholder: '漫画标题',
        sourceName: sourceBadgeFor('title'),
      },
      {
        id: 'series_or_album',
        draftKey: 'seriesOrAlbum',
        label: '系列 / 丛书',
        placeholder: '系列名称',
        sourceName: sourceBadgeFor('series_or_album', 'title'),
      },
      {
        id: 'creators',
        draftKey: 'creators',
        label: '作者 / 编剧',
        placeholder: '作者',
        half: true,
        sourceName: sourceBadgeFor('creators'),
      },
      {
        id: 'directors',
        draftKey: 'directors',
        label: '画师 / 作画',
        placeholder: '画师',
        half: true,
        sourceName: sourceBadgeFor('illustrators_or_directors'),
      },
      {
        id: 'publisher_or_studio',
        draftKey: 'publisherOrStudio',
        label: '出版社',
        placeholder: '出版社',
        half: true,
        sourceName: sourceBadgeFor('publisher_or_studio'),
      },
      {
        id: 'year',
        draftKey: 'dateOrYear',
        label: '年份 / 日期',
        placeholder: 'YYYY',
        half: true,
        sourceName: sourceBadgeFor('published_date', 'year'),
      },
      {
        id: 'track_or_episode',
        draftKey: 'trackOrEpisode',
        label: '卷 / 话号',
        placeholder: '如：1',
        half: true,
        sourceName: sourceBadgeFor('track_or_episode_number'),
      },
      {
        id: 'language',
        draftKey: 'language',
        label: '语言',
        placeholder: 'zh / ja / en',
        half: true,
        sourceName: sourceBadgeFor('language'),
      },
      {
        id: 'genres',
        draftKey: 'genres',
        label: '流派 / 分类',
        placeholder: '流派',
        sourceName: sourceBadgeFor('genres'),
      },
      {
        id: 'tags',
        draftKey: 'tags',
        label: '标签',
        placeholder: '标签',
        sourceName: sourceBadgeFor('tags'),
      },
      {
        id: 'summary',
        draftKey: 'summary',
        label: '内容简介',
        placeholder: '内容简介…',
        multiline: true,
        rows: 5,
        sourceName: sourceBadgeFor('summary'),
      },
    ]
  }

  if (props.mediaKind === 'music') {
    return [
      {
        id: 'title',
        draftKey: 'title',
        label: '曲目标题',
        placeholder: '曲目标题',
        sourceName: sourceBadgeFor('title'),
      },
      {
        id: 'creators',
        draftKey: 'creators',
        label: '艺人 / 歌手',
        placeholder: '艺人',
        sourceName: sourceBadgeFor('creators'),
      },
      {
        id: 'series_or_album',
        draftKey: 'seriesOrAlbum',
        label: '所属专辑',
        placeholder: '专辑名',
        sourceName: sourceBadgeFor('series_or_album'),
      },
      {
        id: 'disc_or_season',
        draftKey: 'discOrSeason',
        label: '碟号',
        placeholder: '1',
        half: true,
        sourceName: sourceBadgeFor('disc_or_season_number'),
      },
      {
        id: 'track_or_episode',
        draftKey: 'trackOrEpisode',
        label: '音轨号',
        placeholder: '1',
        half: true,
        sourceName: sourceBadgeFor('track_or_episode_number'),
      },
      {
        id: 'year',
        draftKey: 'dateOrYear',
        label: '发行年份',
        placeholder: 'YYYY',
        half: true,
        sourceName: sourceBadgeFor('year', 'published_date'),
      },
      {
        id: 'genres',
        draftKey: 'genres',
        label: '流派',
        placeholder: '流派',
        half: true,
        sourceName: sourceBadgeFor('genres'),
      },
      {
        id: 'lyrics',
        draftKey: 'lyrics',
        label: '歌词（支持 LRC 或纯文本）',
        placeholder: '暂无歌词…',
        multiline: true,
        rows: 7,
        sourceName: sourceBadgeFor('synced_lyrics', 'plain_lyrics'),
      },
    ]
  }

  return [
    {
      id: 'title',
      draftKey: 'title',
      label: '标题',
      placeholder: '影片/剧集标题',
      sourceName: sourceBadgeFor('title'),
    },
    {
      id: 'original_title',
      draftKey: 'originalTitle',
      label: '原作名',
      placeholder: '原作名',
      sourceName: sourceBadgeFor('original_title'),
    },
    {
      id: 'series_or_album',
      draftKey: 'seriesOrAlbum',
      label: '所属剧集 / 系列',
      placeholder: '剧集名',
      sourceName: sourceBadgeFor('series_or_album'),
    },
    {
      id: 'disc_or_season',
      draftKey: 'discOrSeason',
      label: '季号',
      placeholder: '1',
      half: true,
      sourceName: sourceBadgeFor('disc_or_season_number'),
    },
    {
      id: 'track_or_episode',
      draftKey: 'trackOrEpisode',
      label: '集号',
      placeholder: '1',
      half: true,
      sourceName: sourceBadgeFor('track_or_episode_number'),
    },
    {
      id: 'year',
      draftKey: 'dateOrYear',
      label: '首播 / 年份',
      placeholder: 'YYYY-MM-DD',
      half: true,
      sourceName: sourceBadgeFor('year', 'published_date'),
    },
    {
      id: 'rating',
      draftKey: 'rating',
      label: '评分',
      placeholder: '8.5',
      half: true,
      sourceName: sourceBadgeFor('rating'),
    },
    {
      id: 'publisher_or_studio',
      draftKey: 'publisherOrStudio',
      label: '制片 / 发行商',
      placeholder: '制片厂',
      sourceName: sourceBadgeFor('publisher_or_studio'),
    },
    {
      id: 'directors',
      draftKey: 'directors',
      label: '导演',
      placeholder: '导演',
      sourceName: sourceBadgeFor('illustrators_or_directors'),
    },
    {
      id: 'cast',
      draftKey: 'cast',
      label: '主演',
      placeholder: '演员（`/` 分隔）',
      sourceName: sourceBadgeFor('cast'),
    },
    {
      id: 'genres',
      draftKey: 'genres',
      label: '流派',
      placeholder: '流派',
      sourceName: sourceBadgeFor('genres'),
    },
    {
      id: 'tags',
      draftKey: 'tags',
      label: '标签',
      placeholder: '标签',
      sourceName: sourceBadgeFor('tags'),
    },
    {
      id: 'summary',
      draftKey: 'summary',
      label: '剧情简介',
      placeholder: '剧情简介…',
      multiline: true,
      rows: 5,
      sourceName: sourceBadgeFor('summary'),
    },
  ]
})

function emitDirectApply(advanceNext = false): void {
  const edited = buildEditedMetadata()
  if (!edited) return
  const opts: CandidateApplyOptions = {
    organizeAfterSave: organizeAfterSave.value,
    writeAlbumCover: writeAlbumCover.value,
  }
  if (advanceNext) {
    emit('directApplyNext', edited, props.preview.fillMode, opts)
  } else {
    emit('directApply', edited, props.preview.fillMode, opts)
  }
}

defineExpose({
  triggerSave: emitDirectApply,
  isEdited: isDraftEdited,
})
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-[var(--md-sys-color-surface-container-low)]">
    <!-- Preview / Editable Draft Header -->
    <div
      class="flex shrink-0 items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)]/45 px-3.5 py-2.5"
    >
      <div class="flex min-w-0 items-center gap-1.5">
        <span
          class="truncate rounded-full px-2 py-0.5 text-[10px] font-semibold"
          :class="
            preview.isMerged
              ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
              : 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
          "
        >
          {{ preview.sourceLabel }}
        </span>
        <span class="truncate text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]">
          待写入结果（可编辑）
        </span>
        <span
          v-if="isDraftEdited"
          class="shrink-0 rounded-full bg-[var(--md-sys-color-warning-container)] px-1.5 py-0.5 text-[9px] font-semibold text-[var(--md-sys-color-on-warning-container)]"
        >
          已微调
        </span>
      </div>

      <div class="flex shrink-0 items-center gap-0.5">
        <button
          v-if="isDraftEdited"
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
          title="恢复为此刮削源的原始信息"
          @click="resetDraft"
        >
          <LuRotateCcw class="h-3.5 w-3.5" />
        </button>

        <button
          v-if="closable"
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
          title="关闭右侧条目预览"
          @click="emit('close')"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>

    <!-- Loading State -->
    <div
      v-if="preview.loading && !preview.meta"
      class="flex flex-1 flex-col items-center justify-center gap-2 px-4 text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuLoader2 class="h-5 w-5 animate-spin text-[var(--md-sys-color-primary)]" />
      <span>正在拉取该候选源完整条目信息…</span>
    </div>

    <!-- Editable Mirror Form Body (Matches Left Local Metadata Form) -->
    <template v-else-if="preview.meta">
      <div class="flex-1 space-y-3.5 overflow-y-auto px-3.5 pt-3 pb-4">
        <!-- Cover Card -->
        <div class="flex gap-3 rounded-2xl bg-[var(--md-sys-color-surface)] p-3">
          <div class="flex shrink-0 flex-col items-center gap-1.5">
            <div
              class="relative overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-high)]"
              :class="[
                mediaKind === 'music' ? 'h-24 w-24 album-shadow' : 'h-32 w-24 book-shadow',
                !draft.useCover ? 'opacity-45' : '',
              ]"
            >
              <img
                v-if="draft.useCover && isCoverUsable(preview.meta.cover_url)"
                :src="resolveScraperCoverUrl(preview.meta.cover_url)"
                alt="scraped cover"
                referrerpolicy="no-referrer"
                class="h-full w-full object-cover"
                @error="markCoverBroken(preview.meta.cover_url)"
              />
              <div
                v-else
                class="flex h-full w-full flex-col items-center justify-center gap-1 text-[var(--md-sys-color-on-surface-variant)]"
              >
                <LuBookOpen v-if="mediaKind === 'book'" class="h-6 w-6 opacity-50" />
                <LuLayers v-else-if="mediaKind === 'comic'" class="h-6 w-6 opacity-50" />
                <LuDisc3 v-else-if="mediaKind === 'music'" class="h-6 w-6 opacity-50" />
                <LuClapperboard v-else class="h-6 w-6 opacity-50" />
                <span class="text-[10px]">
                  {{
                    !draft.useCover
                      ? '已取消封面'
                      : preview.meta.cover_url
                        ? '封面加载失败'
                        : '无封面'
                  }}
                </span>
              </div>
            </div>
            <span
              v-if="draft.useCover && isCoverUsable(preview.meta.cover_url) && sourceBadgeFor('cover_url')"
              class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]"
            >
              封面 · {{ sourceBadgeFor('cover_url') }}
            </span>
          </div>

          <div class="flex min-w-0 flex-1 flex-col justify-between gap-2">
            <div>
              <div class="line-clamp-2 text-[13px] font-semibold text-[var(--md-sys-color-on-surface)]">
                {{ draft.title || preview.meta.title || '—' }}
              </div>
              <div
                v-if="draft.creators"
                class="mt-1 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ draft.creators }}
              </div>
              <div
                v-if="preview.loading"
                class="mt-1.5 inline-flex items-center gap-1 text-[10px] text-[var(--md-sys-color-primary)]"
              >
                <LuLoader2 class="h-3 w-3 animate-spin" />
                <span>补全详情中…</span>
              </div>
            </div>

            <div class="flex flex-wrap gap-1.5">
              <button
                v-if="isCoverUsable(preview.meta.cover_url)"
                type="button"
                class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
                :class="!draft.useCover ? 'md-chip-selected' : ''"
                @click="draft.useCover = !draft.useCover"
              >
                <LuImageOff v-if="draft.useCover" class="h-3 w-3" />
                <LuImage v-else class="h-3 w-3" />
                <span>{{ draft.useCover ? '不用此封面' : '恢复封面' }}</span>
              </button>
            </div>
          </div>
        </div>

        <!-- Editable Structured Fields Grid (1-to-1 Mirror of Left Form) -->
        <div class="grid grid-cols-2 gap-2.5">
          <div
            v-for="spec in fieldSpecs"
            :key="spec.id"
            :class="spec.half ? 'col-span-1' : 'col-span-2'"
            class="space-y-1"
          >
            <div class="flex items-center justify-between gap-1.5">
              <div class="flex min-w-0 items-center gap-1.5">
                <span class="truncate text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
                  {{ spec.label }}
                </span>
                <span
                  v-if="spec.sourceName && draft[spec.draftKey].trim()"
                  class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 text-[9px] font-semibold text-[var(--md-sys-color-primary)]"
                >
                  {{ spec.sourceName }}
                </span>
              </div>
            </div>

            <textarea
              v-if="spec.multiline"
              v-model="draft[spec.draftKey]"
              :rows="spec.rows ?? 5"
              :placeholder="spec.placeholder"
              class="w-full rounded-xl border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] px-2.5 py-2 text-xs leading-relaxed text-[var(--md-sys-color-on-surface)] outline-none transition focus:border-[var(--md-sys-color-primary)]"
            />
            <input
              v-else
              v-model="draft[spec.draftKey]"
              type="text"
              :placeholder="spec.placeholder"
              class="h-9 w-full rounded-xl border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] px-2.5 text-xs text-[var(--md-sys-color-on-surface)] outline-none transition focus:border-[var(--md-sys-color-primary)]"
            />
          </div>
        </div>
      </div>

      <!-- Bottom Mirror Action Bar -->
      <div
        class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container)]/75 px-3.5 py-2.5"
      >
        <div class="flex flex-wrap items-center gap-3 text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
          <span>
            写入模式：{{ preview.fillMode === 'overwrite' ? '覆盖' : '仅补空' }}
          </span>
          <span title="写入元数据后按设置页的默认命名模板重命名并归档目录">
            <AppCheckbox v-model="organizeAfterSave" label="保存并归档" />
          </span>
          <span
            v-if="
              mediaKind === 'music' &&
              draft.useCover &&
              isCoverUsable(preview.meta.cover_url)
            "
            title="写入内嵌封面时同步在专辑目录保存 cover 图片"
          >
            <AppCheckbox v-model="writeAlbumCover" label="同步专辑 cover" />
          </span>
        </div>

        <div class="flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            :disabled="saving"
            class="md-btn md-btn-sm md-state"
            :class="hasNext ? 'md-btn-tonal' : 'md-btn-filled'"
            title="将右侧结果写入当前原文件 (⌘S)"
            @click="emitDirectApply(false)"
          >
            <LuLoader2 v-if="saving" class="h-3.5 w-3.5 animate-spin" />
            <LuSave v-else class="h-3.5 w-3.5" />
            <span>保存</span>
          </button>
          <button
            v-if="hasNext"
            type="button"
            :disabled="saving"
            class="md-btn md-btn-sm md-btn-filled md-state"
            title="写入当前原文件并切换下一项 (⌘↵)"
            @click="emitDirectApply(true)"
          >
            <LuSkipForward class="h-3.5 w-3.5" />
            <span>保存并下一项</span>
          </button>
        </div>
      </div>
    </template>
  </div>
</template>
