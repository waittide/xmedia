<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuArrowDown,
  LuArrowUp,
  LuCheckCircle2,
  LuSave,
  LuSettings,
  LuTrash2,
} from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdChip from '@/components/ui/MdChip.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdSelect from '@/components/ui/MdSelect.vue'
import MdSwitch from '@/components/ui/MdSwitch.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import type { FfmpegStatusDto, MediaKind } from '@/types/api'
import { formatBytes } from '@/utils/format'
import { toolApi } from '../../api'
import { DEFAULT_SCRAPER_PRIORITIES } from '../../providers'
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

const { t } = useI18n()

const scrapeLocaleOptions = [
  { value: 'zh-CN', label: '简体中文 (zh-CN)' },
  { value: 'zh-TW', label: '繁體中文 (zh-TW)' },
  { value: 'ja-JP', label: '日本語 (ja-JP)' },
  { value: 'en-US', label: 'English (en-US)' },
]

const cachePresets = computed(() => [
  { key: 'none', label: t('tool.settings.scrapers.presetNone'), days: 0, hours: 0 },
  { key: '12h', label: t('tool.settings.scrapers.preset12h'), days: 0, hours: 12 },
  { key: '1d', label: t('tool.settings.scrapers.preset1d'), days: 1, hours: 0 },
  { key: '7d', label: t('tool.settings.scrapers.preset7d'), days: 7, hours: 0 },
  { key: '30d', label: t('tool.settings.scrapers.preset30d'), days: 30, hours: 0 },
])

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
  book: [...DEFAULT_SCRAPER_PRIORITIES.book],
  comic: [...DEFAULT_SCRAPER_PRIORITIES.comic],
  music: [...DEFAULT_SCRAPER_PRIORITIES.music],
  video: [...DEFAULT_SCRAPER_PRIORITIES.video],
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
      label: t('tool.settings.scrapers.statusDisabled'),
      cls: 'bg-surface-container-high text-on-surface-variant',
    }
  }
  if (!prov.credentials_ready) {
    return {
      label: prov.requires_api_key
        ? t('tool.settings.scrapers.statusNeedKey')
        : t('tool.settings.scrapers.statusNotReady'),
      cls: 'bg-warning-container text-on-warning-container',
    }
  }
  return {
    label: t('tool.settings.scrapers.statusReady'),
    cls: 'bg-success-container text-on-success-container',
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
    cacheClearedBanner.value = t('tool.settings.scrapers.cacheCleared', {
      count: res.removed_count,
    })
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
      <h2 class="text-title-small text-on-surface">
        {{ t('tool.settings.scrapers.priorityTitle', { domain: t(`tool.domains.${domainSubTab}`) }) }}
      </h2>
      <p class="text-body-small mt-1 text-on-surface-variant">
        {{ t('tool.settings.scrapers.priorityDesc') }}
      </p>
    </div>

    <div class="space-y-2">
      <div
        v-for="(prov, idx) in orderedProvidersForDomain"
        :key="prov.id"
        class="flex items-center justify-between gap-3 rounded-corner-lg bg-surface-container-low px-4 py-3 transition"
        :class="!prov.enabled ? 'opacity-65' : ''"
      >
        <div class="flex min-w-0 flex-1 items-center gap-3">
          <span
            class="text-label-medium flex h-7 w-7 shrink-0 items-center justify-center rounded-corner-full bg-secondary-container font-mono text-on-secondary-container"
          >
            {{ idx + 1 }}
          </span>
          <div class="min-w-0">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-title-small text-on-surface">
                {{ prov.name }}
              </span>
              <span
                class="text-label-small rounded-corner-full px-2 py-0.5"
                :class="providerStatusBadge(prov).cls"
              >
                {{ providerStatusBadge(prov).label }}
              </span>
            </div>
            <p
              class="text-body-small mt-0.5 truncate text-on-surface-variant"
              :title="prov.description"
            >
              {{ prov.description }}
            </p>
          </div>
        </div>

        <!-- Per-provider Controls -->
        <div class="flex shrink-0 items-center gap-1">
          <MdSwitch
            :model-value="prov.enabled"
            :disabled="busyProviderId === prov.id"
            :aria-label="
              prov.enabled
                ? t('tool.settings.scrapers.disableHint')
                : t('tool.settings.scrapers.enableHint')
            "
            @update:model-value="(val) => toggleProviderEnabled(prov, val)"
          />

          <MdIconButton
            size="sm"
            :selected="providerHasCustomConfig(prov.id) ? true : undefined"
            :icon="LuSettings"
            :tooltip="t('tool.settings.scrapers.configureHint')"
            :aria-label="t('tool.settings.scrapers.configureHint')"
            @click="editingProvider = prov"
          />

          <div class="mx-0.5 h-4 w-px bg-outline-variant/70" />

          <MdIconButton
            size="sm"
            :disabled="idx === 0"
            :icon="LuArrowUp"
            :tooltip="t('tool.settings.scrapers.moveUp')"
            :aria-label="t('tool.settings.scrapers.moveUp')"
            @click="moveProviderPriority(domainSubTab, idx, -1)"
          />
          <MdIconButton
            size="sm"
            :disabled="idx === orderedProvidersForDomain.length - 1"
            :icon="LuArrowDown"
            :tooltip="t('tool.settings.scrapers.moveDown')"
            :aria-label="t('tool.settings.scrapers.moveDown')"
            @click="moveProviderPriority(domainSubTab, idx, 1)"
          />
        </div>
      </div>
    </div>
  </section>

  <!-- Metadata Cache Duration -->
  <section class="md-card-filled space-y-4 p-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-title-small text-on-surface">{{ t('tool.settings.scrapers.cacheTitle') }}</h2>
        <p class="text-body-small mt-1 text-on-surface-variant">
          {{ t('tool.settings.scrapers.cacheDesc') }}
        </p>
      </div>

      <div class="flex items-center gap-2">
        <RefreshButton :action="loadCacheStats" />
        <MdButton
          variant="outlined"
          :icon="LuTrash2"
          :loading="clearingCache"
          :disabled="!cacheStats || cacheStats.total_entries === 0"
          @click="handleClearCache"
        >
          {{ t('tool.settings.scrapers.clearCache') }}
        </MdButton>
      </div>
    </div>

    <div
      v-if="cacheClearedBanner"
      class="text-body-small rounded-corner-lg bg-success-container px-3.5 py-2 font-medium text-on-success-container"
    >
      {{ cacheClearedBanner }}
    </div>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <MdTextField
        :model-value="cacheDays"
        type="number"
        min="0"
        max="3650"
        :label="t('tool.settings.scrapers.cacheDays')"
        @update:model-value="cacheDays = Math.max(0, Number($event) || 0)"
      >
        <template #trailing>
          <span class="text-label-small">{{ t('tool.settings.scrapers.daysUnit') }}</span>
        </template>
      </MdTextField>

      <MdTextField
        :model-value="cacheHours"
        type="number"
        min="0"
        max="23"
        :label="t('tool.settings.scrapers.cacheHours')"
        @update:model-value="cacheHours = Math.max(0, Math.min(23, Number($event) || 0))"
      >
        <template #trailing>
          <span class="text-label-small">{{ t('tool.settings.scrapers.hoursUnit') }}</span>
        </template>
      </MdTextField>
    </div>

    <div class="flex flex-wrap items-center justify-between gap-2 pt-1">
      <div class="flex flex-wrap items-center gap-1.5">
        <MdChip
          v-for="preset in cachePresets"
          :key="preset.key"
          variant="filter"
          :label="preset.label"
          :selected="cacheDays === preset.days && cacheHours === preset.hours"
          @click="cacheDays = preset.days; cacheHours = preset.hours"
        />
      </div>

      <div class="text-label-small font-mono text-on-surface-variant">
        {{ computedCacheTtlMs.toLocaleString() }} ms
        <template v-if="cacheStats">
          ·
          {{
            t('tool.settings.scrapers.cacheStatsSummary', {
              valid: cacheStats.valid_entries,
              total: cacheStats.total_entries,
              bytes: formatBytes(cacheStats.total_bytes),
            })
          }}
        </template>
      </div>
    </div>
  </section>

  <!-- Global Defaults: Locale, Proxy & FFprobe Status -->
  <section class="md-card-filled space-y-4 p-6">
    <div>
      <h2 class="text-title-small text-on-surface">{{ t('tool.settings.scrapers.globalTitle') }}</h2>
      <p class="text-body-small mt-1 text-on-surface-variant">
        {{ t('tool.settings.scrapers.globalDesc') }}
      </p>
    </div>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <div class="flex flex-col gap-1">
        <span class="text-label-medium text-on-surface-variant">
          {{ t('tool.settings.scrapers.defaultLocale') }}
        </span>
        <MdSelect v-model="scrapeLocale" :options="scrapeLocaleOptions" />
      </div>

      <MdTextField
        v-model="proxyInput"
        :label="t('tool.settings.scrapers.globalProxy')"
        :placeholder="t('tool.settings.scrapers.globalProxyPlaceholder')"
      />
    </div>

    <div
      v-if="ffmpegStatus"
      class="flex flex-wrap items-center justify-between gap-3 rounded-corner-lg bg-surface-container-low px-4 py-3"
    >
      <div>
        <div class="text-title-small text-on-surface">
          {{ t('tool.settings.scrapers.ffmpegTitle') }}
        </div>
        <div class="text-body-small mt-0.5 text-on-surface-variant">
          {{ ffmpegStatus.ffmpeg_version || t('tool.settings.scrapers.ffmpegFallbackDesc') }}
        </div>
      </div>
      <span
        v-if="ffmpegStatus.ffprobe_available"
        class="text-label-medium inline-flex items-center gap-1 rounded-corner-full bg-success-container px-2.5 py-1 text-on-success-container"
      >
        <LuCheckCircle2 class="h-3.5 w-3.5" aria-hidden="true" />
        <span>{{ t('tool.settings.scrapers.statusReady') }}</span>
      </span>
      <span
        v-else
        class="text-label-medium inline-flex items-center gap-1 rounded-corner-full bg-warning-container px-2.5 py-1 text-on-warning-container"
      >
        <LuAlertCircle class="h-3.5 w-3.5" aria-hidden="true" />
        <span>{{ t('tool.settings.scrapers.noFfprobe') }}</span>
      </span>
    </div>

    <div class="flex justify-end pt-1">
      <MdButton variant="filled" :icon="LuSave" :loading="saving" @click="handleSaveGlobalSettings">
        {{ saving ? t('tool.common.saving') : t('tool.settings.scrapers.saveGlobal') }}
      </MdButton>
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
