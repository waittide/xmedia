<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  LuAlertCircle,
  LuArrowDown,
  LuArrowUp,
  LuCheckCircle2,
  LuSave,
  LuSettings,
  LuTrash2,
} from 'vue-icons-plus/lu'
import MdSelect from '@/components/ui/MdSelect.vue'
import MdSwitch from '@/components/ui/MdSwitch.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import type { FfmpegStatusDto, MediaKind } from '@/types/api'
import { toolApi } from '../../api'
import { formatBytes } from '../../i18n'
import type {
  ProviderConfigOverrideDto,
  ProviderStatusDto,
  PublicSettingsDto,
  ScrapeCacheStatsDto,
  ScraperPrioritiesDto,
} from '../../types'
import ScraperProviderConfigDialog from './ScraperProviderConfigDialog.vue'

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

const proxyInput = ref('')
const scrapeLocale = ref('zh-CN')
const cacheDays = ref(7)
const cacheHours = ref(0)
const cacheStats = ref<ScrapeCacheStatsDto | null>(null)
const clearingCache = ref(false)
const cacheClearedBanner = ref<string | null>(null)
const saving = ref(false)
const busyProviderId = ref<string | null>(null)
const editingProvider = ref<ProviderStatusDto | null>(null)
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
  music: [
    'qq',
    'netease',
    'kugou',
    'kuwo',
    'migu',
    'taihe',
    'qishui',
    'itunes',
    'bangumi',
    'musicbrainz',
    'lrclib',
  ],
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
  if (d === 0 && h === 0 && ms > 0) h = 1
  cacheDays.value = d
  cacheHours.value = h
}

