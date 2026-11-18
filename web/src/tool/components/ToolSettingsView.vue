<script setup lang="ts">
import { computed, onMounted, ref, watch, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuArrowDown,
  LuArrowLeft,
  LuArrowUp,
  LuBookOpen,
  LuCheck,
  LuCheckCircle2,
  LuClapperboard,
  LuDisc3,
  LuFileSignature,
  LuFolderPlus,
  LuKey,
  LuLayers,
  LuLibrary,
  LuMonitor,
  LuMoon,
  LuPalette,
  LuRotateCcw,
  LuRotateCw,
  LuSave,
  LuSlidersHorizontal,
  LuSparkles,
  LuSun,
  LuTrash2,
} from 'vue-icons-plus/lu'
import {
  CATPPUCCIN_ACCENTS,
  CATPPUCCIN_ACCENT_HEX,
  DARK_PALETTE_PRESETS,
  LIGHT_PALETTE_PRESETS,
  normalizeHex,
  type CatppuccinAccent,
  type CustomPaletteTokens,
} from '@/theme/catppuccin'
import AppColorPicker from '@/components/ui/AppColorPicker.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { useThemeStore } from '@/stores/theme'
import type { FfmpegStatusDto, LocaleCode, MediaKind } from '@/types/api'
import { toolApi } from '../api'
import { formatBytes } from '../i18n'
import type {
  ManagedRootDto,
  ProviderStatusDto,
  PublicSettingsDto,
  RenameTemplatesDto,
  ScrapeCacheStatsDto,
  ScraperPrioritiesDto,
} from '../types'

export type ToolSettingsTab = 'appearance' | 'scrapers' | 'roots' | 'templates'

const MS_PER_HOUR = 3_600_000
const MS_PER_DAY = 24 * MS_PER_HOUR

const props = defineProps<{
  settings: PublicSettingsDto | null
  activeDomain: MediaKind
  initialTab?: ToolSettingsTab
  ffmpegStatus?: FfmpegStatusDto | null
}>()

const emit = defineEmits<{
  back: []
  updated: [settings: PublicSettingsDto]
  selectDomain: [domain: MediaKind]
}>()

const { t } = useI18n()
const themeStore = useThemeStore()

const activeTab = ref<ToolSettingsTab>(props.initialTab ?? 'appearance')
const domainSubTab = ref<MediaKind>(props.activeDomain)

watch(
  () => props.initialTab,
  (next) => {
    if (next) activeTab.value = next
  },
)

watch(
  () => props.activeDomain,
  (dom) => {
    domainSubTab.value = dom
  },
)

function selectDomainSubTab(dom: MediaKind): void {
  domainSubTab.value = dom
  emit('selectDomain', dom)
}

const domainTabItems: { value: MediaKind; label: string; icon: Component }[] = [
  { value: 'book', label: '图书', icon: LuBookOpen },
  { value: 'comic', label: '漫画', icon: LuLayers },
  { value: 'music', label: '音乐', icon: LuDisc3 },
  { value: 'video', label: '影视', icon: LuClapperboard },
]

const domainBadgeLabels: Record<MediaKind, string> = {
  book: '图书',
  comic: '漫画',
  music: '音乐',
  video: '影视',
}

// Roots state
const newRootName = ref('')
const newRootPath = ref('')
const addingRoot = ref(false)

// Scrapers & Cache state
const tmdbKeyInput = ref('')
const bangumiTokenInput = ref('')
const proxyInput = ref('')
const scrapeLocale = ref('zh-CN')
const cacheDays = ref(7)
const cacheHours = ref(0)
const cacheStats = ref<ScrapeCacheStatsDto | null>(null)
const clearingCache = ref(false)
const allProviders = ref<ProviderStatusDto[]>([])
const priorities = ref<ScraperPrioritiesDto>({
  book: ['bangumi', 'openlibrary'],
  comic: ['bangumi'],
  music: ['itunes', 'musicbrainz', 'lrclib'],
  video: ['tmdb', 'bangumi'],
})

