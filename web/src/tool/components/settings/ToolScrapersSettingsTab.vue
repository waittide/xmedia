<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import {
  LuArrowDown,
  LuArrowUp,
  LuCheckCircle2,
  LuKey,
  LuSave,
  LuTrash2,
} from 'vue-icons-plus/lu'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import type { FfmpegStatusDto, MediaKind } from '@/types/api'
import { toolApi } from '../../api'
import { formatBytes } from '../../i18n'
import type {
  ProviderStatusDto,
  PublicSettingsDto,
  ScrapeCacheStatsDto,
  ScraperPrioritiesDto,
} from '../../types'

const MS_PER_HOUR = 3_600_000
const MS_PER_DAY = 24 * MS_PER_HOUR

const props = defineProps<{
  settings: PublicSettingsDto | null
  domainSubTab: MediaKind
  ffmpegStatus?: FfmpegStatusDto | null
}>()

const emit = defineEmits<{
  updated: [settings: PublicSettingsDto]
  error: [message: string | null]
  saved: []
}>()

const domainBadgeLabels: Record<MediaKind, string> = {
  book: '图书',
  comic: '漫画',
  music: '音乐',
  video: '影视',
}

const scrapeLocaleOptions = [
  { value: 'zh-CN', label: '简体中文 (zh-CN)' },
  { value: 'zh-TW', label: '繁體中文 (zh-TW)' },
  { value: 'ja-JP', label: '日本語 (ja-JP)' },
  { value: 'en-US', label: 'English (en-US)' },
]

const cachePresets = [
  { label: '不缓存', days: 0, hours: 0 },
  { label: '12 小时', days: 0, hours: 12 },
  { label: '1 天', days: 1, hours: 0 },
  { label: '7 天', days: 7, hours: 0 },
  { label: '30 天', days: 30, hours: 0 },
]

const tmdbKeyInput = ref('')
const bangumiTokenInput = ref('')
const proxyInput = ref('')
const enableAdultScrapers = ref(false)
const scrapeLocale = ref('zh-CN')
const cacheDays = ref(7)
const cacheHours = ref(0)
const cacheStats = ref<ScrapeCacheStatsDto | null>(null)
const clearingCache = ref(false)
const cacheClearedBanner = ref<string | null>(null)
const saving = ref(false)
const allProviders = ref<ProviderStatusDto[]>([])
const priorities = ref<ScraperPrioritiesDto>({
  book: [
    'douban',
    'neodb',
    'xinhua',
    'qidian',
    'fanqie',
    'qimao',
    '17k',
    'ciweimao',
    'sfacg',
    'faloo',
    'jjwxc',
    'bangumi',
    'anilist',
    'openlibrary',
  ],
  comic: ['bangumi', 'douban', 'neodb', 'anilist', 'mangadex', 'nhentai'],
  music: ['qq', 'netease', 'kugou', 'kuwo', 'migu', 'taihe', 'qishui', 'itunes', 'bangumi', 'musicbrainz', 'lrclib'],
  video: ['tmdb', 'bangumi', 'hanime1'],
})

const computedCacheTtlMs = computed<number>(() => {
  const d = Math.max(0, Math.floor(Number(cacheDays.value) || 0))
  const h = Math.max(0, Math.min(23, Math.floor(Number(cacheHours.value) || 0)))
  return d * MS_PER_DAY + h * MS_PER_HOUR
})

function hydrateCacheDurationFromMs(ms: number): void {
  if (!ms || ms <= 0) {
    cacheDays.value = 0
    cacheHours.value = 0
    return
  }
  const d = Math.floor(ms / MS_PER_DAY)
  let h = Math.floor((ms % MS_PER_DAY) / MS_PER_HOUR)
  if (d === 0 && h === 0 && ms > 0) {
    h = 1
  }
  cacheDays.value = d
  cacheHours.value = h
}

function applyCachePreset(days: number, hours: number): void {
  cacheDays.value = days
  cacheHours.value = hours
}

watch(
  () => props.settings,
  (s) => {
    if (!s) return
    proxyInput.value = s.http_proxy ?? ''
    enableAdultScrapers.value = Boolean(s.enable_adult_scrapers)
    scrapeLocale.value = s.default_scrape_locale || 'zh-CN'
    hydrateCacheDurationFromMs(s.scrape_cache_ttl_ms)
    priorities.value = {
      book: [...s.scraper_priorities.book],
      comic: [...s.scraper_priorities.comic],
      music: [...s.scraper_priorities.music],
      video: [...s.scraper_priorities.video],
    }
  },
  { immediate: true },
)

async function loadCacheStats(): Promise<void> {
  try {
    cacheStats.value = await toolApi.getScrapeCacheStats()
  } catch {
    cacheStats.value = null
  }
}

