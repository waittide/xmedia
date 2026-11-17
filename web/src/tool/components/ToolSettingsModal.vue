<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowDown,
  LuArrowUp,
  LuCheck,
  LuFileSignature,
  LuFolderPlus,
  LuKey,
  LuLibrary,
  LuMonitor,
  LuMoon,
  LuPalette,
  LuRefreshCw,
  LuRotateCw,
  LuSave,
  LuSettings,
  LuSlidersHorizontal,
  LuSun,
  LuTrash2,
  LuX,
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
import { useThemeStore } from '@/stores/theme'
import type { LocaleCode, MediaKind } from '@/types/api'
import { toolApi } from '../api'
import type {
  ManagedRootDto,
  ProviderStatusDto,
  PublicSettingsDto,
  RenameTemplatesDto,
  ScraperPrioritiesDto,
} from '../types'

export type ToolSettingsTab = 'appearance' | 'roots' | 'scrapers' | 'templates'

const props = defineProps<{
  open: boolean
  settings: PublicSettingsDto | null
  activeDomain: MediaKind
  initialTab?: ToolSettingsTab
}>()

const emit = defineEmits<{
  close: []
  updated: [settings: PublicSettingsDto]
}>()

const { t } = useI18n()
const themeStore = useThemeStore()

const activeTab = ref<ToolSettingsTab>('appearance')

const newRootName = ref('')
const newRootPath = ref('')
const newRootKind = ref<MediaKind>('book')
const rootFilterKind = ref<'all' | MediaKind>('all')
const addingRoot = ref(false)

const tmdbKeyInput = ref('')
const bangumiTokenInput = ref('')
const proxyInput = ref('')
const scrapeLocale = ref('zh-CN')
const priorityDomainTab = ref<MediaKind>('music')
const allProviders = ref<ProviderStatusDto[]>([])
const priorities = ref<ScraperPrioritiesDto>({
  book: ['bangumi', 'openlibrary'],
  comic: ['bangumi'],
  music: ['itunes', 'musicbrainz', 'lrclib'],
  video: ['tmdb', 'bangumi'],
})

const DEFAULT_RENAME_TEMPLATES: RenameTemplatesDto = {
  book: '{author} - {title}',
  comic: '{series} - #{number:2} {title}',
  music: '{track:2} - {artist} - {title}',
  video_movie: '{title} ({year})/{title} ({year})',
  video_episode: '{show} ({year})/Season {season:2}/{show} - S{season:2}E{episode:2} - {title}',
}

const templates = ref<RenameTemplatesDto>({ ...DEFAULT_RENAME_TEMPLATES })

function resetTemplatesToDefaults(): void {
  templates.value = { ...DEFAULT_RENAME_TEMPLATES }
}

const saving = ref(false)
const savedBanner = ref(false)
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

const rootKindOptions = [
  { value: 'book', label: '图书 (Book)' },
  { value: 'comic', label: '漫画 (Comic)' },
  { value: 'music', label: '音乐 (Music)' },
  { value: 'video', label: '影视 (Video)' },
]

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