const DEFAULT_RENAME_TEMPLATES: RenameTemplatesDto = {
  book: '{author}/{series}/{subseries}/<Vol.{volume:2} >{title} ({year})/{title} - {author}',
  comic: '{series}/{series} <Vol.{volume:2}> <Ch.{number:2}> {title}',
  music: '{album_artist}/{album} ({year})/{track:2} - {title}',
  video_movie: '{title} ({year})/{title} ({year})',
  video_episode: '{show} ({year})/Season {season:2}/{show} - S{season:2}E{episode:2} - {title}',
}

const templates = ref<RenameTemplatesDto>({ ...DEFAULT_RENAME_TEMPLATES })

const saving = ref(false)
const savedBanner = ref(false)
const cacheClearedBanner = ref<string | null>(null)
const errorMsg = ref<string | null>(null)

const uiLocaleOptions: { code: LocaleCode; nativeLabel: string; subLabel: string }[] = [
  { code: 'zh-CN', nativeLabel: '简体中文', subLabel: 'Simplified Chinese' },
  { code: 'en-US', nativeLabel: 'English', subLabel: 'English (US)' },
]

const customTokenFields: { key: keyof CustomPaletteTokens; labelKey: string }[] = [
  { key: 'canvas', labelKey: 'theme.tokenCanvas' },
  { key: 'shell', labelKey: 'theme.tokenShell' },
  { key: 'surface', labelKey: 'theme.tokenSurface' },
  { key: 'text', labelKey: 'theme.tokenText' },
  { key: 'subtext', labelKey: 'theme.tokenSubtext' },
]

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

function syncFromSettings(s: PublicSettingsDto | null): void {
  if (!s) return
  proxyInput.value = s.http_proxy ?? ''
  scrapeLocale.value = s.default_scrape_locale || 'zh-CN'
  hydrateCacheDurationFromMs(s.scrape_cache_ttl_ms)
  templates.value = { ...s.rename_templates }
  priorities.value = {
    book: [...s.scraper_priorities.book],
    comic: [...s.scraper_priorities.comic],
    music: [...s.scraper_priorities.music],
    video: [...s.scraper_priorities.video],
  }
}

