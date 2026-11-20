<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuArrowLeft,
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuImage,
  LuLayers,
  LuLoader2,
  LuX,
  LuZap,
} from 'vue-icons-plus/lu'
import type { MediaKind } from '@/types/api'
import { resolveScraperCoverUrl } from '../../api'
import type { FieldSourceBadge, ScrapedMetadata } from '../../types'
import type { ApplyFillMode, ScrapedFieldKey } from '../inspector/useMetadataForm'

export interface CandidatePreviewState {
  key: string
  isMerged: boolean
  sourceLabel: string
  meta: ScrapedMetadata | null
  loading: boolean
  fieldSources: Record<string, FieldSourceBadge> | null
  fillMode: ApplyFillMode
}

const props = defineProps<{
  mediaKind: MediaKind
  preview: CandidatePreviewState
}>()

const emit = defineEmits<{
  fillAll: [meta: ScrapedMetadata, mode: ApplyFillMode]
  fillField: [field: ScrapedFieldKey, meta: ScrapedMetadata, label: string]
  directApply: [meta: ScrapedMetadata, mode: ApplyFillMode]
  close: []
}>()

const brokenCoverUrls = ref<Set<string>>(new Set())

watch(
  () => props.preview.meta?.cover_url,
  () => {
    // keep previously known broken URLs, no-op needed
  },
)

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

interface PreviewFieldRow {
  id: string
  fillKey: ScrapedFieldKey
  label: string
  value: string
  multiline?: boolean
  half?: boolean
  sourceName?: string | null
}