onMounted(async () => {
  await Promise.allSettled([
    toolApi.listProviders().then((res) => {
      allProviders.value = res
    }),
    loadCacheStats(),
  ])
})

function domainSupportsProvider(dom: MediaKind, p: ProviderStatusDto): boolean {
  if (dom === 'book') return p.supported_targets.includes('book')
  if (dom === 'comic') return p.supported_targets.includes('comic')
  if (dom === 'music') {
    return (
      p.supported_targets.includes('music_track') ||
      p.supported_targets.includes('music_album') ||
      p.supported_targets.includes('lyrics')
    )
  }
  return p.supported_targets.includes('movie') || p.supported_targets.includes('tv_show')
}

function normalizeDomainOrder(dom: MediaKind, currentIds: string[]): string[] {
  const valid = allProviders.value.filter((p) => domainSupportsProvider(dom, p)).map((p) => p.id)
  const ordered = currentIds.filter((id) => valid.includes(id))
  for (const id of valid) {
    if (!ordered.includes(id)) ordered.push(id)
  }
  return ordered
}

const orderedProvidersForDomain = computed<ProviderStatusDto[]>(() => {
  const dom = props.domainSubTab
  const order = normalizeDomainOrder(dom, priorities.value[dom])
  return order
    .map((id) => allProviders.value.find((p) => p.id === id))
    .filter((p): p is ProviderStatusDto => Boolean(p))
})

function moveProviderPriority(dom: MediaKind, idx: number, delta: -1 | 1): void {
  const current = normalizeDomainOrder(dom, priorities.value[dom])
  const targetIdx = idx + delta
  if (targetIdx < 0 || targetIdx >= current.length) return
  const copy = [...current]
  const [moved] = copy.splice(idx, 1)
  copy.splice(targetIdx, 0, moved!)
  priorities.value = {
    ...priorities.value,
    [dom]: copy,
  }
}

async function handleClearCache(): Promise<void> {
  clearingCache.value = true
  emit('error', null)
  try {
    const res = await toolApi.clearScrapeCache()
    await loadCacheStats()
    cacheClearedBanner.value = `已清理 ${res.removed_count} 条刮削缓存记录`
    setTimeout(() => {
      cacheClearedBanner.value = null
    }, 2400)
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    clearingCache.value = false
  }
}

async function handleSaveSettings(): Promise<void> {
  saving.value = true
  emit('error', null)
  try {
    const normalizedPrio: ScraperPrioritiesDto = {
      book: normalizeDomainOrder('book', priorities.value.book),
      comic: normalizeDomainOrder('comic', priorities.value.comic),
      music: normalizeDomainOrder('music', priorities.value.music),
      video: normalizeDomainOrder('video', priorities.value.video),
    }
    const next = await toolApi.updateSettings({
      tmdb_api_key: tmdbKeyInput.value.trim() ? tmdbKeyInput.value.trim() : undefined,
      bangumi_access_token: bangumiTokenInput.value.trim()
        ? bangumiTokenInput.value.trim()
        : undefined,
      http_proxy: proxyInput.value,
      enable_adult_scrapers: enableAdultScrapers.value,
      default_scrape_locale: scrapeLocale.value,
      scrape_cache_ttl_ms: computedCacheTtlMs.value,
      scraper_priorities: normalizedPrio,
    })
    tmdbKeyInput.value = ''
    bangumiTokenInput.value = ''
    emit('updated', next)
    await Promise.allSettled([
      toolApi.listProviders().then((res) => {
        allProviders.value = res
      }),
      loadCacheStats(),
    ])
    emit('saved')
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    saving.value = false
  }
}