watch(
  () => props.settings,
  (next) => syncFromSettings(next),
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

const filteredRoots = computed<ManagedRootDto[]>(() => {
  const all = props.settings?.roots ?? []
  return all.filter((r) => r.default_kind === domainSubTab.value)
})

function accentHex(acc: CatppuccinAccent): string {
  return CATPPUCCIN_ACCENT_HEX[themeStore.isDark ? 'dark' : 'light'][acc]
}

function onCustomTokenInput(
  mode: 'light' | 'dark',
  key: keyof CustomPaletteTokens,
  raw: string,
  fallback: string,
): void {
  const clean = normalizeHex(raw, fallback)
  themeStore.updateCustomPalette(mode, { [key]: clean })
}

function onCustomAccentInput(raw: string): void {
  const clean = normalizeHex(raw, themeStore.customAccent)
  themeStore.setCustomAccentHex(clean)
}

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
  const dom = domainSubTab.value
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

function resetDomainTemplates(dom: MediaKind): void {
  if (dom === 'book') templates.value.book = DEFAULT_RENAME_TEMPLATES.book
  else if (dom === 'comic') templates.value.comic = DEFAULT_RENAME_TEMPLATES.comic
  else if (dom === 'music') templates.value.music = DEFAULT_RENAME_TEMPLATES.music
  else {
    templates.value.video_movie = DEFAULT_RENAME_TEMPLATES.video_movie
    templates.value.video_episode = DEFAULT_RENAME_TEMPLATES.video_episode
  }
}

const domainTokenExamples: Record<MediaKind, { tokens: string[]; spec: string; sample: string }> = {
  book: {
    tokens: ['{author}', '{series}', '{subseries}', '{volume:2}', '{title}', '{year}', '{publisher}'],
    spec: 'Calibre / Kavita 规范',
    sample: '刘慈欣/地球往事/Vol.01 三体 (2008)/三体 - 刘慈欣.epub',
  },
  comic: {
    tokens: ['{series}', '{series_group}', '{volume:2}', '{number:2}', '{title}', '{year}'],
    spec: 'Komga / Kavita 规范',
    sample: '葬送的芙莉莲/葬送的芙莉莲 Vol.01 Ch.01 冒险的终点.cbz',
  },
  music: {
    tokens: ['{album_artist}', '{artist}', '{album}', '{disc}', '{track:2}', '{title}', '{year}'],
    spec: 'MusicBrainz Picard / Navidrome 规范',
    sample: '米津玄師/LOST CORNER (2024)/01 - RED OUT.flac',
  },
  video: {
    tokens: ['{title}', '{original_title}', '{show}', '{year}', '{season:2}', '{episode:2}'],
    spec: 'Jellyfin / Kodi 官方目录规范',
    sample: '葬送的芙莉莲 (2023)/Season 01/葬送的芙莉莲 - S01E01 - 冒险的终点.mkv',
  },
}

async function handleAddRoot(): Promise<void> {
  if (!newRootPath.value.trim() || addingRoot.value) return
  addingRoot.value = true
  errorMsg.value = null
  try {
    await toolApi.addRoot(newRootName.value.trim(), newRootPath.value.trim(), domainSubTab.value)
    newRootName.value = ''
    newRootPath.value = ''
    const next = await toolApi.getSettings()
    emit('updated', next)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    addingRoot.value = false
  }
}

async function handleRemoveRoot(id: string): Promise<void> {
  errorMsg.value = null
  try {
    await toolApi.removeRoot(id)
    const next = await toolApi.getSettings()
    emit('updated', next)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  }
}

async function handleClearCache(): Promise<void> {
  clearingCache.value = true
  errorMsg.value = null
  try {
    const res = await toolApi.clearScrapeCache()
    await loadCacheStats()
    cacheClearedBanner.value = `已清理 ${res.removed_count} 条刮削缓存记录`
    setTimeout(() => {
      cacheClearedBanner.value = null
    }, 2400)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    clearingCache.value = false
  }
}

async function handleSaveSettings(): Promise<void> {
  saving.value = true
  errorMsg.value = null
  savedBanner.value = false
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
      default_scrape_locale: scrapeLocale.value,
      scrape_cache_ttl_ms: computedCacheTtlMs.value,
      rename_templates: { ...templates.value },
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
    savedBanner.value = true
    setTimeout(() => {
      savedBanner.value = false
    }, 2200)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
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
  <div class="mx-auto max-w-5xl px-6 py-8 sm:px-8">
    <!-- Top Header (Aligned with Server SettingsView) -->
    <div class="mb-8 flex flex-wrap items-center justify-between gap-4">
      <div>
        <h1 class="text-2xl font-bold tracking-tight text-[var(--ctp-text)]">
          {{ t('settings.title') }}
        </h1>
        <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
          管理界面外观、各媒体类型目录白名单、刮削源优先级、元数据缓存与重命名模板
        </p>
      </div>

      <button
        type="button"
        class="inline-flex h-9 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-3.5 text-xs font-semibold text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
        @click="emit('back')"
      >
        <LuArrowLeft class="h-3.5 w-3.5" />
        <span>返回工作区</span>
      </button>
    </div>

    <!-- Server-Style Left-Right Layout -->
    <div class="flex flex-col gap-8 md:flex-row">
      <!-- Left Secondary Sidebar -->
      <nav class="flex shrink-0 gap-1 overflow-x-auto md:w-52 md:flex-col">
        <button
          type="button"
          class="flex items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold whitespace-nowrap transition"
          :class="
            activeTab === 'appearance'
              ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]'
          "
          @click="activeTab = 'appearance'"
        >
          <LuPalette class="h-4 w-4 shrink-0" />
          <span>{{ t('settings.navAppearance') }}</span>
        </button>

        <button
          type="button"
          class="flex items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold whitespace-nowrap transition"
          :class="
            activeTab === 'scrapers'
              ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]'
          "
          @click="activeTab = 'scrapers'"
        >
          <LuSparkles class="h-4 w-4 shrink-0" />
          <span>刮削与缓存</span>
        </button>

        <div class="my-1 hidden h-px bg-[var(--ctp-surface0)] md:block" />

        <button
          type="button"
          class="flex items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold whitespace-nowrap transition"
          :class="
            activeTab === 'roots'
              ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]'
          "
          @click="activeTab = 'roots'"
        >
          <LuLibrary class="h-4 w-4 shrink-0" />
          <span>媒体根目录</span>
        </button>

        <button
          type="button"
          class="flex items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold whitespace-nowrap transition"
          :class="
            activeTab === 'templates'
              ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]'
          "
          @click="activeTab = 'templates'"
        >
          <LuFileSignature class="h-4 w-4 shrink-0" />
          <span>重命名模板</span>
        </button>
      </nav>

      <!-- Right Main Content Area -->
      <div class="min-w-0 flex-1 space-y-6">
        <div
          v-if="errorMsg"
          class="rounded-2xl bg-[var(--ctp-red)]/15 px-4 py-3 text-xs font-medium text-[var(--ctp-red)]"
        >
          {{ errorMsg }}
        </div>

        <div
          v-if="savedBanner"
          class="flex items-center gap-2 rounded-2xl bg-emerald-500/15 px-4 py-3 text-xs font-semibold text-emerald-500"
        >
          <LuCheck class="h-4 w-4" />
          <span>配置已保存并立即生效</span>
        </div>

        <!-- Shared Domain Type Tab Bar for Domain-Scoped Settings -->
        <div
          v-if="activeTab !== 'appearance'"
          class="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-shell)] p-2"
        >
          <div class="flex flex-wrap items-center gap-1">
            <button
              v-for="d in domainTabItems"
              :key="d.value"
              type="button"
              class="inline-flex items-center gap-1.5 rounded-xl px-3.5 py-2 text-xs font-semibold transition"
              :class="
                domainSubTab === d.value
                  ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-xs'
                  : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
              "
              @click="selectDomainSubTab(d.value)"
            >
              <component :is="d.icon" class="h-3.5 w-3.5 shrink-0" />
              <span>{{ d.label }}</span>
            </button>
          </div>

          <span class="px-2 text-[11px] text-[var(--ctp-subtext0)]">
            当前类型：{{ domainBadgeLabels[domainSubTab] }}
          </span>
        </div>

        <!-- ==================== 1. APPEARANCE & LANGUAGE ==================== -->
        <template v-if="activeTab === 'appearance'">
          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('theme.languageTitle') }}</h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.languageDesc') }}</p>

            <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
              <button
                v-for="loc in uiLocaleOptions"
                :key="loc.code"
                type="button"
                class="flex items-center justify-between rounded-xl border px-4 py-3 text-left transition"
                :class="
                  themeStore.locale === loc.code
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] hover:border-[var(--ctp-surface1)]'
                "
                @click="themeStore.setLocale(loc.code)"
              >
                <div>
                  <p class="text-xs font-semibold text-[var(--ctp-text)]">{{ loc.nativeLabel }}</p>
                  <p class="text-[11px] text-[var(--ctp-subtext0)]">{{ loc.subLabel }}</p>
                </div>
                <LuCheck
                  v-if="themeStore.locale === loc.code"
                  class="h-4 w-4 text-[var(--ctp-accent)]"
                />
              </button>
            </div>
          </section>

          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <div class="flex flex-wrap items-start justify-between gap-3">
              <div>
                <h2 class="text-sm font-bold">{{ t('theme.modeTitle') }}</h2>
                <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.modeDesc') }}</p>
              </div>
              <button
                type="button"
                class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)]"
                @click="themeStore.resetPalettesToDefault()"
              >
                <LuRotateCw class="h-3.5 w-3.5" />
                <span>{{ t('theme.resetDefaults') }}</span>
              </button>
            </div>

            <div class="mt-4 grid grid-cols-3 gap-3">
              <button
                type="button"
                class="flex items-center justify-center gap-2 rounded-xl border py-2.5 text-xs font-semibold transition"
                :class="
                  themeStore.themeMode === 'system'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
                "
                @click="themeStore.setThemeMode('system')"
              >
                <LuMonitor class="h-4 w-4" />
                <span>{{ t('theme.system') }}</span>
              </button>
              <button
                type="button"
                class="flex items-center justify-center gap-2 rounded-xl border py-2.5 text-xs font-semibold transition"
                :class="
                  themeStore.themeMode === 'light'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
                "
                @click="themeStore.setThemeMode('light')"
              >
                <LuSun class="h-4 w-4" />
                <span>{{ t('theme.light') }}</span>
              </button>
              <button
                type="button"
                class="flex items-center justify-center gap-2 rounded-xl border py-2.5 text-xs font-semibold transition"
                :class="
                  themeStore.themeMode === 'dark'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
                "
                @click="themeStore.setThemeMode('dark')"
              >
                <LuMoon class="h-4 w-4" />
                <span>{{ t('theme.dark') }}</span>
              </button>
            </div>
          </section>

          <!-- Light & Dark Palettes -->
          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('theme.lightPaletteTitle') }}</h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.lightPaletteDesc') }}</p>

            <div class="mt-4 grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
              <button
                v-for="preset in LIGHT_PALETTE_PRESETS"
                :key="preset.id"
                type="button"
                class="flex items-center justify-between rounded-xl border px-3.5 py-3 text-left text-xs font-semibold transition"
                :class="
                  themeStore.lightPreset === preset.id
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
                "
                @click="themeStore.setLightPreset(preset.id)"
              >
                <span class="flex min-w-0 items-center gap-2">
                  <span class="flex -space-x-1">
                    <span
                      class="h-3.5 w-3.5 rounded-full border border-black/15"
                      :style="{ backgroundColor: preset.tokens.canvas }"
                    />
                    <span
                      class="h-3.5 w-3.5 rounded-full border border-black/15"
                      :style="{ backgroundColor: preset.tokens.shell }"
                    />
                    <span
                      class="h-3.5 w-3.5 rounded-full border border-black/15"
                      :style="{ backgroundColor: preset.defaultAccent }"
                    />
                  </span>
                  <span class="truncate">{{ t(`theme.presets.${preset.id}`) }}</span>
                </span>
                <LuCheck v-if="themeStore.lightPreset === preset.id" class="h-4 w-4 shrink-0" />
              </button>

              <button
                type="button"
                class="flex items-center justify-between rounded-xl border px-3.5 py-3 text-left text-xs font-semibold transition"
                :class="
                  themeStore.lightPreset === 'custom'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
                "
                @click="
                  themeStore.lightPreset === 'custom'
                    ? undefined
                    : themeStore.seedCustomFromActivePreset('light')
                "
              >
                <span class="flex items-center gap-2">
                  <LuSlidersHorizontal class="h-3.5 w-3.5 shrink-0" />
                  <span>{{ t('theme.customPalette') }}</span>
                </span>
                <LuCheck v-if="themeStore.lightPreset === 'custom'" class="h-4 w-4 shrink-0" />
              </button>
            </div>

            <div
              v-if="themeStore.lightPreset === 'custom'"
              class="mt-4 grid grid-cols-2 gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4 sm:grid-cols-5"
            >
              <div v-for="f in customTokenFields" :key="`light-${f.key}`">
                <label class="mb-1.5 block truncate text-[11px] font-medium text-[var(--ctp-subtext0)]">
                  {{ t(f.labelKey) }}
                </label>
                <div class="flex items-center gap-1.5">
                  <AppColorPicker
                    :model-value="themeStore.customLight[f.key]"
                    @update:model-value="themeStore.updateCustomPalette('light', { [f.key]: $event })"
                  />
                  <input
                    type="text"
                    :value="themeStore.customLight[f.key]"
                    maxlength="7"
                    class="h-8 w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2 font-mono text-xs outline-none focus:border-[var(--ctp-accent)]"
                    @change="
                      onCustomTokenInput(
                        'light',
                        f.key,
                        ($event.target as HTMLInputElement).value,
                        themeStore.customLight[f.key],
                      )
                    "
                  />
                </div>
              </div>
            </div>
          </section>

          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('theme.darkPaletteTitle') }}</h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.darkPaletteDesc') }}</p>

            <div class="mt-4 grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
              <button
                v-for="preset in DARK_PALETTE_PRESETS"
                :key="preset.id"
                type="button"
                class="flex items-center justify-between rounded-xl border px-3.5 py-3 text-left text-xs font-semibold transition"
                :class="
                  themeStore.darkPreset === preset.id
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
                "
                @click="themeStore.setDarkPreset(preset.id)"
              >
                <span class="flex min-w-0 items-center gap-2">
                  <span class="flex -space-x-1">
                    <span
                      class="h-3.5 w-3.5 rounded-full border border-white/20"
                      :style="{ backgroundColor: preset.tokens.canvas }"
                    />
                    <span
                      class="h-3.5 w-3.5 rounded-full border border-white/20"
                      :style="{ backgroundColor: preset.tokens.shell }"
                    />
                    <span
                      class="h-3.5 w-3.5 rounded-full border border-white/20"
                      :style="{ backgroundColor: preset.defaultAccent }"
                    />
                  </span>
                  <span class="truncate">{{ t(`theme.presets.${preset.id}`) }}</span>
                </span>
                <LuCheck v-if="themeStore.darkPreset === preset.id" class="h-4 w-4 shrink-0" />
              </button>

              <button
                type="button"
                class="flex items-center justify-between rounded-xl border px-3.5 py-3 text-left text-xs font-semibold transition"
                :class="
                  themeStore.darkPreset === 'custom'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
                "
                @click="
                  themeStore.darkPreset === 'custom'
                    ? undefined
                    : themeStore.seedCustomFromActivePreset('dark')
                "
              >
                <span class="flex items-center gap-2">
                  <LuSlidersHorizontal class="h-3.5 w-3.5 shrink-0" />
                  <span>{{ t('theme.customPalette') }}</span>
                </span>
                <LuCheck v-if="themeStore.darkPreset === 'custom'" class="h-4 w-4 shrink-0" />
              </button>
            </div>

            <div
              v-if="themeStore.darkPreset === 'custom'"
              class="mt-4 grid grid-cols-2 gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4 sm:grid-cols-5"
            >
              <div v-for="f in customTokenFields" :key="`dark-${f.key}`">
                <label class="mb-1.5 block truncate text-[11px] font-medium text-[var(--ctp-subtext0)]">
                  {{ t(f.labelKey) }}
                </label>
                <div class="flex items-center gap-1.5">
                  <AppColorPicker
                    :model-value="themeStore.customDark[f.key]"
                    @update:model-value="themeStore.updateCustomPalette('dark', { [f.key]: $event })"
                  />
                  <input
                    type="text"
                    :value="themeStore.customDark[f.key]"
                    maxlength="7"
                    class="h-8 w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2 font-mono text-xs outline-none focus:border-[var(--ctp-accent)]"
                    @change="
                      onCustomTokenInput(
                        'dark',
                        f.key,
                        ($event.target as HTMLInputElement).value,
                        themeStore.customDark[f.key],
                      )
                    "
                  />
                </div>
              </div>
            </div>
          </section>

          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('theme.accentTitle') }}</h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.accentDesc') }}</p>

            <div class="mt-4 grid grid-cols-1 gap-2.5 sm:grid-cols-3">
              <button
                type="button"
                class="rounded-xl border px-3.5 py-2.5 text-xs font-semibold transition"
                :class="
                  themeStore.accentMode === 'preset'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
                "
                @click="themeStore.setAccentMode('preset')"
              >
                {{ t('theme.accentModePreset') }}
              </button>
              <button
                type="button"
                class="rounded-xl border px-3.5 py-2.5 text-xs font-semibold transition"
                :class="
                  themeStore.accentMode === 'catppuccin'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
                "
                @click="themeStore.setAccentMode('catppuccin')"
              >
                {{ t('theme.accentModeCatppuccin') }}
              </button>
              <button
                type="button"
                class="rounded-xl border px-3.5 py-2.5 text-xs font-semibold transition"
                :class="
                  themeStore.accentMode === 'custom'
                    ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
                "
                @click="themeStore.setAccentMode('custom')"
              >
                {{ t('theme.accentModeCustom') }}
              </button>
            </div>

            <div
              v-if="themeStore.accentMode === 'catppuccin'"
              class="mt-4 flex flex-wrap gap-2.5"
            >
              <button
                v-for="acc in CATPPUCCIN_ACCENTS"
                :key="acc"
                type="button"
                class="flex h-8 w-8 items-center justify-center rounded-full border border-black/15 transition hover:scale-110"
                :style="{ backgroundColor: accentHex(acc) }"
                :title="t(`theme.accents.${acc}`)"
                @click="themeStore.setCatppuccinAccent(acc)"
              >
                <LuCheck
                  v-if="themeStore.accent === acc"
                  class="h-3.5 w-3.5"
                  :style="{ color: themeStore.isDark ? '#11111b' : '#ffffff' }"
                />
              </button>
            </div>

            <div
              v-else-if="themeStore.accentMode === 'custom'"
              class="mt-4 flex items-center gap-2.5"
            >
              <AppColorPicker
                :model-value="themeStore.customAccent"
                @update:model-value="themeStore.setCustomAccentHex($event)"
              />
              <input
                type="text"
                :value="themeStore.customAccent"
                maxlength="7"
                class="h-9 w-32 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 font-mono text-xs outline-none focus:border-[var(--ctp-accent)]"
                @change="onCustomAccentInput(($event.target as HTMLInputElement).value)"
              />
            </div>
          </section>
        </template>

        <!-- ==================== 2. SCRAPERS, PRIORITY & METADATA CACHE ==================== -->
        <template v-else-if="activeTab === 'scrapers'">
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
                        {{ prov.configured ? '已就绪' : '需配置密钥' }}
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

        <!-- ==================== 3. DOMAIN-ISOLATED MEDIA ROOTS ==================== -->
        <template v-else-if="activeTab === 'roots'">
          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold text-[var(--ctp-text)]">
              新增「{{ domainBadgeLabels[domainSubTab] }}」本地目录
            </h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
              各媒体类型的根目录白名单彼此隔离，切换上方类型 Tab 可分别管理不同领域的目录
            </p>

            <form class="mt-4 space-y-4" @submit.prevent="handleAddRoot">
              <div class="grid grid-cols-1 gap-4 sm:grid-cols-12">
                <div class="sm:col-span-4">
                  <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                    显示别名（可选）
                  </label>
                  <input
                    v-model="newRootName"
                    type="text"
                    placeholder="默认取文件夹名"
                    class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                  />
                </div>

                <div class="sm:col-span-8">
                  <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                    服务器本地绝对路径
                  </label>
                  <input
                    v-model="newRootPath"
                    type="text"
                    required
                    :placeholder="`例如 /data/media/${domainSubTab}`"
                    class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                  />
                </div>
              </div>

              <div class="flex justify-end">
                <button
                  type="submit"
                  :disabled="!newRootPath.trim() || addingRoot"
                  class="inline-flex h-10 items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
                >
                  <LuFolderPlus class="h-4 w-4" />
                  <span>添加到「{{ domainBadgeLabels[domainSubTab] }}」</span>
                </button>
              </div>
            </form>
          </section>

          <section class="card-soft space-y-3 rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold text-[var(--ctp-text)]">
              已登记「{{ domainBadgeLabels[domainSubTab] }}」根目录 ({{ filteredRoots.length }})
            </h2>

            <div
              v-if="filteredRoots.length === 0"
              class="rounded-2xl border border-dashed border-[var(--ctp-surface1)] bg-[var(--ctp-canvas)]/60 py-10 text-center text-xs text-[var(--ctp-subtext0)]"
            >
              当前「{{ domainBadgeLabels[domainSubTab] }}」领域尚未添加本地目录
            </div>

            <div
              v-for="r in filteredRoots"
              :key="r.id"
              class="flex items-center justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4"
            >
              <div class="min-w-0">
                <div class="flex items-center gap-2">
                  <span class="truncate text-sm font-bold text-[var(--ctp-text)]">{{ r.name }}</span>
                  <span
                    class="rounded-full bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
                  >
                    {{ domainBadgeLabels[domainSubTab] }}
                  </span>
                </div>
                <p class="mt-1 truncate font-mono text-xs text-[var(--ctp-subtext0)]">{{ r.path }}</p>
              </div>

              <button
                type="button"
                class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-red-500/10 hover:text-red-500"
                title="移除目录白名单（不会删除磁盘原文件）"
                @click="handleRemoveRoot(r.id)"
              >
                <LuTrash2 class="h-4 w-4" />
              </button>
            </div>
          </section>
        </template>

        <!-- ==================== 4. DOMAIN-SCOPED RENAME TEMPLATES ==================== -->
        <template v-else-if="activeTab === 'templates'">
          <section class="card-soft space-y-4 rounded-2xl bg-[var(--ctp-shell)] p-6">
            <div class="flex flex-wrap items-start justify-between gap-3">
              <div>
                <h2 class="text-sm font-bold text-[var(--ctp-text)]">
                  「{{ domainBadgeLabels[domainSubTab] }}」默认重命名归档模板
                </h2>
                <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
                  对齐 {{ domainTokenExamples[domainSubTab].spec }}；支持
                  <code class="font-mono text-[var(--ctp-accent)]">:2</code> 零填充与
                  <code class="font-mono text-[var(--ctp-accent)]">&lt;...&gt;</code> 条件段（变量为空自动省略）
                </p>
              </div>

              <button
                type="button"
                class="inline-flex h-8 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
                @click="resetDomainTemplates(domainSubTab)"
              >
                <LuRotateCcw class="h-3.5 w-3.5" />
                <span>恢复当前类型默认</span>
              </button>
            </div>

            <template v-if="domainSubTab === 'book'">
              <div>
                <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                  图书归档路径模板
                </label>
                <input
                  v-model="templates.book"
                  type="text"
                  class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                />
              </div>
            </template>

            <template v-else-if="domainSubTab === 'comic'">
              <div>
                <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                  漫画归档路径模板
                </label>
                <input
                  v-model="templates.comic"
                  type="text"
                  class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                />
              </div>
            </template>

            <template v-else-if="domainSubTab === 'music'">
              <div>
                <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                  音乐归档路径模板
                </label>
                <input
                  v-model="templates.music"
                  type="text"
                  class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                />
              </div>
            </template>

            <template v-else>
              <div class="space-y-4">
                <div>
                  <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                    电影 (Movie) 归档路径模板
                  </label>
                  <input
                    v-model="templates.video_movie"
                    type="text"
                    class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                  />
                </div>
                <div>
                  <label class="mb-1.5 block text-xs font-semibold text-[var(--ctp-subtext1)]">
                    剧集 / 番剧 (TV Show Episode) 归档路径模板
                  </label>
                  <input
                    v-model="templates.video_episode"
                    type="text"
                    class="h-10 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                  />
                </div>
              </div>
            </template>

            <div class="space-y-2 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-3.5">
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="text-[11px] text-[var(--ctp-subtext0)]">可用占位变量：</span>
                <code
                  v-for="tok in domainTokenExamples[domainSubTab].tokens"
                  :key="tok"
                  class="rounded-md bg-[var(--ctp-mantle)] px-2 py-0.5 font-mono text-[11px] text-[var(--ctp-accent)]"
                >
                  {{ tok }}
                </code>
              </div>
              <p class="font-mono text-[11px] text-[var(--ctp-subtext0)]">
                示例输出：{{ domainTokenExamples[domainSubTab].sample }}
              </p>
            </div>

            <div class="flex justify-end pt-1">
              <button
                type="button"
                :disabled="saving"
                class="inline-flex h-10 items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
                @click="handleSaveSettings"
              >
                <LuSave class="h-4 w-4" />
                <span>{{ saving ? '保存中…' : '保存模板设置' }}</span>
              </button>
            </div>
          </section>
        </template>
      </div>
    </div>
  </div>
</template>