const fieldRows = computed<PreviewFieldRow[]>(() => {
  const m = props.preview.meta
  if (!m) return []

  const combinedTags = Array.from(new Set([...(m.genres ?? []), ...(m.tags ?? [])].filter(Boolean)))
  const dateOrYear = m.published_date || (m.year ? String(m.year) : '')

  if (props.mediaKind === 'book') {
    return [
      {
        id: 'title',
        fillKey: 'title',
        label: '书名 (dc:title)',
        value: m.title || '',
        sourceName: sourceBadgeFor('title'),
      },
      {
        id: 'creators',
        fillKey: 'creators',
        label: '作者 (dc:creator)',
        value: m.creators.join(' / '),
        sourceName: sourceBadgeFor('creators'),
      },
      {
        id: 'series_or_album',
        fillKey: 'series_or_album',
        label: '系列 / 丛书 (Series)',
        value: m.series_or_album || '',
        sourceName: sourceBadgeFor('series_or_album'),
      },
      {
        id: 'isbn',
        fillKey: 'isbn',
        label: 'ISBN',
        value: m.isbn || '',
        half: true,
        sourceName: sourceBadgeFor('isbn'),
      },
      {
        id: 'publisher_or_studio',
        fillKey: 'publisher_or_studio',
        label: '出版社 (dc:publisher)',
        value: m.publisher_or_studio || '',
        half: true,
        sourceName: sourceBadgeFor('publisher_or_studio'),
      },
      {
        id: 'year',
        fillKey: 'year',
        label: '出版日期 (dc:date)',
        value: dateOrYear,
        half: true,
        sourceName: sourceBadgeFor('published_date', 'year'),
      },
      {
        id: 'language',
        fillKey: 'language',
        label: '语言 (dc:language)',
        value: m.language || '',
        half: true,
        sourceName: sourceBadgeFor('language'),
      },
      {
        id: 'tags',
        fillKey: 'tags',
        label: '标签 / 主题 (dc:subject)',
        value: combinedTags.join(' / '),
        sourceName: sourceBadgeFor('tags', 'genres'),
      },
      {
        id: 'summary',
        fillKey: 'summary',
        label: '内容简介 (dc:description)',
        value: m.summary || '',
        multiline: true,
        sourceName: sourceBadgeFor('summary'),
      },
    ]
  }

  if (props.mediaKind === 'comic') {
    return [
      {
        id: 'title',
        fillKey: 'title',
        label: '标题',
        value: m.title || '',
        sourceName: sourceBadgeFor('title'),
      },
      {
        id: 'series_or_album',
        fillKey: 'series_or_album',
        label: '系列 / 丛书',
        value: m.series_or_album || m.title || '',
        sourceName: sourceBadgeFor('series_or_album', 'title'),
      },
      {
        id: 'creators',
        fillKey: 'creators',
        label: '作者 / 编剧',
        value: m.creators.join(' / '),
        half: true,
        sourceName: sourceBadgeFor('creators'),
      },
      {
        id: 'directors',
        fillKey: 'directors',
        label: '画师 / 作画',
        value: m.illustrators_or_directors.join(' / '),
        half: true,
        sourceName: sourceBadgeFor('illustrators_or_directors'),
      },
      {
        id: 'publisher_or_studio',
        fillKey: 'publisher_or_studio',
        label: '出版社',
        value: m.publisher_or_studio || '',
        half: true,
        sourceName: sourceBadgeFor('publisher_or_studio'),
      },
      {
        id: 'year',
        fillKey: 'year',
        label: '年份 / 日期',
        value: dateOrYear,
        half: true,
        sourceName: sourceBadgeFor('published_date', 'year'),
      },
      {
        id: 'track_or_episode',
        fillKey: 'track_or_episode',
        label: '卷 / 话号',
        value: m.track_or_episode_number ? String(m.track_or_episode_number) : '',
        half: true,
        sourceName: sourceBadgeFor('track_or_episode_number'),
      },
      {
        id: 'language',
        fillKey: 'language',
        label: '语言',
        value: m.language || '',
        half: true,
        sourceName: sourceBadgeFor('language'),
      },
      {
        id: 'genres',
        fillKey: 'genres',
        label: '流派 / 分类',
        value: m.genres.join(' / '),
        sourceName: sourceBadgeFor('genres'),
      },
      {
        id: 'tags',
        fillKey: 'tags',
        label: '标签',
        value: m.tags.join(' / '),
        sourceName: sourceBadgeFor('tags'),
      },
      {
        id: 'summary',
        fillKey: 'summary',
        label: '内容简介',
        value: m.summary || '',
        multiline: true,
        sourceName: sourceBadgeFor('summary'),
      },
    ]
  }

  if (props.mediaKind === 'music') {
    return [
      {
        id: 'title',
        fillKey: 'title',
        label: '曲目标题',
        value: m.title || '',
        sourceName: sourceBadgeFor('title'),
      },
      {
        id: 'creators',
        fillKey: 'creators',
        label: '艺人 / 歌手',
        value: m.creators.join(' / '),
        sourceName: sourceBadgeFor('creators'),
      },
      {
        id: 'series_or_album',
        fillKey: 'series_or_album',
        label: '所属专辑',
        value: m.series_or_album || '',
        sourceName: sourceBadgeFor('series_or_album'),
      },
      {
        id: 'disc_or_season',
        fillKey: 'disc_or_season',
        label: '碟号',
        value: m.disc_or_season_number ? String(m.disc_or_season_number) : '',
        half: true,
        sourceName: sourceBadgeFor('disc_or_season_number'),
      },
      {
        id: 'track_or_episode',
        fillKey: 'track_or_episode',
        label: '音轨号',
        value: m.track_or_episode_number ? String(m.track_or_episode_number) : '',
        half: true,
        sourceName: sourceBadgeFor('track_or_episode_number'),
      },
      {
        id: 'year',
        fillKey: 'year',
        label: '发行年份',
        value: dateOrYear,
        half: true,
        sourceName: sourceBadgeFor('year', 'published_date'),
      },
      {
        id: 'genres',
        fillKey: 'genres',
        label: '流派',
        value: m.genres.join(' / '),
        half: true,
        sourceName: sourceBadgeFor('genres'),
      },
      {
        id: 'lyrics',
        fillKey: 'lyrics',
        label: m.synced_lyrics ? '同步歌词 (LRC)' : '纯文本歌词',
        value: m.synced_lyrics || m.plain_lyrics || '',
        multiline: true,
        sourceName: sourceBadgeFor('synced_lyrics', 'plain_lyrics'),
      },
    ]
  }

  // Video
  return [
    {
      id: 'title',
      fillKey: 'title',
      label: '标题',
      value: m.title || '',
      sourceName: sourceBadgeFor('title'),
    },
    {
      id: 'original_title',
      fillKey: 'original_title',
      label: '原作名',
      value: m.original_title || '',
      sourceName: sourceBadgeFor('original_title'),
    },
    {
      id: 'series_or_album',
      fillKey: 'series_or_album',
      label: '所属剧集 / 系列',
      value: m.series_or_album || '',
      sourceName: sourceBadgeFor('series_or_album'),
    },
    {
      id: 'disc_or_season',
      fillKey: 'disc_or_season',
      label: '季号',
      value: m.disc_or_season_number ? String(m.disc_or_season_number) : '',
      half: true,
      sourceName: sourceBadgeFor('disc_or_season_number'),
    },
    {
      id: 'track_or_episode',
      fillKey: 'track_or_episode',
      label: '集号',
      value: m.track_or_episode_number ? String(m.track_or_episode_number) : '',
      half: true,
      sourceName: sourceBadgeFor('track_or_episode_number'),
    },
    {
      id: 'year',
      fillKey: 'year',
      label: '首播 / 年份',
      value: dateOrYear,
      half: true,
      sourceName: sourceBadgeFor('year', 'published_date'),
    },
    {
      id: 'rating',
      fillKey: 'rating',
      label: '评分',
      value: m.rating ? String(m.rating) : '',
      half: true,
      sourceName: sourceBadgeFor('rating'),
    },
    {
      id: 'publisher_or_studio',
      fillKey: 'publisher_or_studio',
      label: '制片 / 发行商',
      value: m.publisher_or_studio || '',
      sourceName: sourceBadgeFor('publisher_or_studio'),
    },
    {
      id: 'directors',
      fillKey: 'directors',
      label: '导演',
      value: m.illustrators_or_directors.join(' / '),
      sourceName: sourceBadgeFor('illustrators_or_directors'),
    },
    {
      id: 'cast',
      fillKey: 'cast',
      label: '主演',
      value: m.cast.slice(0, 10).join(' / '),
      sourceName: sourceBadgeFor('cast'),
    },
    {
      id: 'genres',
      fillKey: 'genres',
      label: '流派',
      value: m.genres.join(' / '),
      sourceName: sourceBadgeFor('genres'),
    },
    {
      id: 'tags',
      fillKey: 'tags',
      label: '标签',
      value: m.tags.slice(0, 15).join(' / '),
      sourceName: sourceBadgeFor('tags'),
    },
    {
      id: 'summary',
      fillKey: 'summary',
      label: '剧情简介',
      value: m.summary || '',
      multiline: true,
      sourceName: sourceBadgeFor('summary'),
    },
  ]
})
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-[var(--md-sys-color-surface-container-low)]">
    <!-- Preview Pane Header -->
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
          条目对照预览
        </span>
      </div>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
        title="关闭右侧条目预览"
        @click="emit('close')"
      >
        <LuX class="h-3.5 w-3.5" />
      </button>
    </div>

    <!-- Loading State -->
    <div
      v-if="preview.loading && !preview.meta"
      class="flex flex-1 flex-col items-center justify-center gap-2 px-4 text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuLoader2 class="h-5 w-5 animate-spin text-[var(--md-sys-color-primary)]" />
      <span>正在拉取该候选源完整条目信息…</span>
    </div>

    <!-- Structured Mirror Preview Body (Matches Left Inspector Form) -->
    <template v-else-if="preview.meta">
      <div class="flex-1 space-y-3.5 overflow-y-auto px-3.5 pt-3 pb-4">
        <!-- Cover Card (Same Proportions as Left Inspector Form) -->
        <div class="flex gap-3 rounded-2xl bg-[var(--md-sys-color-surface)] p-3">
          <div class="flex shrink-0 flex-col items-center gap-1.5">
            <div
              class="relative overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-high)]"
              :class="mediaKind === 'music' ? 'h-24 w-24 album-shadow' : 'h-32 w-24 book-shadow'"
            >
              <img
                v-if="isCoverUsable(preview.meta.cover_url)"
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
                  {{ preview.meta.cover_url ? '封面加载失败' : '无封面' }}
                </span>
              </div>
            </div>
            <span
              v-if="isCoverUsable(preview.meta.cover_url) && sourceBadgeFor('cover_url')"
              class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]"
            >
              封面 · {{ sourceBadgeFor('cover_url') }}
            </span>
          </div>

          <div class="flex min-w-0 flex-1 flex-col justify-between gap-2">
            <div>
              <div class="line-clamp-2 text-[13px] font-semibold text-[var(--md-sys-color-on-surface)]">
                {{ preview.meta.title || '—' }}
              </div>
              <div
                v-if="preview.meta.creators.length > 0"
                class="mt-1 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ preview.meta.creators.join(' / ') }}
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
                @click="emit('fillField', 'cover', preview.meta!, '封面')"
              >
                <LuImage class="h-3 w-3" />
                <span>← 仅用此封面</span>
              </button>
            </div>
          </div>
        </div>

        <!-- Structured Fields Grid (1-to-1 Mirror of Left Form) -->
        <div class="grid grid-cols-2 gap-2.5">
          <div
            v-for="row in fieldRows"
            :key="row.id"
            :class="row.half ? 'col-span-1' : 'col-span-2'"
            class="space-y-1"
          >
            <div class="flex items-center justify-between gap-1.5">
              <div class="flex min-w-0 items-center gap-1.5">
                <span class="truncate text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
                  {{ row.label }}
                </span>
                <span
                  v-if="row.sourceName && row.value"
                  class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 text-[9px] font-semibold text-[var(--md-sys-color-primary)]"
                >
                  {{ row.sourceName }}
                </span>
              </div>

              <button
                v-if="row.value"
                type="button"
                class="inline-flex shrink-0 items-center gap-0.5 rounded-full bg-[var(--md-sys-color-primary-container)]/75 px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-primary-container)] transition hover:bg-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-on-primary)]"
                :title="`将「${row.label}」填入左侧表单`"
                @click="emit('fillField', row.fillKey, preview.meta!, row.label)"
              >
                <LuArrowLeft class="h-2.5 w-2.5" />
                <span>填入</span>
              </button>
            </div>

            <div
              class="rounded-xl border border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface)] px-2.5 py-2 text-xs leading-relaxed"
              :class="[
                row.value
                  ? 'text-[var(--md-sys-color-on-surface)]'
                  : 'text-[var(--md-sys-color-on-surface-variant)]/45',
                row.multiline ? 'max-h-44 overflow-y-auto whitespace-pre-line' : 'break-words',
              ]"
            >
              {{ row.value || '—' }}
            </div>
          </div>
        </div>
      </div>

      <!-- Bottom Mirror Action Bar -->
      <div
        class="flex shrink-0 items-center justify-between gap-2 border-t border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container)]/75 px-3.5 py-2.5"
      >
        <span class="text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
          模式：{{ preview.fillMode === 'overwrite' ? '覆盖' : '仅补空' }}
        </span>
        <div class="flex items-center gap-1.5">
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            title="将此结果整条回填到左侧表单（暂不落盘）"
            @click="emit('fillAll', preview.meta!, preview.fillMode)"
          >
            <LuArrowLeft class="h-3.5 w-3.5" />
            <span>整条填入</span>
          </button>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-filled md-state"
            title="直接写入文件并保存（含封面）"
            @click="emit('directApply', preview.meta!, preview.fillMode)"
          >
            <LuZap class="h-3.5 w-3.5" />
            <span>直写</span>
          </button>
        </div>
      </div>
    </template>
  </div>
</template>
