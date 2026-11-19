<script setup lang="ts">
import { LuImage, LuLayers } from 'vue-icons-plus/lu'
import { resolveScraperCoverUrl } from '../../api'
import type { FieldSourceBadge, ScrapedMetadata } from '../../types'

export interface ScrapeEffectiveView {
  metadata: ScrapedMetadata
  sources: Record<string, FieldSourceBadge>
}

const props = defineProps<{
  effectiveView: ScrapeEffectiveView | null
  activeSourceTab: string
  previewMode: 'merged' | 'single'
  activeSingleProviderName?: string
  searching: boolean
  errorMsg: string | null
  disabledFields: Set<string>
  brokenCoverUrls: Set<string>
  isVideo: boolean
}>()

const emit = defineEmits<{
  'update:previewMode': [mode: 'merged' | 'single']
  toggleField: [key: string]
  markCoverBroken: [url: string | null | undefined]
}>()

function isCoverUsable(url: string | null | undefined): url is string {
  return Boolean(url && !props.brokenCoverUrls.has(url))
}

function isFieldEnabled(key: string): boolean {
  return !props.disabledFields.has(key)
}

function badgeName(key: string): string | null {
  return props.effectiveView?.sources[key]?.provider_name ?? null
}
</script>

<template>
  <div class="overflow-y-auto p-4 md:col-span-7">
  <div
    v-if="errorMsg"
    class="mb-3 rounded-xl border border-red-500/35 bg-red-500/10 px-3 py-2 text-xs text-red-400"
  >
    {{ errorMsg }}
  </div>

  <!-- Preview Mode Header -->
  <div
    v-if="effectiveView"
    class="mb-3 flex flex-wrap items-center justify-between gap-2 rounded-xl bg-[var(--ctp-mantle)]/55 px-3 py-2"
  >
    <div class="flex items-center gap-1.5 text-xs font-semibold text-[var(--ctp-text)]">
      <LuLayers class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
      <span>
        {{
          activeSourceTab !== 'merged' && previewMode === 'single'
            ? `单源原始信息（${activeSingleProviderName}）`
            : '多源优先级互补合成结果（每项右侧标注实际来源）'
        }}
      </span>
    </div>

    <div
      v-if="activeSourceTab !== 'merged'"
      class="flex items-center gap-1 rounded-lg bg-[var(--ctp-canvas)] p-0.5"
    >
      <button
        type="button"
        class="rounded-md px-2 py-0.5 text-[11px] font-semibold transition"
        :class="
          previewMode === 'merged'
            ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'text-[var(--ctp-subtext1)]'
        "
        @click="emit('update:previewMode', 'merged')"
      >
        看聚合后
      </button>
      <button
        type="button"
        class="rounded-md px-2 py-0.5 text-[11px] font-semibold transition"
        :class="
          previewMode === 'single'
            ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'text-[var(--ctp-subtext1)]'
        "
        @click="emit('update:previewMode', 'single')"
      >
        仅看此源
      </button>
    </div>
  </div>

  <div v-if="searching" class="py-16 text-center text-xs text-[var(--ctp-subtext0)]">
    正在合成多源元信息…
  </div>
  <div v-else-if="effectiveView" class="space-y-3.5">
    <!-- Top Block: Cover (with Source Badge) + Core Annotated Fields -->
    <div class="flex flex-col gap-4 sm:flex-row">
      <div class="flex shrink-0 flex-col items-center gap-1.5">
        <div
          class="flex h-38 w-28 items-center justify-center overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] shadow-md"
        >
          <img
            v-if="isCoverUsable(effectiveView.metadata.cover_url)"
            :src="resolveScraperCoverUrl(effectiveView.metadata.cover_url)"
            alt=""
            class="h-full w-full object-cover"
            @error="emit('markCoverBroken', effectiveView.metadata.cover_url)"
          />
          <div
            v-else
            class="flex flex-col items-center gap-1 text-[var(--ctp-subtext0)]"
          >
            <LuImage class="h-6 w-6 opacity-50" />
            <span class="text-[10px]">暂无封面</span>
          </div>
        </div>
        <span
          v-if="isCoverUsable(effectiveView.metadata.cover_url) && badgeName('cover_url')"
          class="rounded-full bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
        >
          封面 · {{ badgeName('cover_url') }}
        </span>
        <span
          v-if="isCoverUsable(effectiveView.metadata.backdrop_url) && badgeName('backdrop_url')"
          class="rounded-full bg-[var(--ctp-mantle)] px-2 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext1)]"
        >
          背景图 · {{ badgeName('backdrop_url') }}
        </span>
      </div>

      <!-- Field-by-Field Provenance List with Inclusion Toggles -->
      <div class="min-w-0 flex-1 space-y-2 rounded-2xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/35 p-3">
        <!-- Title -->
        <div
          v-if="effectiveView.metadata.title"
          class="flex items-start justify-between gap-2 transition"
          :class="!isFieldEnabled('title') ? 'opacity-45' : ''"
        >
          <label class="flex min-w-0 cursor-pointer items-start gap-2">
            <input
              type="checkbox"
              :checked="isFieldEnabled('title')"
              class="mt-1 h-3.5 w-3.5 accent-[var(--ctp-accent)]"
              @change="emit('toggleField', 'title')"
            />
            <div class="min-w-0">
              <span class="text-[10px] text-[var(--ctp-subtext0)]">标题</span>
              <div class="text-sm font-bold text-[var(--ctp-text)]">
                {{ effectiveView.metadata.title }}
              </div>
            </div>
          </label>
          <span
            v-if="badgeName('title')"
            class="shrink-0 rounded-md bg-[var(--ctp-accent-soft)] px-1.5 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
          >
            {{ badgeName('title') }}
          </span>
        </div>

        <!-- Original Title -->
        <div
          v-if="
            effectiveView.metadata.original_title &&
            effectiveView.metadata.original_title !== effectiveView.metadata.title
          "
          class="flex items-center justify-between gap-2 text-xs transition"
          :class="!isFieldEnabled('original_title') ? 'opacity-45' : ''"
        >
          <label class="flex min-w-0 cursor-pointer items-center gap-2">
            <input
              type="checkbox"
              :checked="isFieldEnabled('original_title')"
              class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
              @change="emit('toggleField', 'original_title')"
            />
            <span class="truncate text-[var(--ctp-subtext1)]">
              <b class="font-medium text-[var(--ctp-subtext0)]">原名：</b>
              {{ effectiveView.metadata.original_title }}
            </span>
          </label>
          <span
            v-if="badgeName('original_title')"
            class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
          >
            {{ badgeName('original_title') }}
          </span>
        </div>

        <!-- Series / Album -->
        <div
          v-if="effectiveView.metadata.series_or_album"
          class="flex items-center justify-between gap-2 text-xs transition"
          :class="!isFieldEnabled('series_or_album') ? 'opacity-45' : ''"
        >
          <label class="flex min-w-0 cursor-pointer items-center gap-2">
            <input
              type="checkbox"
              :checked="isFieldEnabled('series_or_album')"
              class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
              @change="emit('toggleField', 'series_or_album')"
            />
            <span class="truncate text-[var(--ctp-subtext1)]">
              <b class="font-medium text-[var(--ctp-subtext0)]">系列 / 专辑：</b>
              {{ effectiveView.metadata.series_or_album }}
            </span>
          </label>
          <span
            v-if="badgeName('series_or_album')"
            class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
          >
            {{ badgeName('series_or_album') }}
          </span>
        </div>

        <!-- Track / Episode Number -->
        <div
          v-if="
            effectiveView.metadata.track_or_episode_number ||
            effectiveView.metadata.disc_or_season_number !== null
          "
          class="flex items-center justify-between gap-2 text-xs transition"
          :class="!isFieldEnabled('numbers') ? 'opacity-45' : ''"
        >
          <label class="flex min-w-0 cursor-pointer items-center gap-2">
            <input
              type="checkbox"
              :checked="isFieldEnabled('numbers')"
              class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
              @change="emit('toggleField', 'numbers')"
            />
            <span class="text-[var(--ctp-subtext1)]">
              <b class="font-medium text-[var(--ctp-subtext0)]">季碟 / 序号：</b>
              #{{ effectiveView.metadata.disc_or_season_number ?? 1 }} -
              {{ effectiveView.metadata.track_or_episode_number ?? 1 }}
            </span>
          </label>
          <span
            v-if="
              badgeName('track_or_episode_number') ||
              badgeName('disc_or_season_number')
            "
            class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
          >
            {{
              badgeName('track_or_episode_number') ||
              badgeName('disc_or_season_number')
            }}
          </span>
        </div>

        <!-- Creators -->
        <div
          v-if="effectiveView.metadata.creators.length > 0"
          class="flex items-center justify-between gap-2 text-xs transition"
          :class="!isFieldEnabled('creators') ? 'opacity-45' : ''"
        >
          <label class="flex min-w-0 cursor-pointer items-center gap-2">
            <input
              type="checkbox"
              :checked="isFieldEnabled('creators')"
              class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
              @change="emit('toggleField', 'creators')"
            />
            <span class="truncate text-[var(--ctp-subtext1)]">
              <b class="font-medium text-[var(--ctp-subtext0)]">创作者：</b>
              {{ effectiveView.metadata.creators.join(' / ') }}
            </span>
          </label>
          <span
            v-if="badgeName('creators')"
            class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
          >
            {{ badgeName('creators') }}
          </span>
        </div>

        <!-- Directors / Pencillers -->
        <div
          v-if="effectiveView.metadata.illustrators_or_directors.length > 0"
          class="flex items-center justify-between gap-2 text-xs transition"
          :class="!isFieldEnabled('illustrators_or_directors') ? 'opacity-45' : ''"
        >
          <label class="flex min-w-0 cursor-pointer items-center gap-2">
            <input
              type="checkbox"
              :checked="isFieldEnabled('illustrators_or_directors')"
              class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
              @change="emit('toggleField', 'illustrators_or_directors')"
            />
            <span class="truncate text-[var(--ctp-subtext1)]">
              <b class="font-medium text-[var(--ctp-subtext0)]">导演 / 作画：</b>
              {{ effectiveView.metadata.illustrators_or_directors.join(' / ') }}
            </span>
          </label>
          <span
            v-if="badgeName('illustrators_or_directors')"
            class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
          >
            {{ badgeName('illustrators_or_directors') }}
          </span>
        </div>

        <!-- Cast -->
        <div
          v-if="effectiveView.metadata.cast.length > 0"
          class="flex items-center justify-between gap-2 text-xs transition"
          :class="!isFieldEnabled('cast') ? 'opacity-45' : ''"
        >
          <label class="flex min-w-0 cursor-pointer items-center gap-2">
            <input
              type="checkbox"
              :checked="isFieldEnabled('cast')"
              class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
              @change="emit('toggleField', 'cast')"
            />
            <span class="truncate text-[var(--ctp-subtext1)]">
              <b class="font-medium text-[var(--ctp-subtext0)]">主演 / 声优：</b>
              {{ effectiveView.metadata.cast.slice(0, 6).join(' / ') }}
            </span>
          </label>
          <span
            v-if="badgeName('cast')"
            class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
          >
            {{ badgeName('cast') }}
          </span>
        </div>

        <!-- Publisher / Studio -->
        <div
          v-if="effectiveView.metadata.publisher_or_studio"
          class="flex items-center justify-between gap-2 text-xs transition"
          :class="!isFieldEnabled('publisher_or_studio') ? 'opacity-45' : ''"
        >
          <label class="flex min-w-0 cursor-pointer items-center gap-2">
            <input
              type="checkbox"
              :checked="isFieldEnabled('publisher_or_studio')"
              class="h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
              @change="emit('toggleField', 'publisher_or_studio')"
            />
            <span class="truncate text-[var(--ctp-subtext1)]">
              <b class="font-medium text-[var(--ctp-subtext0)]">出品 / 出版社：</b>
              {{ effectiveView.metadata.publisher_or_studio }}
            </span>
          </label>
          <span
            v-if="badgeName('publisher_or_studio')"
            class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
          >
            {{ badgeName('publisher_or_studio') }}
          </span>
        </div>

        <!-- Year / Rating / ISBN -->
        <div class="flex flex-wrap items-center gap-2 pt-0.5 text-xs">
          <label
            v-if="effectiveView.metadata.year"
            class="inline-flex cursor-pointer items-center gap-1 rounded-lg bg-[var(--ctp-canvas)] px-2 py-0.5 text-[11px] transition"
            :class="!isFieldEnabled('year') ? 'opacity-45' : ''"
          >
            <input
              type="checkbox"
              :checked="isFieldEnabled('year')"
              class="h-3 w-3 accent-[var(--ctp-accent)]"
              @change="emit('toggleField', 'year')"
            />
            <span>年份 {{ effectiveView.metadata.year }}</span>
            <span v-if="badgeName('year')" class="text-[10px] text-[var(--ctp-accent)]">
              · {{ badgeName('year') }}
            </span>
          </label>
          <label
            v-if="effectiveView.metadata.rating"
            class="inline-flex cursor-pointer items-center gap-1 rounded-lg bg-[var(--ctp-canvas)] px-2 py-0.5 text-[11px] transition"
            :class="!isFieldEnabled('rating') ? 'opacity-45' : ''"
          >
            <input
              type="checkbox"
              :checked="isFieldEnabled('rating')"
              class="h-3 w-3 accent-[var(--ctp-accent)]"
              @change="emit('toggleField', 'rating')"
            />
            <span>★ {{ effectiveView.metadata.rating.toFixed(1) }}</span>
            <span v-if="badgeName('rating')" class="text-[10px] text-[var(--ctp-accent)]">
              · {{ badgeName('rating') }}
            </span>
          </label>
          <label
            v-if="effectiveView.metadata.isbn"
            class="inline-flex cursor-pointer items-center gap-1 rounded-lg bg-[var(--ctp-canvas)] px-2 py-0.5 font-mono text-[11px] transition"
            :class="!isFieldEnabled('isbn') ? 'opacity-45' : ''"
          >
            <input
              type="checkbox"
              :checked="isFieldEnabled('isbn')"
              class="h-3 w-3 accent-[var(--ctp-accent)]"
              @change="emit('toggleField', 'isbn')"
            />
            <span>ISBN {{ effectiveView.metadata.isbn }}</span>
            <span v-if="badgeName('isbn')" class="font-sans text-[10px] text-[var(--ctp-accent)]">
              · {{ badgeName('isbn') }}
            </span>
          </label>
        </div>

        <!-- Genres & Tags -->
        <div
          v-if="
            effectiveView.metadata.genres.length > 0 ||
            effectiveView.metadata.tags.length > 0
          "
          class="flex items-start justify-between gap-2 pt-1 transition"
          :class="!isFieldEnabled('tags') ? 'opacity-45' : ''"
        >
          <label class="flex cursor-pointer items-start gap-2">
            <input
              type="checkbox"
              :checked="isFieldEnabled('tags')"
              class="mt-1 h-3.5 w-3.5 shrink-0 accent-[var(--ctp-accent)]"
              @change="emit('toggleField', 'tags')"
            />
            <div class="flex flex-wrap gap-1">
              <span
                v-for="tag in [
                  ...effectiveView.metadata.genres,
                  ...effectiveView.metadata.tags,
                ].slice(0, 8)"
                :key="tag"
                class="rounded-full bg-[var(--ctp-canvas)] px-2 py-0.5 text-[10px] text-[var(--ctp-subtext1)]"
              >
                {{ tag }}
              </span>
            </div>
          </label>
          <span
            v-if="badgeName('genres') || badgeName('tags')"
            class="shrink-0 rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-accent)]"
          >
            {{ badgeName('genres') || badgeName('tags') }}
          </span>
        </div>
      </div>
    </div>

    <!-- Video Jellyfin Scraped Artwork Strip (Fanart / Season Poster / Episode Thumb) -->
    <div
      v-if="
        isVideo &&
        (isCoverUsable(effectiveView.metadata.backdrop_url) ||
          isCoverUsable(effectiveView.metadata.season_poster_url) ||
          isCoverUsable(effectiveView.metadata.episode_still_url))
      "
      class="rounded-2xl border border-[var(--ctp-surface0)]/70 bg-[var(--ctp-mantle)]/45 p-3"
    >
      <div class="mb-2 flex items-center justify-between">
        <span class="text-[11px] font-semibold text-[var(--ctp-subtext0)]">
          Jellyfin 附属刮削图片（写入 fanart / Season 海报 / 单集 thumb）
        </span>
      </div>
      <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-3">
        <div
          v-if="isCoverUsable(effectiveView.metadata.backdrop_url)"
          class="space-y-1 rounded-xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-canvas)] p-2"
        >
          <div class="aspect-video overflow-hidden rounded-lg bg-[var(--ctp-mantle)]">
            <img
              :src="effectiveView.metadata.backdrop_url"
              alt=""
              class="h-full w-full object-cover"
              @error="emit('markCoverBroken', effectiveView.metadata.backdrop_url)"
            />
          </div>
          <div class="flex items-center justify-between text-[10px]">
            <span class="font-medium text-[var(--ctp-subtext1)]">fanart.jpg</span>
            <span v-if="badgeName('backdrop_url')" class="font-semibold text-[var(--ctp-accent)]">
              {{ badgeName('backdrop_url') }}
            </span>
          </div>
        </div>

        <div
          v-if="isCoverUsable(effectiveView.metadata.season_poster_url)"
          class="space-y-1 rounded-xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-canvas)] p-2"
        >
          <div class="aspect-video overflow-hidden rounded-lg bg-[var(--ctp-mantle)]">
            <img
              :src="effectiveView.metadata.season_poster_url"
              alt=""
              class="h-full w-full object-contain"
              @error="emit('markCoverBroken', effectiveView.metadata.season_poster_url)"
            />
          </div>
          <div class="flex items-center justify-between text-[10px]">
            <span class="font-medium text-[var(--ctp-subtext1)]">Season/poster.jpg</span>
            <span
              v-if="badgeName('season_poster_url')"
              class="font-semibold text-[var(--ctp-accent)]"
            >
              {{ badgeName('season_poster_url') }}
            </span>
          </div>
        </div>

        <div
          v-if="isCoverUsable(effectiveView.metadata.episode_still_url)"
          class="space-y-1 rounded-xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-canvas)] p-2"
        >
          <div class="aspect-video overflow-hidden rounded-lg bg-[var(--ctp-mantle)]">
            <img
              :src="effectiveView.metadata.episode_still_url"
              alt=""
              class="h-full w-full object-cover"
              @error="emit('markCoverBroken', effectiveView.metadata.episode_still_url)"
            />
          </div>
          <div class="flex items-center justify-between text-[10px]">
            <span class="font-medium text-[var(--ctp-subtext1)]">单集 -thumb.jpg</span>
            <span
              v-if="badgeName('episode_still_url')"
              class="font-semibold text-[var(--ctp-accent)]"
            >
              {{ badgeName('episode_still_url') }}
            </span>
          </div>
        </div>
      </div>
    </div>

    <!-- Summary Block with Provenance Badge -->
    <div
      v-if="effectiveView.metadata.summary"
      class="rounded-2xl border border-[var(--ctp-surface0)]/70 bg-[var(--ctp-mantle)]/55 p-3 transition"
      :class="!isFieldEnabled('summary') ? 'opacity-45' : ''"
    >
      <div class="mb-1.5 flex items-center justify-between gap-2">
        <label class="flex cursor-pointer items-center gap-2">
          <input
            type="checkbox"
            :checked="isFieldEnabled('summary')"
            class="h-3.5 w-3.5 accent-[var(--ctp-accent)]"
            @change="emit('toggleField', 'summary')"
          />
          <span class="text-[11px] font-semibold text-[var(--ctp-subtext0)]">
            {{
              effectiveView.metadata.target === 'tv_show'
                ? '单集简介 (写入 <episodedetails>)'
                : '内容简介'
            }}
          </span>
        </label>
        <span
          v-if="badgeName('summary')"
          class="rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
        >
          {{ badgeName('summary') }}
        </span>
      </div>
      <p class="text-xs leading-relaxed whitespace-pre-line text-[var(--ctp-text)]">
        {{ effectiveView.metadata.summary }}
      </p>
    </div>

    <!-- Series / Season NFO Summary Preview for TV Shows -->
    <div
      v-if="
        effectiveView.metadata.target === 'tv_show' &&
        (effectiveView.metadata.show_summary || effectiveView.metadata.season_summary) &&
        (effectiveView.metadata.show_summary !== effectiveView.metadata.summary ||
          effectiveView.metadata.season_title)
      "
      class="rounded-2xl border border-[var(--ctp-surface0)]/70 bg-[var(--ctp-mantle)]/40 p-3"
    >
      <div class="mb-1.5 flex items-center justify-between gap-2">
        <span class="text-[11px] font-semibold text-[var(--ctp-subtext0)]">
          剧集 / 季级简介 (写入 tvshow.nfo &amp; season.nfo{{
            effectiveView.metadata.season_title
              ? ` · ${effectiveView.metadata.season_title}`
              : ''
          }})
        </span>
        <span
          v-if="badgeName('season_summary') || badgeName('show_summary')"
          class="rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
        >
          {{ badgeName('season_summary') || badgeName('show_summary') }}
        </span>
      </div>
      <p class="text-xs leading-relaxed whitespace-pre-line text-[var(--ctp-subtext1)]">
        {{ effectiveView.metadata.season_summary || effectiveView.metadata.show_summary }}
      </p>
    </div>

    <!-- Lyrics Block with Provenance Badge -->
    <div
      v-if="
        effectiveView.metadata.synced_lyrics || effectiveView.metadata.plain_lyrics
      "
      class="rounded-2xl border border-[var(--ctp-surface0)]/70 bg-[var(--ctp-mantle)]/55 p-3 transition"
      :class="!isFieldEnabled('lyrics') ? 'opacity-45' : ''"
    >
      <div class="mb-1.5 flex items-center justify-between gap-2">
        <label class="flex cursor-pointer items-center gap-2">
          <input
            type="checkbox"
            :checked="isFieldEnabled('lyrics')"
            class="h-3.5 w-3.5 accent-[var(--ctp-accent)]"
            @change="emit('toggleField', 'lyrics')"
          />
          <span class="text-[11px] font-semibold text-[var(--ctp-subtext0)]">
            歌词预览 ({{
              effectiveView.metadata.synced_lyrics ? 'LRC 时间轴同步歌词' : '纯文本歌词'
            }})
          </span>
        </label>
        <span
          v-if="badgeName('lyrics')"
          class="rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
        >
          {{ badgeName('lyrics') }}
        </span>
      </div>
      <pre
        class="max-h-48 overflow-y-auto font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-[var(--ctp-text)]"
      >{{ effectiveView.metadata.synced_lyrics || effectiveView.metadata.plain_lyrics }}</pre>
    </div>
  </div>

  <div
    v-else
    class="flex h-full items-center justify-center py-16 text-xs text-[var(--ctp-subtext0)]"
  >
    暂无可用刮削数据，请调整关键词或启用更多刮削源后重试
  </div>
  </div>
</template>