const filteredRoots = computed<ManagedRootDto[]>(() => {
  const all = props.settings?.roots ?? []
  if (rootFilterKind.value === 'all') return all
  return all.filter((r) => r.default_kind === rootFilterKind.value)
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

const orderedProvidersForPriorityTab = computed<ProviderStatusDto[]>(() => {
  const dom = priorityDomainTab.value
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

watch(
  () => [props.open, props.settings, props.initialTab, props.activeDomain] as const,
  async ([isOpen, s, tab, dom]) => {
    if (!isOpen) return
    if (tab) activeTab.value = tab
    newRootKind.value = dom
    priorityDomainTab.value = dom
    errorMsg.value = null
    savedBanner.value = false
    tmdbKeyInput.value = ''
    bangumiTokenInput.value = ''
    if (s) {
      proxyInput.value = s.http_proxy ?? ''
      scrapeLocale.value = s.default_scrape_locale || 'zh-CN'
      templates.value = { ...s.rename_templates }
      priorities.value = {
        book: [...s.scraper_priorities.book],
        comic: [...s.scraper_priorities.comic],
        music: [...s.scraper_priorities.music],
        video: [...s.scraper_priorities.video],
      }
    }
    try {
      allProviders.value = await toolApi.listProviders()
    } catch {
      allProviders.value = []
    }
  },
)

async function handleAddRoot(): Promise<void> {
  if (!newRootPath.value.trim() || addingRoot.value) return
  addingRoot.value = true
  errorMsg.value = null
  try {
    await toolApi.addRoot(newRootName.value.trim(), newRootPath.value.trim(), newRootKind.value)
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
      rename_templates: { ...templates.value },
      scraper_priorities: normalizedPrio,
    })
    tmdbKeyInput.value = ''
    bangumiTokenInput.value = ''
    emit('updated', next)
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
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-3 backdrop-blur-xs sm:p-6"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[90vh] w-full max-w-4xl flex-col overflow-hidden rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] shadow-2xl"
      >
        <!-- Header -->
        <div
          class="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--ctp-surface0)]/80 px-5 py-4"
        >
          <div class="flex items-center gap-2">
            <LuSettings class="h-4 w-4 text-[var(--ctp-accent)]" />
            <h2 class="text-sm font-bold text-[var(--ctp-text)]">xmedia-tool 工作台设置</h2>
          </div>
          <button
            type="button"
            class="flex h-8 w-8 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <!-- Tabs -->
        <div class="flex shrink-0 flex-wrap gap-1 border-b border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/40 px-5 py-2.5">
          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded-xl px-3.5 py-1.5 text-xs font-semibold transition"
            :class="
              activeTab === 'appearance'
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60'
            "
            @click="activeTab = 'appearance'"
          >
            <LuPalette class="h-3.5 w-3.5" />
            <span>{{ t('settings.navAppearance') }}</span>
          </button>
          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded-xl px-3.5 py-1.5 text-xs font-semibold transition"
            :class="
              activeTab === 'roots'
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60'
            "
            @click="activeTab = 'roots'"
          >
            <LuLibrary class="h-3.5 w-3.5" />
            <span>分域目录管理</span>
          </button>
          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded-xl px-3.5 py-1.5 text-xs font-semibold transition"
            :class="
              activeTab === 'scrapers'
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60'
            "
            @click="activeTab = 'scrapers'"
          >
            <LuKey class="h-3.5 w-3.5" />
            <span>刮削源密钥与代理</span>
          </button>
          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded-xl px-3.5 py-1.5 text-xs font-semibold transition"
            :class="
              activeTab === 'templates'
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60'
            "
            @click="activeTab = 'templates'"
          >
            <LuFileSignature class="h-3.5 w-3.5" />
            <span>默认重命名模板</span>
          </button>
        </div>

        <!-- Body -->
        <div class="flex-1 space-y-4 overflow-y-auto p-5">
          <div
            v-if="errorMsg"
            class="rounded-xl border border-red-500/35 bg-red-500/10 px-3 py-2 text-xs text-red-400"
          >
            {{ errorMsg }}
          </div>

          <!-- TAB 1: APPEARANCE & LANGUAGE (Aligned with xmedia SettingsView) -->
          <div v-if="activeTab === 'appearance'" class="space-y-4">
            <!-- Language -->
            <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-5">
              <h3 class="text-sm font-bold">{{ t('theme.languageTitle') }}</h3>
              <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.languageDesc') }}</p>

              <div class="mt-3.5 grid grid-cols-1 gap-3 sm:grid-cols-2">
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

            <!-- Theme Mode -->
            <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-5">
              <div class="flex flex-wrap items-start justify-between gap-2">
                <div>
                  <h3 class="text-sm font-bold">{{ t('theme.modeTitle') }}</h3>
                  <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.modeDesc') }}</p>
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

              <div class="mt-3.5 grid grid-cols-3 gap-3">
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

            <!-- Light Palette Presets -->
            <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-5">
              <h3 class="text-sm font-bold">{{ t('theme.lightPaletteTitle') }}</h3>
              <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.lightPaletteDesc') }}</p>

              <div class="mt-3.5 grid grid-cols-1 gap-2.5 sm:grid-cols-3">
                <button
                  v-for="preset in LIGHT_PALETTE_PRESETS"
                  :key="preset.id"
                  type="button"
                  class="flex items-center justify-between rounded-xl border px-3.5 py-2.5 text-left text-xs font-semibold transition"
                  :class="
                    themeStore.lightPreset === preset.id
                      ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                      : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
                  "
                  @click="themeStore.setLightPreset(preset.id)"
                >
                  <div class="flex min-w-0 items-center gap-2">
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
                  </div>
                  <LuCheck v-if="themeStore.lightPreset === preset.id" class="h-4 w-4 shrink-0" />
                </button>

                <button
                  type="button"
                  class="flex items-center justify-between rounded-xl border px-3.5 py-2.5 text-left text-xs font-semibold transition"
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
                class="mt-3.5 grid grid-cols-2 gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-3.5 sm:grid-cols-5"
              >
                <div v-for="f in customTokenFields" :key="`light-${f.key}`">
                  <label class="mb-1 block truncate text-[11px] font-medium text-[var(--ctp-subtext0)]">
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
                      @change="onCustomTokenInput('light', f.key, ($event.target as HTMLInputElement).value, themeStore.customLight[f.key])"
                    />
                  </div>
                </div>
              </div>
            </section>

            <!-- Dark Palette Presets -->
            <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-5">
              <h3 class="text-sm font-bold">{{ t('theme.darkPaletteTitle') }}</h3>
              <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.darkPaletteDesc') }}</p>

              <div class="mt-3.5 grid grid-cols-1 gap-2.5 sm:grid-cols-3">
                <button
                  v-for="preset in DARK_PALETTE_PRESETS"
                  :key="preset.id"
                  type="button"
                  class="flex items-center justify-between rounded-xl border px-3.5 py-2.5 text-left text-xs font-semibold transition"
                  :class="
                    themeStore.darkPreset === preset.id
                      ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                      : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
                  "
                  @click="themeStore.setDarkPreset(preset.id)"
                >
                  <div class="flex min-w-0 items-center gap-2">
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
                  </div>
                  <LuCheck v-if="themeStore.darkPreset === preset.id" class="h-4 w-4 shrink-0" />
                </button>

                <button
                  type="button"
                  class="flex items-center justify-between rounded-xl border px-3.5 py-2.5 text-left text-xs font-semibold transition"
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
                class="mt-3.5 grid grid-cols-2 gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-3.5 sm:grid-cols-5"
              >
                <div v-for="f in customTokenFields" :key="`dark-${f.key}`">
                  <label class="mb-1 block truncate text-[11px] font-medium text-[var(--ctp-subtext0)]">
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
                      @change="onCustomTokenInput('dark', f.key, ($event.target as HTMLInputElement).value, themeStore.customDark[f.key])"
                    />
                  </div>
                </div>
              </div>
            </section>

            <!-- Accent Color -->
            <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-5">
              <h3 class="text-sm font-bold">{{ t('theme.accentTitle') }}</h3>
              <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.accentDesc') }}</p>

              <div class="mt-3.5 grid grid-cols-1 gap-2.5 sm:grid-cols-3">
                <button
                  type="button"
                  class="rounded-xl border px-3.5 py-2 text-xs font-semibold transition"
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
                  class="rounded-xl border px-3.5 py-2 text-xs font-semibold transition"
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
                  class="rounded-xl border px-3.5 py-2 text-xs font-semibold transition"
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
                class="mt-3.5 flex flex-wrap gap-2.5"
              >
                <button
                  v-for="acc in CATPPUCCIN_ACCENTS"
                  :key="acc"
                  type="button"
                  class="flex h-7 w-7 items-center justify-center rounded-full transition hover:scale-110"
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
                class="mt-3.5 flex items-center gap-2.5"
              >
                <AppColorPicker
                  :model-value="themeStore.customAccent"
                  @update:model-value="themeStore.setCustomAccentHex($event)"
                />
                <input
                  type="text"
                  :value="themeStore.customAccent"
                  maxlength="7"
                  class="h-9 w-36 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 font-mono text-xs outline-none focus:border-[var(--ctp-accent)]"
                  @change="onCustomAccentInput(($event.target as HTMLInputElement).value)"
                />
              </div>
            </section>
          </div>

          <!-- TAB 2: DOMAIN-ISOLATED ROOTS -->
          <div v-else-if="activeTab === 'roots'" class="space-y-4">
            <form
              class="space-y-3 rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/40 p-4"
              @submit.prevent="handleAddRoot"
            >
              <div class="flex items-center justify-between gap-2">
                <div class="text-xs font-semibold text-[var(--ctp-text)]">
                  新增专属领域媒体根目录
                </div>
                <span class="text-[11px] text-[var(--ctp-subtext0)]">
                  图书、漫画、音乐、影视目录彼此独立隔离
                </span>
              </div>
              <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-12">
                <div class="sm:col-span-3">
                  <AppSelect v-model="newRootKind" :options="rootKindOptions" />
                </div>
                <input
                  v-model="newRootName"
                  type="text"
                  placeholder="显示别名（可选，默认目录名）"
                  class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-3"
                />
                <input
                  v-model="newRootPath"
                  type="text"
                  placeholder="服务器本地绝对路径，如 /data/media/music"
                  class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)] sm:col-span-6"
                />
              </div>
              <div class="flex justify-end">
                <button
                  type="submit"
                  :disabled="!newRootPath.trim() || addingRoot"
                  class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] transition hover:opacity-95 disabled:opacity-50"
                >
                  <LuFolderPlus class="h-3.5 w-3.5" />
                  <span>添加到「{{ domainBadgeLabels[newRootKind] }}」领域</span>
                </button>
              </div>
            </form>

            <div class="space-y-2.5">
              <div class="flex flex-wrap items-center justify-between gap-2">
                <div class="text-xs font-semibold text-[var(--ctp-subtext0)]">
                  已登记根目录 ({{ filteredRoots.length }})
                </div>
                <div class="flex items-center gap-1 rounded-xl bg-[var(--ctp-mantle)] p-1">
                  <button
                    v-for="f in [
                      { id: 'all' as const, label: '全部' },
                      { id: 'book' as const, label: '图书' },
                      { id: 'comic' as const, label: '漫画' },
                      { id: 'music' as const, label: '音乐' },
                      { id: 'video' as const, label: '影视' },
                    ]"
                    :key="f.id"
                    type="button"
                    class="rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
                    :class="
                      rootFilterKind === f.id
                        ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                        : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                    "
                    @click="rootFilterKind = f.id"
                  >
                    {{ f.label }}
                  </button>
                </div>
              </div>

              <div
                v-if="filteredRoots.length === 0"
                class="rounded-2xl border border-dashed border-[var(--ctp-surface0)] py-8 text-center text-xs text-[var(--ctp-subtext0)]"
              >
                当前筛选下暂无根目录，请在上方添加对应领域的本地文件夹路径
              </div>
              <div
                v-for="r in filteredRoots"
                :key="r.id"
                class="flex items-center justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 px-4 py-3"
              >
                <div class="min-w-0">
                  <div class="flex items-center gap-2">
                    <span
                      class="rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-bold text-[var(--ctp-accent)]"
                    >
                      {{ r.default_kind ? domainBadgeLabels[r.default_kind] : '未绑定领域' }}
                    </span>
                    <span class="text-xs font-bold text-[var(--ctp-text)]">{{ r.name }}</span>
                  </div>
                  <div class="mt-0.5 truncate font-mono text-[11px] text-[var(--ctp-subtext0)]">
                    {{ r.path }}
                  </div>
                </div>
                <button
                  type="button"
                  class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-red-500/15 hover:text-red-400"
                  title="移除白名单（不会删除磁盘文件）"
                  @click="handleRemoveRoot(r.id)"
                >
                  <LuTrash2 class="h-4 w-4" />
                </button>
              </div>
            </div>
          </div>

          <!-- TAB 3: SCRAPERS, PRIORITY ORDER & PROXY -->
          <div v-else-if="activeTab === 'scrapers'" class="space-y-4">
            <!-- Per-Domain Scraper Priority Reordering -->
            <div class="space-y-3 rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/40 p-4">
              <div class="flex flex-wrap items-center justify-between gap-2">
                <div>
                  <div class="text-xs font-bold text-[var(--ctp-text)]">
                    多源互补刮削优先级顺序
                  </div>
                  <p class="mt-0.5 text-[11px] text-[var(--ctp-subtext0)]">
                    排在前面的源优先填充信息；若某字段缺失（如无封面、无歌词、无 ISBN），自动依次从下方源补齐
                  </p>
                </div>

                <div class="flex items-center gap-1 rounded-xl bg-[var(--ctp-canvas)] p-1 border border-[var(--ctp-surface0)]/75">
                  <button
                    v-for="d in (['book', 'comic', 'music', 'video'] as MediaKind[])"
                    :key="`prio-${d}`"
                    type="button"
                    class="rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
                    :class="
                      priorityDomainTab === d
                        ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                        : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
                    "
                    @click="priorityDomainTab = d"
                  >
                    {{ domainBadgeLabels[d] }}
                  </button>
                </div>
              </div>

              <div class="space-y-1.5">
                <div
                  v-for="(prov, idx) in orderedProvidersForPriorityTab"
                  :key="prov.id"
                  class="flex items-center justify-between gap-3 rounded-xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-canvas)] px-3.5 py-2.5"
                >
                  <div class="flex min-w-0 items-center gap-2.5">
                    <span
                      class="flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-[var(--ctp-accent-soft)] font-mono text-[11px] font-bold text-[var(--ctp-accent)]"
                    >
                      {{ idx + 1 }}
                    </span>
                    <div class="min-w-0">
                      <div class="flex items-center gap-2">
                        <span class="text-xs font-semibold text-[var(--ctp-text)]">
                          {{ prov.name }}
                        </span>
                        <span
                          class="rounded-full px-2 py-0.2 text-[10px] font-medium"
                          :class="
                            prov.configured
                              ? 'bg-emerald-500/15 text-emerald-500'
                              : 'bg-amber-500/15 text-amber-500'
                          "
                        >
                          {{ prov.configured ? '已就绪' : '需配置密钥' }}
                        </span>
                      </div>
                      <div class="truncate text-[11px] text-[var(--ctp-subtext0)]">
                        {{ prov.description }}
                      </div>
                    </div>
                  </div>

                  <div class="flex shrink-0 items-center gap-1">
                    <button
                      type="button"
                      :disabled="idx === 0"
                      class="flex h-7 w-7 items-center justify-center rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)] disabled:opacity-35"
                      title="提高优先级"
                      @click="moveProviderPriority(priorityDomainTab, idx, -1)"
                    >
                      <LuArrowUp class="h-3.5 w-3.5" />
                    </button>
                    <button
                      type="button"
                      :disabled="idx === orderedProvidersForPriorityTab.length - 1"
                      class="flex h-7 w-7 items-center justify-center rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)] disabled:opacity-35"
                      title="降低优先级"
                      @click="moveProviderPriority(priorityDomainTab, idx, 1)"
                    >
                      <LuArrowDown class="h-3.5 w-3.5" />
                    </button>
                  </div>
                </div>
              </div>
            </div>

            <div class="space-y-3 rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 p-4">
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
                placeholder="输入新的 TMDB v3 API Key 或 v4 Bearer Token（留空保持原值）"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </div>

            <div class="space-y-3 rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 p-4">
              <div class="flex items-center justify-between">
                <div class="flex items-center gap-2 text-xs font-bold text-[var(--ctp-text)]">
                  <LuKey class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                  <span>Bangumi Access Token（可选，免密钥亦可检索公开条目）</span>
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
                placeholder="输入 Bangumi Personal Access Token（留空保持原值）"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </div>

            <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <label class="block">
                <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">
                  HTTP / SOCKS5 代理地址（用于访问 TMDB / MusicBrainz 等外网源）
                </span>
                <input
                  v-model="proxyInput"
                  type="text"
                  placeholder="如 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080"
                  class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                />
              </label>
              <div class="block">
                <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">
                  默认刮削语言偏好
                </span>
                <AppSelect v-model="scrapeLocale" :options="scrapeLocaleOptions" />
              </div>
            </div>
          </div>

          <!-- TAB 4: RENAME TEMPLATES -->
          <div v-else class="space-y-3.5">
            <div
              class="flex flex-wrap items-start justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/40 p-3.5"
            >
              <div class="space-y-1 text-xs">
                <div class="font-bold text-[var(--ctp-text)]">
                  占位符与 Jellyfin 目录归档语法
                </div>
                <p class="leading-relaxed text-[11px] text-[var(--ctp-subtext0)]">
                  使用 <code class="font-mono text-[var(--ctp-accent)]">{变量}</code> 引用字段，<code class="font-mono text-[var(--ctp-accent)]">{变量:2}</code> 指定数值补零位数（如 <code class="font-mono">01</code>），<code class="font-mono text-[var(--ctp-accent)]">/</code> 表示相对当前根目录建夹归档；包裹为空变量的括号或方括号（如 <code class="font-mono">({year})</code>、<code class="font-mono">[tmdbid-{tmdbid}]</code>）会自动消隐。
                </p>
              </div>
              <button
                type="button"
                class="inline-flex shrink-0 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
                @click="resetTemplatesToDefaults"
              >
                <LuRotateCw class="h-3.5 w-3.5" />
                <span>恢复默认模板</span>
              </button>
            </div>

            <label class="block">
              <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">图书模板 ({author}, {title}, {year}, {isbn}, {publisher})</span>
              <input
                v-model="templates.book"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">漫画模板 ({series}, {number:2}, {volume:2}, {title}, {year})</span>
              <input
                v-model="templates.comic"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">音乐模板 ({track:2}, {disc}, {artist}, {album}, {title}, {year})</span>
              <input
                v-model="templates.music"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">电影 Jellyfin 归档模板 ({title}, {original_title}, {year}, {tmdbid}, {bangumiid}, {jellyfin_id})</span>
              <input
                v-model="templates.video_movie"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
            <label class="block">
              <span class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">剧集 Jellyfin 归档模板 ({show}, {season:2}, {episode:2}, {title}, {year}, {tmdbid}, {bangumiid}, {jellyfin_id})</span>
              <input
                v-model="templates.video_episode"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
              />
            </label>
          </div>
        </div>

        <!-- Footer -->
        <div
          class="flex shrink-0 items-center justify-between gap-3 border-t border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 px-5 py-3.5"
        >
          <span v-if="savedBanner" class="inline-flex items-center gap-1.5 text-xs font-semibold text-emerald-500">
            <LuCheck class="h-4 w-4" />
            设置已保存
          </span>
          <span v-else class="text-[11px] text-[var(--ctp-subtext0)]">
            Apple Music、MusicBrainz、LRCLIB、OpenLibrary 与 Bangumi 默认免密钥即开即用
          </span>

          <div class="flex items-center gap-2">
            <button
              type="button"
              class="rounded-xl px-4 py-2 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/65"
              @click="emit('close')"
            >
              关闭
            </button>
            <button
              v-if="activeTab === 'scrapers' || activeTab === 'templates'"
              type="button"
              :disabled="saving"
              class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
              @click="handleSaveSettings"
            >
              <LuRefreshCw v-if="saving" class="h-3.5 w-3.5 animate-spin" />
              <LuSave v-else class="h-3.5 w-3.5" />
              <span>保存配置</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
