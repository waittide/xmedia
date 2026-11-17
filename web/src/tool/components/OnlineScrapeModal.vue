<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuCheck,
  LuExternalLink,
  LuKey,
  LuRefreshCw,
  LuSearch,
  LuSparkles,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import { toolApi } from '../api'
import type {
  MediaFileDetail,
  ProviderStatusDto,
  ScrapeCandidate,
  ScrapeTargetKind,
  ScrapedMetadata,
} from '../types'

const props = defineProps<{
  open: boolean
  detail: MediaFileDetail | null
}>()

const emit = defineEmits<{
  close: []
  applied: [detail: MediaFileDetail]
  openSettings: []
}>()

const targetKind = ref<ScrapeTargetKind>('music_track')
const providers = ref<ProviderStatusDto[]>([])
const enabledProviderIds = ref<string[]>([])

const keyword = ref('')
const artistOrAuthor = ref('')
const albumOrSeries = ref('')
const isbn = ref('')
const season = ref('')
const episode = ref('')

const searching = ref(false)
const candidates = ref<ScrapeCandidate[]>([])
const selectedCandidate = ref<ScrapeCandidate | null>(null)
const loadingDetail = ref(false)
const previewDetail = ref<ScrapedMetadata | null>(null)

const applyCover = ref(true)
const writeSidecarLrc = ref(true)
const applying = ref(false)
const errorMsg = ref<string | null>(null)

const targetTabs = computed<Array<{ value: ScrapeTargetKind; label: string }>>(() => {
  const kind = props.detail?.entry.media_kind
  if (kind === 'music') {
    return [
      { value: 'music_track', label: '单曲元数据 + 封面' },
      { value: 'lyrics', label: 'LRC 同步歌词' },
      { value: 'music_album', label: '整张专辑' },
    ]
  }
  if (kind === 'video') {
    return [
      { value: 'tv_show', label: '剧集 / 番剧 (含 SxxExx)' },
      { value: 'movie', label: '电影' },
    ]
  }
  if (kind === 'comic') {
    return [
      { value: 'comic', label: '漫画系列 / 单行本' },
      { value: 'book', label: '轻小说 / 图书源' },
    ]
  }
  return [{ value: 'book', label: '电子书 / 出版物' }]
})

async function refreshProvidersForTarget(target: ScrapeTargetKind): Promise<void> {
  try {
    const list = await toolApi.listProviders(target)
    providers.value = list
    enabledProviderIds.value = list.filter((p) => p.configured).map((p) => p.id)
  } catch {
    providers.value = []
  }
}

function seedFromDetail(d: MediaFileDetail): void {
  errorMsg.value = null
  candidates.value = []
  selectedCandidate.value = null
  previewDetail.value = null

  const m = d.metadata
  if (m.kind === 'music') {
    targetKind.value = 'music_track'
    keyword.value = m.data.title || d.entry.display_title
    artistOrAuthor.value = m.data.artist || ''
    albumOrSeries.value = m.data.album || ''
  } else if (m.kind === 'video') {
    targetKind.value = m.data.is_episode ? 'tv_show' : 'movie'
    keyword.value = (m.data.is_episode && m.data.show_title) || m.data.title || d.entry.display_title
    season.value = m.data.season_number !== null ? String(m.data.season_number) : ''
    episode.value = m.data.episode_number !== null ? String(m.data.episode_number) : ''
  } else if (m.kind === 'comic') {
    targetKind.value = 'comic'
    keyword.value = m.data.series || m.data.title || d.entry.display_title
    artistOrAuthor.value = m.data.writers[0] ?? ''
  } else if (m.kind === 'book') {
    targetKind.value = 'book'
    keyword.value = m.data.title || d.entry.display_title
    artistOrAuthor.value = m.data.authors[0] ?? ''
    isbn.value = m.data.isbn ?? ''
  }
}

watch(
  () => [props.open, props.detail] as const,
  async ([isOpen, d]) => {
    if (isOpen && d) {
      seedFromDetail(d)
      await refreshProvidersForTarget(targetKind.value)
      if (keyword.value.trim()) {
        void runSearch()
      }
    }
  },
)

watch(targetKind, async (next) => {
  if (!props.open) return
  await refreshProvidersForTarget(next)
})

function toggleProvider(p: ProviderStatusDto): void {
  if (!p.configured) {
    emit('openSettings')
    return
  }
  if (enabledProviderIds.value.includes(p.id)) {
    enabledProviderIds.value = enabledProviderIds.value.filter((id) => id !== p.id)
  } else {
    enabledProviderIds.value = [...enabledProviderIds.value, p.id]
  }
}