watch(
  () => props.settings,
  (s) => {
    if (!s) return
    proxyInput.value = s.http_proxy ?? ''
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

async function loadProviders(): Promise<void> {
  allProviders.value = await toolApi.listProviders()
}

async function loadCacheStats(): Promise<void> {
  try {
    cacheStats.value = await toolApi.getScrapeCacheStats()
  } catch {
    cacheStats.value = null
  }
}

onMounted(async () => {
  await Promise.allSettled([loadProviders(), loadCacheStats()])
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

function providerHasCustomConfig(providerId: string): boolean {
  const cfg: ProviderConfigOverrideDto | undefined = props.settings?.provider_configs?.[providerId]
  const hasOverride = Boolean(cfg?.http_proxy || cfg?.locale || cfg?.timeout_secs || cfg?.user_agent)
  if (hasOverride) return true
  if (providerId === 'tmdb') return Boolean(props.settings?.has_tmdb_api_key)
  if (providerId === 'bangumi') return Boolean(props.settings?.has_bangumi_token)
  if (providerId === 'qishui') return Boolean(props.settings?.has_qishui_cookie)
  return false
}

function providerStatusBadge(prov: ProviderStatusDto): { label: string; cls: string } {
  if (!prov.enabled) {
    return {
      label: '已禁用',
      cls: 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]',
    }
  }
  if (!prov.credentials_ready) {
    return {
      label: prov.requires_api_key ? '需配置密钥' : '未就绪',
      cls: 'bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]',
    }
  }
  return {
    label: '已就绪',
    cls: 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]',
  }
}

async function toggleProviderEnabled(prov: ProviderStatusDto, nextEnabled: boolean): Promise<void> {
  if (busyProviderId.value === prov.id) return
  busyProviderId.value = prov.id
  emit('error', null)
  try {
    const existing = props.settings?.provider_configs?.[prov.id] ?? {}
    const next = await toolApi.updateSettings({
      provider_configs: {
        [prov.id]: {
          ...existing,
          enabled: nextEnabled,
        },
      },
    })
    emit('updated', next)
    await loadProviders()
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    busyProviderId.value = null
  }
}

async function moveProviderPriority(dom: MediaKind, idx: number, delta: -1 | 1): Promise<void> {
  const current = normalizeDomainOrder(dom, priorities.value[dom])
  const targetIdx = idx + delta
  if (targetIdx < 0 || targetIdx >= current.length) return
  const copy = [...current]
  const [moved] = copy.splice(idx, 1)
  copy.splice(targetIdx, 0, moved!)
  const nextPriorities: ScraperPrioritiesDto = {
    book: dom === 'book' ? copy : normalizeDomainOrder('book', priorities.value.book),
    comic: dom === 'comic' ? copy : normalizeDomainOrder('comic', priorities.value.comic),
    music: dom === 'music' ? copy : normalizeDomainOrder('music', priorities.value.music),
    video: dom === 'video' ? copy : normalizeDomainOrder('video', priorities.value.video),
  }
  priorities.value = nextPriorities
  emit('error', null)
  try {
    const next = await toolApi.updateSettings({ scraper_priorities: nextPriorities })
    emit('updated', next)
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  }
}

async function handleProviderDialogUpdated(next: PublicSettingsDto): Promise<void> {
  emit('updated', next)
  emit('saved')
  await loadProviders()
}

let cacheClearedTimer: ReturnType<typeof setTimeout> | null = null

async function handleClearCache(): Promise<void> {
  clearingCache.value = true
  emit('error', null)
  try {
    const res = await toolApi.clearScrapeCache()
    await loadCacheStats()
    if (cacheClearedTimer) clearTimeout(cacheClearedTimer)
    cacheClearedBanner.value = `已清理 ${res.removed_count} 条刮削缓存记录`
    cacheClearedTimer = setTimeout(() => {
      cacheClearedTimer = null
      cacheClearedBanner.value = null
    }, 2400)
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    clearingCache.value = false
  }
}

onBeforeUnmount(() => {
  if (cacheClearedTimer) {
    clearTimeout(cacheClearedTimer)
    cacheClearedTimer = null
  }
})

async function handleSaveGlobalSettings(): Promise<void> {
  saving.value = true
  emit('error', null)
  try {
    const next = await toolApi.updateSettings({
      http_proxy: proxyInput.value,
      default_scrape_locale: scrapeLocale.value,
      scrape_cache_ttl_ms: computedCacheTtlMs.value,
    })
    emit('updated', next)
    await Promise.allSettled([loadProviders(), loadCacheStats()])
    emit('saved')
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <!-- Domain Priority Waterfall Section -->
  <section class="md-card-filled space-y-4 p-6">
    <div>
      <h2 class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
        「{{ domainBadgeLabels[domainSubTab] }}」刮削源与优先级
      </h2>
      <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
        按顺序依次补齐缺失字段；可单独启停任意刮削源或点击设置图标独立配置密钥、语言与代理
      </p>
    </div>

    <div class="space-y-2">
      <div
        v-for="(prov, idx) in orderedProvidersForDomain"
        :key="prov.id"
        class="flex items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container-low)] px-4 py-3 transition"
        :class="!prov.enabled ? 'opacity-65' : ''"
      >
        <div class="flex min-w-0 flex-1 items-center gap-3">
          <span
            class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[var(--md-sys-color-secondary-container)] font-mono text-xs font-semibold text-[var(--md-sys-color-on-secondary-container)]"
          >
            {{ idx + 1 }}
          </span>
          <div class="min-w-0">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-[13px] font-semibold text-[var(--md-sys-color-on-surface)]">
                {{ prov.name }}
              </span>
              <span
                class="rounded-full px-2 py-0.5 text-[10px] font-semibold"
                :class="providerStatusBadge(prov).cls"
              >
                {{ providerStatusBadge(prov).label }}
              </span>
            </div>
            <p
              class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
              :title="prov.description"
            >
              {{ prov.description }}
            </p>
          </div>
        </div>

        <!-- Per-provider Controls: [Switch] [Settings] [Up] [Down] -->
        <div class="flex shrink-0 items-center gap-1.5">
          <MdSwitch
            :model-value="prov.enabled"
            :disabled="busyProviderId === prov.id"
            :title="prov.enabled ? '禁用此刮削源' : '启用此刮削源'"
            @update:model-value="(val) => toggleProviderEnabled(prov, val)"
          />

          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state relative"
            :class="
              providerHasCustomConfig(prov.id)
                ? '!text-[var(--md-sys-color-primary)]'
                : ''
            "
            title="独立配置此刮削源（密钥 / 语言 / 代理 / 超时）"
            @click="editingProvider = prov"
          >
            <LuSettings class="h-4 w-4" />
            <span
              v-if="providerHasCustomConfig(prov.id)"
              class="absolute top-1.5 right-1.5 h-1.5 w-1.5 rounded-full bg-[var(--md-sys-color-primary)]"
            />
          </button>

          <div class="mx-0.5 h-4 w-px bg-[var(--md-sys-color-outline-variant)]/70" />

          <button
            type="button"
            :disabled="idx === 0"
            class="md-icon-btn md-icon-btn-sm md-state"
            title="提高优先级"
            @click="moveProviderPriority(domainSubTab, idx, -1)"
          >
            <LuArrowUp class="h-4 w-4" />
          </button>
          <button
            type="button"
            :disabled="idx === orderedProvidersForDomain.length - 1"
            class="md-icon-btn md-icon-btn-sm md-state"
            title="降低优先级"
            @click="moveProviderPriority(domainSubTab, idx, 1)"
          >
            <LuArrowDown class="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  </section>

  <!-- Metadata Cache Duration -->
  <section class="md-card-filled space-y-4 p-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">刮削元数据缓存</h2>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          自动缓存各数据源搜索与详情结果，避免重复请求（设为 0 天 0 小时将禁用缓存）
        </p>
      </div>

      <div class="flex items-center gap-1.5">
        <RefreshButton :action="loadCacheStats" />
        <button
          type="button"
          :disabled="clearingCache || !cacheStats || cacheStats.total_entries === 0"
          class="md-btn md-btn-sm md-btn-outlined md-state"
          @click="handleClearCache"
        >
          <LuTrash2 class="h-3.5 w-3.5" />
          <span>清空缓存</span>
        </button>
      </div>
    </div>

    <div
      v-if="cacheClearedBanner"
      class="rounded-2xl bg-[var(--md-sys-color-success-container)] px-3.5 py-2 text-xs font-medium text-[var(--md-sys-color-on-success-container)]"
    >
      {{ cacheClearedBanner }}
    </div>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <div>
        <label class="mb-1.5 block text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">
          缓存天数
        </label>
        <div class="relative">
          <input
            v-model.number="cacheDays"
            type="number"
            min="0"
            max="3650"
            class="md-input h-10 w-full pl-3.5 pr-10 font-mono text-xs"
          />
          <span class="pointer-events-none absolute top-1/2 right-3.5 -translate-y-1/2 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            天
          </span>
        </div>
      </div>

      <div>
        <label class="mb-1.5 block text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">
          缓存小时 (0–23)
        </label>
        <div class="relative">
          <input
            v-model.number="cacheHours"
            type="number"
            min="0"
            max="23"
            class="md-input h-10 w-full pl-3.5 pr-12 font-mono text-xs"
          />
          <span class="pointer-events-none absolute top-1/2 right-3.5 -translate-y-1/2 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            小时
          </span>
        </div>
      </div>
    </div>

    <div class="flex flex-wrap items-center justify-between gap-2 pt-1">
      <div class="flex flex-wrap items-center gap-1.5">
        <button
          v-for="preset in cachePresets"
          :key="preset.label"
          type="button"
          class="md-chip md-state"
          :class="
            cacheDays === preset.days && cacheHours === preset.hours ? 'md-chip-selected' : ''
          "
          @click="cacheDays = preset.days; cacheHours = preset.hours"
        >
          {{ preset.label }}
        </button>
      </div>

      <div class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
        {{ computedCacheTtlMs.toLocaleString() }} ms
        <template v-if="cacheStats">
          · 有效 {{ cacheStats.valid_entries }} / {{ cacheStats.total_entries }} 条 ({{
            formatBytes(cacheStats.total_bytes)
          }})
        </template>
      </div>
    </div>
  </section>

  <!-- Global Defaults: Locale, Proxy & FFprobe Status -->
  <section class="md-card-filled space-y-4 p-6">
    <div>
      <h2 class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">全局抓取默认设置</h2>
      <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
        未单独配置的刮削源将默认继承以下语言与网络代理设置
      </p>
    </div>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <div>
        <label class="mb-1.5 block text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">
          默认元数据抓取语言
        </label>
        <MdSelect v-model="scrapeLocale" :options="scrapeLocaleOptions" />
      </div>

      <div>
        <label class="mb-1.5 block text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">
          全局 HTTP / SOCKS5 代理（可选）
        </label>
        <input
          v-model="proxyInput"
          type="text"
          placeholder="例如 http://127.0.0.1:7890 或 socks5h://127.0.0.1:1080"
          class="md-input h-10 w-full px-3.5 font-mono text-xs"
        />
      </div>
    </div>

    <div
      v-if="ffmpegStatus"
      class="flex flex-wrap items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container-low)] px-4 py-3"
    >
      <div>
        <div class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          FFmpeg / FFprobe 流探测引擎
        </div>
        <div class="mt-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
          {{ ffmpegStatus.ffmpeg_version || '用于探测视频分辨率、编码、内封音轨与字幕流' }}
        </div>
      </div>
      <span
        v-if="ffmpegStatus.ffprobe_available"
        class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-success-container)] px-2.5 py-1 text-[11px] font-semibold text-[var(--md-sys-color-on-success-container)]"
      >
        <LuCheckCircle2 class="h-3.5 w-3.5" />
        <span>已就绪</span>
      </span>
      <span
        v-else
        class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-warning-container)] px-2.5 py-1 text-[11px] font-semibold text-[var(--md-sys-color-on-warning-container)]"
      >
        <LuAlertCircle class="h-3.5 w-3.5" />
        <span>未检测到 ffprobe</span>
      </span>
    </div>

    <div class="flex justify-end pt-1">
      <button
        type="button"
        :disabled="saving"
        class="md-btn md-btn-filled md-state"
        @click="handleSaveGlobalSettings"
      >
        <LuSave class="h-4 w-4" />
        <span>{{ saving ? '保存中…' : '保存全局设置' }}</span>
      </button>
    </div>
  </section>

  <!-- Per-Provider Independent Settings Dialog -->
  <ScraperProviderConfigDialog
    :open="Boolean(editingProvider)"
    :provider="editingProvider"
    :settings="settings"
    @close="editingProvider = null"
    @updated="handleProviderDialogUpdated"
  />
</template>