async function clearSecret(which: 'tmdb' | 'bangumi'): Promise<void> {
  saving.value = true
  try {
    const next = await toolApi.updateSettings(
      which === 'tmdb' ? { tmdb_api_key: '' } : { bangumi_access_token: '' },
    )
    emit('updated', next)
    allProviders.value = await toolApi.listProviders()
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <!-- Domain Priority Waterfall Section -->
  <section class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6">
    <div>
      <h2 class="text-sm font-bold text-[var(--ctp-text)]">
        「{{ domainBadgeLabels[domainSubTab] }}」刮削源优先级与瀑布流填充
      </h2>
      <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
        点击工具栏「刮削」添加后台任务时，严格按
        <strong class="text-[var(--ctp-text)]">
          本地已有非空信息 → 第 1 优先级源 → 第 2 优先级源 → 低优先级源
        </strong>
        顺序依次补齐缺失字段
      </p>
    </div>

    <div class="space-y-2">
      <div
        v-for="(prov, idx) in orderedProvidersForDomain"
        :key="prov.id"
        class="flex items-center justify-between gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 py-3"
      >
        <div class="flex min-w-0 items-center gap-3">
          <span
            class="flex h-6 w-6 shrink-0 items-center justify-center rounded-lg bg-[var(--ctp-accent-soft)] font-mono text-xs font-bold text-[var(--ctp-accent)]"
          >
            {{ idx + 1 }}
          </span>
          <div class="min-w-0">
            <div class="flex items-center gap-2">
              <span class="text-xs font-bold text-[var(--ctp-text)]">{{ prov.name }}</span>
              <span
                class="rounded-full px-2 py-0.5 text-[10px] font-semibold"
                :class="
                  prov.configured
                    ? 'bg-emerald-500/15 text-emerald-500'
                    : 'bg-amber-500/15 text-amber-500'
                "
              >
                {{ prov.configured ? '已就绪' : prov.requires_api_key ? '需配置密钥' : '未启用' }}
              </span>
            </div>
            <p class="mt-0.5 truncate text-[11px] text-[var(--ctp-subtext0)]">
              {{ prov.description }}
            </p>
          </div>
        </div>

        <div class="flex shrink-0 items-center gap-1">
          <button
            type="button"
            :disabled="idx === 0"
            class="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)] disabled:opacity-35"
            title="提高优先级"
            @click="moveProviderPriority(domainSubTab, idx, -1)"
          >
            <LuArrowUp class="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            :disabled="idx === orderedProvidersForDomain.length - 1"
            class="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)] disabled:opacity-35"
            title="降低优先级"
            @click="moveProviderPriority(domainSubTab, idx, 1)"
          >
            <LuArrowDown class="h-3.5 w-3.5" />
          </button>
        </div>
      </div>
    </div>
  </section>

  <!-- Metadata Cache Duration (Days + Hours -> Backend ms) -->
  <section class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-sm font-bold text-[var(--ctp-text)]">刮削元数据缓存</h2>
        <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
          自动持久化缓存各数据源的搜索与详情元数据，避免同系列剧集/专辑重复请求外部接口（设为 0 天 0 小时将禁用缓存）
        </p>
      </div>

      <div class="flex items-center gap-2">
        <RefreshButton :action="loadCacheStats" />
        <button
          type="button"
          :disabled="clearingCache || !cacheStats || cacheStats.total_entries === 0"
          class="inline-flex h-8 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 text-xs font-semibold text-[var(--ctp-subtext1)] transition hover:border-red-500/50 hover:text-red-500 disabled:opacity-45"
          @click="handleClearCache"
        >
          <LuTrash2 class="h-3.5 w-3.5" />
          <span>清空缓存</span>
        </button>
      </div>
    </div>

    <div
      v-if="cacheClearedBanner"
      class="rounded-xl bg-emerald-500/12 px-3 py-2 text-xs font-medium text-emerald-500"
    >
      {{ cacheClearedBanner }}
    </div>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <div>
        <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
          缓存天数 (天)
        </label>
        <div class="relative">
          <input
            v-model.number="cacheDays"
            type="number"
            min="0"
            max="3650"
            class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] pr-10 pl-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
          />
          <span class="pointer-events-none absolute top-1/2 right-3.5 -translate-y-1/2 text-xs text-[var(--ctp-subtext0)]">
            天
          </span>
        </div>
      </div>

      <div>
        <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
          缓存小时 (0 - 23 小时)
        </label>
        <div class="relative">
          <input
            v-model.number="cacheHours"
            type="number"
            min="0"
            max="23"
            class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] pr-12 pl-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
          />
          <span class="pointer-events-none absolute top-1/2 right-3.5 -translate-y-1/2 text-xs text-[var(--ctp-subtext0)]">
            小时
          </span>
        </div>
      </div>
    </div>

    <!-- Quick Presets + Millisecond Precision Info -->
    <div class="flex flex-wrap items-center justify-between gap-2 pt-1">
      <div class="flex flex-wrap items-center gap-1.5">
        <span class="text-[11px] text-[var(--ctp-subtext0)]">快捷预设：</span>
        <button
          v-for="preset in cachePresets"
          :key="preset.label"
          type="button"
          class="rounded-lg border px-2.5 py-1 text-[11px] font-medium transition"
          :class="
            cacheDays === preset.days && cacheHours === preset.hours
              ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
              : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext1)] hover:border-[var(--ctp-accent)]'
          "
          @click="applyCachePreset(preset.days, preset.hours)"
        >
          {{ preset.label }}
        </button>
      </div>

      <div class="font-mono text-[11px] text-[var(--ctp-subtext0)]">
        后端存储精度：{{ computedCacheTtlMs.toLocaleString() }} ms
        <template v-if="cacheStats">
          · 有效 {{ cacheStats.valid_entries }} / {{ cacheStats.total_entries }} 条 ({{
            formatBytes(cacheStats.total_bytes)
          }})
        </template>
      </div>
    </div>
  </section>

  <!-- Locale, Proxy & API Keys -->
  <section class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6">
    <h2 class="text-sm font-bold text-[var(--ctp-text)]">刮削语言、网络代理与第三方凭证</h2>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <div>
        <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
          默认元数据抓取语言
        </label>
        <AppSelect v-model="scrapeLocale" :options="scrapeLocaleOptions" />
      </div>

      <div>
        <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
          HTTP / SOCKS5 代理（可选）
        </label>
        <input
          v-model="proxyInput"
          type="text"
          placeholder="例如 http://127.0.0.1:7890 或 socks5h://127.0.0.1:1080"
          class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </div>
    </div>

    <div class="space-y-2 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2 text-xs font-bold text-[var(--ctp-text)]">
          <LuKey class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
          <span>TMDB API Key / v4 Read Access Token</span>
        </div>
        <div class="flex items-center gap-2">
          <span
            v-if="settings?.has_tmdb_api_key"
            class="rounded-full bg-emerald-500/15 px-2.5 py-0.5 font-mono text-[10px] font-semibold text-emerald-500"
          >
            已配置 {{ settings.tmdb_api_key_masked }}
          </span>
          <button
            v-if="settings?.has_tmdb_api_key"
            type="button"
            class="text-[11px] text-red-400 hover:underline"
            @click="clearSecret('tmdb')"
          >
            清空
          </button>
        </div>
      </div>
      <input
        v-model="tmdbKeyInput"
        type="password"
        placeholder="输入新的 TMDB v3 API Key 或 v4 Bearer Token（留空保持不变）"
        class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/60 px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>

    <div class="space-y-2 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2 text-xs font-bold text-[var(--ctp-text)]">
          <LuKey class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
          <span>Bangumi Access Token（可选，解锁更高频限额）</span>
        </div>
        <div class="flex items-center gap-2">
          <span
            v-if="settings?.has_bangumi_token"
            class="rounded-full bg-emerald-500/15 px-2.5 py-0.5 font-mono text-[10px] font-semibold text-emerald-500"
          >
            已配置 {{ settings.bangumi_token_masked }}
          </span>
          <button
            v-if="settings?.has_bangumi_token"
            type="button"
            class="text-[11px] text-red-400 hover:underline"
            @click="clearSecret('bangumi')"
          >
            清空
          </button>
        </div>
      </div>
      <input
        v-model="bangumiTokenInput"
        type="password"
        placeholder="输入 Bangumi 个人令牌（留空保持不变）"
        class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/60 px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>

    <!-- R18 / Adult Scrapers Opt-in Toggle -->
    <div
      class="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-4 py-3.5"
    >
      <div class="min-w-0 flex-1">
        <div class="text-xs font-bold text-[var(--ctp-text)]">
          启用 R18 / 成人向刮削源（nhentai / Hanime1）
        </div>
        <div class="mt-0.5 text-[11px] text-[var(--ctp-subtext0)]">
          默认关闭以避免普通漫画与剧集批量刮削误匹配；开启后支持 nhentai（漫画）与 Hanime1（动漫），建议配合上方 HTTP 代理使用
        </div>
      </div>
      <AppSwitch v-model="enableAdultScrapers" />
    </div>

    <!-- FFprobe Status -->
    <div
      v-if="ffmpegStatus"
      class="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-4 py-3"
    >
      <div>
        <div class="text-xs font-bold text-[var(--ctp-text)]">FFmpeg / FFprobe 流探测引擎</div>
        <div class="mt-0.5 text-[11px] text-[var(--ctp-subtext0)]">
          {{ ffmpegStatus.ffmpeg_version || '用于探测视频分辨率、编码、内封音轨与字幕流' }}
        </div>
      </div>
      <span
        v-if="ffmpegStatus.ffprobe_available"
        class="inline-flex items-center gap-1 rounded-full bg-emerald-500/15 px-2.5 py-1 text-[11px] font-semibold text-emerald-500"
      >
        <LuCheckCircle2 class="h-3.5 w-3.5" />
        <span>已就绪</span>
      </span>
      <span
        v-else
        class="inline-flex items-center gap-1 rounded-full bg-amber-500/15 px-2.5 py-1 text-[11px] font-semibold text-amber-500"
      >
        <LuAlertCircle class="h-3.5 w-3.5" />
        <span>未检测到 ffprobe</span>
      </span>
    </div>

    <div class="flex justify-end pt-1">
      <button
        type="button"
        :disabled="saving"
        class="inline-flex h-10 items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
        @click="handleSaveSettings"
      >
        <LuSave class="h-4 w-4" />
        <span>{{ saving ? '保存中…' : '保存刮削与缓存设置' }}</span>
      </button>
    </div>
  </section>
</template>