async function runSearch(): Promise<void> {
  if (!keyword.value.trim() && !isbn.value.trim()) return
  searching.value = true
  errorMsg.value = null
  candidates.value = []
  selectedCandidate.value = null
  previewDetail.value = null

  try {
    const list = await toolApi.searchScrapers({
      target: targetKind.value,
      providers: enabledProviderIds.value,
      keyword: keyword.value.trim(),
      artist_or_author: artistOrAuthor.value.trim() || undefined,
      album_or_series: albumOrSeries.value.trim() || undefined,
      isbn: isbn.value.trim() || undefined,
      season: season.value.trim() ? Number.parseInt(season.value, 10) : undefined,
      episode: episode.value.trim() ? Number.parseInt(episode.value, 10) : undefined,
    })
    candidates.value = list
    if (list.length > 0) {
      await selectCandidate(list[0]!)
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    searching.value = false
  }
}

async function selectCandidate(c: ScrapeCandidate): Promise<void> {
  selectedCandidate.value = c
  loadingDetail.value = true
  errorMsg.value = null
  try {
    previewDetail.value = await toolApi.getScraperDetail(c.provider_id, c.external_id, c.target)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    loadingDetail.value = false
  }
}

async function applyMetadata(): Promise<void> {
  if (!props.detail || !previewDetail.value || applying.value) return
  applying.value = true
  errorMsg.value = null
  try {
    const updated = await toolApi.applyScrapedMetadata(
      props.detail.entry.root_id,
      props.detail.entry.rel_path,
      props.detail.entry.media_kind,
      previewDetail.value,
      applyCover.value,
      writeSidecarLrc.value,
    )
    emit('applied', updated)
    emit('close')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    applying.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open && detail"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-3 backdrop-blur-xs sm:p-6"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[90vh] w-full max-w-5xl flex-col overflow-hidden rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] shadow-2xl"
      >
        <!-- Header -->
        <div
          class="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--ctp-surface0)]/80 px-5 py-4"
        >
          <div class="min-w-0">
            <div class="flex items-center gap-2">
              <LuSparkles class="h-4 w-4 text-[var(--ctp-accent)]" />
              <h2 class="text-sm font-bold text-[var(--ctp-text)]">在线刮削元信息与封面</h2>
            </div>
            <p class="mt-0.5 truncate text-xs text-[var(--ctp-subtext0)]">
              目标文件：{{ detail.entry.file_name }}
            </p>
          </div>
          <button
            type="button"
            class="flex h-8 w-8 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <!-- Target Tabs + Dynamic Provider Pills + Query Bar -->
        <div class="shrink-0 space-y-3 border-b border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/40 px-5 py-3.5">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <!-- Target Selector -->
            <div class="flex flex-wrap items-center gap-1 rounded-xl bg-[var(--ctp-mantle)] p-1">
              <button
                v-for="tab in targetTabs"
                :key="tab.value"
                type="button"
                class="rounded-lg px-3 py-1 text-xs font-semibold transition"
                :class="
                  targetKind === tab.value
                    ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                    : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                "
                @click="targetKind = tab.value"
              >
                {{ tab.label }}
              </button>
            </div>

            <!-- Dynamic Pluggable Providers -->
            <div class="flex flex-wrap items-center gap-1.5">
              <span class="text-[11px] text-[var(--ctp-subtext0)]">刮削源：</span>
              <button
                v-for="prov in providers"
                :key="prov.id"
                type="button"
                :title="prov.description"
                class="inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-[11px] font-medium transition"
                :class="
                  !prov.configured
                    ? 'border-dashed border-amber-500/45 bg-amber-500/10 text-amber-400'
                    : enabledProviderIds.includes(prov.id)
                      ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                      : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext0)]'
                "
                @click="toggleProvider(prov)"
              >
                <LuCheck
                  v-if="prov.configured && enabledProviderIds.includes(prov.id)"
                  class="h-3 w-3"
                />
                <LuKey v-else-if="!prov.configured" class="h-3 w-3" />
                <span>{{ prov.name }}</span>
                <span v-if="!prov.configured" class="text-[10px]">(去配置)</span>
              </button>
            </div>
          </div>

          <form class="grid grid-cols-1 gap-2 sm:grid-cols-12" @submit.prevent="runSearch">
            <input
              v-model="keyword"
              type="text"
              placeholder="标题 / 关键词…"
              class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-5"
            />
            <input
              v-if="detail.entry.media_kind !== 'video'"
              v-model="artistOrAuthor"
              type="text"
              :placeholder="detail.entry.media_kind === 'music' ? '歌手 (可选)…' : '作者 (可选)…'"
              class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-3"
            />
            <input
              v-if="detail.entry.media_kind === 'book'"
              v-model="isbn"
              type="text"
              placeholder="ISBN (可选)…"
              class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-2"
            />
            <input
              v-if="detail.entry.media_kind === 'music'"
              v-model="albumOrSeries"
              type="text"
              placeholder="专辑 (可选)…"
              class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-2"
            />
            <template v-if="detail.entry.media_kind === 'video' && targetKind === 'tv_show'">
              <input
                v-model="season"
                type="number"
                min="0"
                placeholder="季 S"
                class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-2"
              />
              <input
                v-model="episode"
                type="number"
                min="0"
                placeholder="集 E"
                class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-3"
              />
            </template>
            <button
              type="submit"
              :disabled="searching"
              class="inline-flex items-center justify-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] transition hover:opacity-95 disabled:opacity-50 sm:col-span-2"
            >
              <LuRefreshCw v-if="searching" class="h-3.5 w-3.5 animate-spin" />
              <LuSearch v-else class="h-3.5 w-3.5" />
              <span>搜索</span>
            </button>
          </form>
        </div>

        <!-- Split Results + Rich Preview -->
        <div class="grid min-h-0 flex-1 grid-cols-1 divide-y divide-[var(--ctp-surface0)]/75 overflow-hidden md:grid-cols-12 md:divide-x md:divide-y-0">
          <!-- Candidate List -->
          <div class="overflow-y-auto p-3 md:col-span-5">
            <div v-if="searching" class="py-12 text-center text-xs text-[var(--ctp-subtext0)]">
              正在并发检索已启用的刮削源…
            </div>
            <div
              v-else-if="candidates.length === 0"
              class="py-12 text-center text-xs text-[var(--ctp-subtext0)]"
            >
              暂无候选结果，请调整关键词或配置对应刮削源密钥后重试
            </div>
            <div v-else class="space-y-1.5">
              <button
                v-for="cand in candidates"
                :key="`${cand.provider_id}-${cand.external_id}`"
                type="button"
                class="flex w-full items-start gap-3 rounded-2xl p-2.5 text-left transition"
                :class="
                  selectedCandidate?.provider_id === cand.provider_id &&
                  selectedCandidate?.external_id === cand.external_id
                    ? 'bg-[var(--ctp-accent-soft)] ring-1 ring-[var(--ctp-accent)]'
                    : 'hover:bg-[var(--ctp-surface0)]/55'
                "
                @click="selectCandidate(cand)"
              >
                <div class="h-14 w-11 shrink-0 overflow-hidden rounded-lg bg-[var(--ctp-mantle)]">
                  <img
                    v-if="cand.cover_url"
                    :src="cand.cover_url"
                    alt=""
                    class="h-full w-full object-cover"
                    loading="lazy"
                  />
                </div>
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-1.5">
                    <span
                      class="rounded-md bg-[var(--ctp-mantle)] px-1.5 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
                    >
                      {{ cand.provider_name }}
                    </span>
                    <span v-if="cand.year" class="text-[10px] text-[var(--ctp-subtext0)]">
                      {{ cand.year }}
                    </span>
                  </div>
                  <div class="mt-1 truncate text-xs font-semibold text-[var(--ctp-text)]">
                    {{ cand.title }}
                  </div>
                  <div
                    v-if="cand.subtitle"
                    class="mt-0.5 truncate text-[11px] text-[var(--ctp-subtext0)]"
                  >
                    {{ cand.subtitle }}
                  </div>
                </div>
              </button>
            </div>
          </div>

          <!-- Detail Preview -->
          <div class="overflow-y-auto p-4 md:col-span-7">
            <div
              v-if="errorMsg"
              class="mb-3 rounded-xl border border-red-500/35 bg-red-500/10 px-3 py-2 text-xs text-red-400"
            >
              {{ errorMsg }}
            </div>

            <div v-if="loadingDetail" class="py-16 text-center text-xs text-[var(--ctp-subtext0)]">
              正在拉取完整条目详情…
            </div>
            <div v-else-if="previewDetail" class="space-y-4">
              <div class="flex gap-4">
                <div
                  v-if="previewDetail.cover_url"
                  class="h-36 w-26 shrink-0 overflow-hidden rounded-xl bg-[var(--ctp-mantle)] shadow-md"
                >
                  <img :src="previewDetail.cover_url" alt="" class="h-full w-full object-cover" />
                </div>
                <div class="min-w-0 flex-1 space-y-1.5">
                  <div class="text-base font-bold text-[var(--ctp-text)]">
                    {{ previewDetail.title || selectedCandidate?.title }}
                  </div>
                  <div
                    v-if="previewDetail.original_title && previewDetail.original_title !== previewDetail.title"
                    class="text-xs text-[var(--ctp-subtext0)]"
                  >
                    原名：{{ previewDetail.original_title }}
                  </div>
                  <div v-if="previewDetail.series_or_album" class="text-xs text-[var(--ctp-subtext1)]">
                    系列 / 专辑：{{ previewDetail.series_or_album }}
                    <template v-if="previewDetail.disc_or_season_number || previewDetail.track_or_episode_number">
                      · #{{ previewDetail.disc_or_season_number ?? 1 }}-{{ previewDetail.track_or_episode_number ?? 1 }}
                    </template>
                  </div>
                  <div v-if="previewDetail.creators.length > 0" class="text-xs text-[var(--ctp-subtext1)]">
                    创作者：{{ previewDetail.creators.join(' / ') }}
                  </div>
                  <div
                    v-if="previewDetail.illustrators_or_directors.length > 0"
                    class="text-xs text-[var(--ctp-subtext1)]"
                  >
                    导演 / 作画：{{ previewDetail.illustrators_or_directors.join(' / ') }}
                  </div>
                  <div v-if="previewDetail.publisher_or_studio" class="text-xs text-[var(--ctp-subtext0)]">
                    出品 / 出版社：{{ previewDetail.publisher_or_studio }}
                    <template v-if="previewDetail.year"> · {{ previewDetail.year }}</template>
                    <template v-if="previewDetail.rating"> · ★ {{ previewDetail.rating.toFixed(1) }}</template>
                  </div>
                  <div class="flex flex-wrap gap-1 pt-1">
                    <span
                      v-for="tag in [...previewDetail.genres, ...previewDetail.tags].slice(0, 8)"
                      :key="tag"
                      class="rounded-full bg-[var(--ctp-mantle)] px-2 py-0.5 text-[10px] text-[var(--ctp-subtext1)]"
                    >
                      {{ tag }}
                    </span>
                  </div>
                </div>
              </div>

              <div v-if="previewDetail.summary" class="rounded-2xl bg-[var(--ctp-mantle)]/65 p-3">
                <div class="mb-1 text-[11px] font-semibold text-[var(--ctp-subtext0)]">内容简介</div>
                <p class="whitespace-pre-line text-xs leading-relaxed text-[var(--ctp-text)]">
                  {{ previewDetail.summary }}
                </p>
              </div>

              <div
                v-if="previewDetail.synced_lyrics || previewDetail.plain_lyrics"
                class="rounded-2xl bg-[var(--ctp-mantle)]/65 p-3"
              >
                <div class="mb-1 flex items-center justify-between text-[11px] font-semibold text-[var(--ctp-subtext0)]">
                  <span>歌词预览 ({{ previewDetail.synced_lyrics ? 'LRC 时间轴同步歌词' : '纯文本歌词' }})</span>
                </div>
                <pre
                  class="max-h-52 overflow-y-auto font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-[var(--ctp-text)]"
                >{{ previewDetail.synced_lyrics || previewDetail.plain_lyrics }}</pre>
              </div>
            </div>
            <div v-else class="flex h-full items-center justify-center text-xs text-[var(--ctp-subtext0)]">
              选择左侧搜索结果以预览刮削详情
            </div>
          </div>
        </div>

        <!-- Footer -->
        <div
          class="flex shrink-0 flex-wrap items-center justify-between gap-3 border-t border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 px-5 py-3.5"
        >
          <div class="flex flex-wrap items-center gap-4">
            <AppCheckbox
              v-if="previewDetail?.cover_url || previewDetail?.backdrop_url"
              v-model="applyCover"
              label="同时下载并写入封面 / 海报图片"
            />
            <AppCheckbox
              v-if="
                detail.entry.media_kind === 'music' &&
                (previewDetail?.synced_lyrics || previewDetail?.plain_lyrics)
              "
              v-model="writeSidecarLrc"
              label="同步生成同名外挂 .lrc 歌词文件"
            />
          </div>

          <div class="flex items-center gap-2">
            <button
              type="button"
              class="rounded-xl px-4 py-2 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/65"
              @click="emit('close')"
            >
              取消
            </button>
            <button
              type="button"
              :disabled="!previewDetail || applying"
              class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
              @click="applyMetadata"
            >
              <LuRefreshCw v-if="applying" class="h-3.5 w-3.5 animate-spin" />
              <LuExternalLink v-else class="h-3.5 w-3.5" />
              <span>{{ applying ? '正在写入原文件…' : '应用并写入原文件' }}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
