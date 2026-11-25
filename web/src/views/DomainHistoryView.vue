<script setup lang="ts">
import { computed, ref, watch, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import {
  LuActivity,
  LuArrowUpRight,
  LuBarChart3,
  LuBookOpen,
  LuCalendarCheck2,
  LuClapperboard,
  LuClock,
  LuCompass,
  LuDisc3,
  LuFlame,
  LuHistory,
  LuLayers,
  LuLibrary,
  LuPlay,
  LuRefreshCw,
} from 'vue-icons-plus/lu'
import {
  activityApi,
  formatActivityDuration,
  formatLocalDate,
  type ActivityHeatmapDto,
} from '@/api/activity'
import ActivityHeatmapCard from '@/components/domain/ActivityHeatmapCard.vue'
import MediaErrorState from '@/components/domain/MediaErrorState.vue'
import {
  MdButton,
  MdChip,
  MdEmptyState,
  MdIconButton,
  MdLinearProgress,
  MdSkeleton,
  MediaCoverImage,
} from '@/components/ui'
import { activityFlushRevision, livePendingSeconds } from '@/composables/useActivityTracker'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import { libraryRoute, useMergedShelf, type MediaItemVM } from '@/sources'
import { useDomainStore } from '@/stores/domain'
import type { MediaKind } from '@/types/api'

defineOptions({ name: 'DomainHistoryView' })

const { t, locale } = useI18n()
const route = useRoute()
const router = useRouter()
const domainStore = useDomainStore()

const isZh = computed(() => String(locale.value).startsWith('zh'))

const domain = computed<MediaKind>(() => {
  const metaDomain = route.meta.domain
  if (
    metaDomain === 'book' ||
    metaDomain === 'comic' ||
    metaDomain === 'music' ||
    metaDomain === 'video'
  ) {
    return metaDomain
  }
  return domainStore.activeDomain
})

const selectedYear = ref<number | null>(null)
const heatmap = ref<ActivityHeatmapDto | null>(null)
const loading = ref(true)
const manualRefreshing = ref(false)
const error = ref<string | null>(null)
let fetchSeq = 0

const showSkeleton = useDelayedLoading(() => loading.value && !heatmap.value)

const recentShelf = useMergedShelf(domain, 'continue', { limit: 6 })

const DOMAIN_ICONS: Record<MediaKind, Component> = {
  book: LuBookOpen,
  comic: LuLayers,
  music: LuDisc3,
  video: LuClapperboard,
}

const domainIcon = computed(() => DOMAIN_ICONS[domain.value])

const pageTitle = computed(() => t(`history.domainTitle.${domain.value}`))

const todayStr = computed(() => heatmap.value?.today || formatLocalDate())

const includesTodayInRange = computed(() => {
  if (!heatmap.value) return true
  return (
    todayStr.value >= heatmap.value.start_date && todayStr.value <= heatmap.value.end_date
  )
})

const liveTodayDelta = computed(() =>
  includesTodayInRange.value ? Math.max(0, Math.floor(livePendingSeconds[domain.value])) : 0,
)

const effectiveTotalSecs = computed(
  () => (heatmap.value?.summary.total_secs ?? 0) + liveTodayDelta.value,
)

const effectiveTodaySecs = computed(
  () =>
    (heatmap.value?.summary.today_secs ?? 0) +
    Math.max(0, Math.floor(livePendingSeconds[domain.value])),
)

const effectiveWeekSecs = computed(
  () =>
    (heatmap.value?.summary.week_secs ?? 0) +
    Math.max(0, Math.floor(livePendingSeconds[domain.value])),
)

const effectiveMonthSecs = computed(
  () =>
    (heatmap.value?.summary.month_secs ?? 0) +
    Math.max(0, Math.floor(livePendingSeconds[domain.value])),
)

const hasTodayInPersisted = computed(() => {
  if (!heatmap.value) return false
  return heatmap.value.days.some((d) => d.date === todayStr.value && d.duration_secs > 0)
})

const effectiveActiveDays = computed(() => {
  const base = heatmap.value?.summary.active_days ?? 0
  if (includesTodayInRange.value && liveTodayDelta.value > 0 && !hasTodayInPersisted.value) {
    return base + 1
  }
  return base
})

const effectiveCurrentStreak = computed(() => {
  const base = heatmap.value?.summary.current_streak ?? 0
  if (livePendingSeconds[domain.value] > 0 && !hasTodayInPersisted.value) {
    return base + 1
  }
  return base
})

const effectiveLongestStreak = computed(() =>
  Math.max(heatmap.value?.summary.longest_streak ?? 0, effectiveCurrentStreak.value),
)

const effectiveAvgActiveSecs = computed(() => {
  if (effectiveActiveDays.value <= 0) return 0
  return Math.floor(effectiveTotalSecs.value / effectiveActiveDays.value)
})

const availableYears = computed<number[]>(() => {
  const currentYear = new Date().getFullYear()
  const years = heatmap.value?.summary.available_years ?? [currentYear]
  return years.length > 0 ? years : [currentYear]
})

const weekdayStats = computed(() => {
  const base = heatmap.value?.weekday_totals ?? [0, 0, 0, 0, 0, 0, 0]
  const totals = [...base]
  if (includesTodayInRange.value && liveTodayDelta.value > 0) {
    const todayIdx = (new Date().getDay() + 6) % 7
    totals[todayIdx] = (totals[todayIdx] ?? 0) + liveTodayDelta.value
  }
  const maxVal = Math.max(0, ...totals)
  const labels = isZh.value
    ? ['周一', '周二', '周三', '周四', '周五', '周六', '周日']
    : ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']
  const shortLabels = isZh.value
    ? ['一', '二', '三', '四', '五', '六', '日']
    : ['M', 'T', 'W', 'T', 'F', 'S', 'S']

  return totals.map((secs, idx) => ({
    idx,
    label: labels[idx] ?? '',
    shortLabel: shortLabels[idx] ?? '',
    secs,
    ratio: maxVal > 0 ? secs / maxVal : 0,
    isPeak: maxVal > 0 && secs === maxVal,
  }))
})

const topWeekday = computed(() => {
  const peak = weekdayStats.value.find((w) => w.isPeak)
  return peak && peak.secs > 0 ? peak : null
})

function formatHeroDuration(secs: number): string {
  return formatActivityDuration(secs, isZh.value)
}

function formatShortBarDuration(secs: number): string {
  if (secs <= 0) return ''
  if (secs < 60) return `${secs}s`
  const mins = Math.round(secs / 60)
  if (mins < 60) return `${mins}m`
  const h = (secs / 3600).toFixed(secs >= 36_000 ? 0 : 1).replace(/\.0$/, '')
  return `${h}h`
}

function formatRelativeTime(tsMs: number | undefined): string {
  if (!tsMs || tsMs <= 0) return ''
  const diffSec = Math.max(0, Math.floor((Date.now() - tsMs) / 1000))
  if (diffSec < 60) return t('history.justNow')
  const mins = Math.floor(diffSec / 60)
  if (mins < 60) return t('history.minutesAgo', { count: mins })
  const hours = Math.floor(mins / 60)
  if (hours < 24) return t('history.hoursAgo', { count: hours })
  const days = Math.floor(hours / 24)
  if (days < 30) return t('history.daysAgo', { count: days })
  const d = new Date(tsMs)
  return `${d.getMonth() + 1}/${d.getDate()}`
}

async function fetchHeatmap(silent = false): Promise<void> {
  const seq = ++fetchSeq
  if (!silent && !heatmap.value) {
    loading.value = true
  }
  error.value = null
  try {
    const res = await activityApi.getHeatmap(domain.value, {
      year: selectedYear.value,
      today: formatLocalDate(),
    })
    if (seq !== fetchSeq) return
    heatmap.value = res
  } catch (err) {
    if (seq !== fetchSeq) return
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === fetchSeq) {
      loading.value = false
    }
  }
}

async function handleManualRefresh(): Promise<void> {
  if (manualRefreshing.value) return
  manualRefreshing.value = true
  try {
    await Promise.allSettled([fetchHeatmap(true), recentShelf.refresh()])
  } finally {
    manualRefreshing.value = false
  }
}

function selectPeriod(year: number | null): void {
  if (selectedYear.value === year) return
  selectedYear.value = year
  void fetchHeatmap(true)
}

function openRecentItem(item: MediaItemVM): void {
  const target = item.playTo ?? item.to
  if (target) {
    void router.push(target)
  }
}

function itemAspectClass(): string {
  if (domain.value === 'video') return 'aspect-video w-20'
  if (domain.value === 'music') return 'aspect-square w-12'
  return 'aspect-[2/3] w-10'
}

watch(
  domain,
  () => {
    selectedYear.value = null
    heatmap.value = null
    void fetchHeatmap(false)
  },
  { immediate: true },
)

watch(activityFlushRevision, () => {
  void fetchHeatmap(true)
})
</script>

<template>
  <div class="px-4 py-5 sm:px-6 sm:py-6">
    <div class="mx-auto w-full max-w-7xl space-y-6">
      <!-- Page Header + Period Filter Chips -->
      <header class="flex flex-wrap items-center justify-between gap-4">
        <div class="flex min-w-0 items-center gap-3">
          <div
            class="flex h-11 w-11 shrink-0 items-center justify-center rounded-corner-lg bg-primary-container text-on-primary-container"
          >
            <component :is="domainIcon" class="h-5 w-5" aria-hidden="true" />
          </div>
          <div class="min-w-0">
            <h1 class="text-headline-small truncate text-on-surface">
              {{ pageTitle }}
            </h1>
            <p class="text-body-small truncate text-on-surface-variant">
              {{
                selectedYear === null
                  ? t('history.subtitleRolling', {
                      duration: formatHeroDuration(effectiveTotalSecs),
                    })
                  : t('history.subtitleYear', {
                      year: selectedYear,
                      duration: formatHeroDuration(effectiveTotalSecs),
                    })
              }}
            </p>
          </div>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <div
            role="group"
            :aria-label="t('history.periodSelector')"
            class="flex flex-wrap items-center gap-1.5"
          >
            <MdChip
              variant="filter"
              :label="t('history.periodRolling')"
              :selected="selectedYear === null"
              hide-check
              @click="selectPeriod(null)"
            />
            <MdChip
              v-for="yr in availableYears"
              :key="yr"
              variant="filter"
              :label="String(yr)"
              :selected="selectedYear === yr"
              hide-check
              @click="selectPeriod(yr)"
            />
          </div>

          <MdIconButton
            variant="standard"
            size="sm"
            :icon="LuRefreshCw"
            :loading="manualRefreshing"
            :aria-label="t('common.refresh')"
            :tooltip="t('common.refresh')"
            @click="handleManualRefresh"
          />
        </div>
      </header>

      <!-- Error State -->
      <MediaErrorState
        v-if="error && !heatmap"
        :message="error"
        :retrying="loading"
        @retry="fetchHeatmap(false)"
      />

      <!-- Delayed Initial Skeleton -->
      <div v-else-if="loading && !heatmap" class="space-y-6" aria-busy="true">
        <template v-if="showSkeleton">
          <div class="grid grid-cols-2 gap-3.5 lg:grid-cols-4">
            <MdSkeleton v-for="i in 4" :key="`kpi-sk-${i}`" variant="rect" height="6.75rem" class="rounded-corner-xl" />
          </div>
          <MdSkeleton variant="rect" height="14.5rem" class="rounded-corner-xl" />
          <div class="grid grid-cols-1 gap-4 lg:grid-cols-12">
            <MdSkeleton variant="rect" height="14rem" class="rounded-corner-xl lg:col-span-5" />
            <MdSkeleton variant="rect" height="14rem" class="rounded-corner-xl lg:col-span-7" />
          </div>
        </template>
      </div>

      <!-- Loaded Content -->
      <template v-else-if="heatmap">
        <!-- 4-Card MD3 Expressive Bento Metric Strip -->
        <section
          class="grid grid-cols-2 gap-3.5 lg:grid-cols-4"
          :aria-label="t('history.summaryTitle')"
        >
          <!-- Hero Primary Tonal Card: Total Time -->
          <div
            class="flex flex-col justify-between gap-3 rounded-corner-xl bg-primary-container/55 p-4 sm:p-5"
          >
            <div class="flex items-center justify-between gap-2">
              <span class="text-label-large text-on-primary-container/85">
                {{
                  selectedYear === null
                    ? t('history.kpiTotalRolling')
                    : t('history.kpiTotalYear', { year: selectedYear })
                }}
              </span>
              <span
                class="flex h-8 w-8 items-center justify-center rounded-corner-md bg-primary text-on-primary"
              >
                <LuClock class="h-4 w-4" aria-hidden="true" />
              </span>
            </div>
            <div>
              <p class="text-headline-small sm:text-headline-medium font-medium tabular-nums text-on-primary-container">
                {{ formatHeroDuration(effectiveTotalSecs) }}
              </p>
              <p class="text-body-small mt-1 truncate tabular-nums text-on-primary-container/75">
                {{
                  t('history.kpiWeekMonthSub', {
                    week: formatHeroDuration(effectiveWeekSecs),
                    month: formatHeroDuration(effectiveMonthSecs),
                  })
                }}
              </p>
            </div>
          </div>

          <!-- Today Live Card -->
          <div
            class="flex flex-col justify-between gap-3 rounded-corner-xl bg-surface-container-low p-4 sm:p-5"
          >
            <div class="flex items-center justify-between gap-2">
              <span class="text-label-large text-on-surface-variant">
                {{ t('history.kpiToday') }}
              </span>
              <span
                class="flex h-8 w-8 items-center justify-center rounded-corner-md bg-secondary-container text-on-secondary-container"
              >
                <LuActivity class="h-4 w-4" aria-hidden="true" />
              </span>
            </div>
            <div>
              <p class="text-headline-small sm:text-headline-medium font-medium tabular-nums text-on-surface">
                {{ formatHeroDuration(effectiveTodaySecs) }}
              </p>
              <p class="text-body-small mt-1 flex items-center gap-1.5 text-on-surface-variant">
                <span
                  class="inline-block h-2 w-2 rounded-corner-full bg-primary"
                  aria-hidden="true"
                />
                <span class="truncate">{{ t('history.liveTrackingLabel') }}</span>
              </p>
            </div>
          </div>

          <!-- Streak Card -->
          <div
            class="flex flex-col justify-between gap-3 rounded-corner-xl bg-surface-container-low p-4 sm:p-5"
          >
            <div class="flex items-center justify-between gap-2">
              <span class="text-label-large text-on-surface-variant">
                {{ t('history.kpiStreak') }}
              </span>
              <span
                class="flex h-8 w-8 items-center justify-center rounded-corner-md bg-tertiary-container text-on-tertiary-container"
              >
                <LuFlame class="h-4 w-4" aria-hidden="true" />
              </span>
            </div>
            <div>
              <p class="text-headline-small sm:text-headline-medium font-medium tabular-nums text-on-surface">
                {{ t('history.daysUnit', { count: effectiveCurrentStreak }) }}
              </p>
              <p class="text-body-small mt-1 truncate tabular-nums text-on-surface-variant">
                {{ t('history.kpiLongestStreakSub', { count: effectiveLongestStreak }) }}
              </p>
            </div>
          </div>

          <!-- Active Days Card -->
          <div
            class="flex flex-col justify-between gap-3 rounded-corner-xl bg-surface-container-low p-4 sm:p-5"
          >
            <div class="flex items-center justify-between gap-2">
              <span class="text-label-large text-on-surface-variant">
                {{ t('history.kpiActiveDays') }}
              </span>
              <span
                class="flex h-8 w-8 items-center justify-center rounded-corner-md bg-surface-container-high text-primary"
              >
                <LuCalendarCheck2 class="h-4 w-4" aria-hidden="true" />
              </span>
            </div>
            <div>
              <p class="text-headline-small sm:text-headline-medium font-medium tabular-nums text-on-surface">
                {{ t('history.daysUnit', { count: effectiveActiveDays }) }}
              </p>
              <p class="text-body-small mt-1 truncate tabular-nums text-on-surface-variant">
                {{
                  t('history.kpiAvgActiveDaySub', {
                    duration: formatHeroDuration(effectiveAvgActiveSecs),
                  })
                }}
              </p>
            </div>
          </div>
        </section>

        <!-- Primary 52-Week Dynamic MD3 Heatmap -->
        <ActivityHeatmapCard
          :start-date="heatmap.start_date"
          :end-date="heatmap.end_date"
          :today="heatmap.today"
          :days="heatmap.days"
          :live-today-delta-secs="liveTodayDelta"
          :peak-day="heatmap.summary.peak_day"
        />

        <!-- Secondary Insights Row: Weekly Rhythm + Recent Items -->
        <div class="grid grid-cols-1 gap-4 lg:grid-cols-12">
          <!-- Weekly Rhythm Capsule Bar Chart -->
          <section
            class="flex flex-col justify-between rounded-corner-xl bg-surface-container-low p-5 lg:col-span-5"
            :aria-label="t('history.weeklyRhythmTitle')"
          >
            <div class="mb-5 flex items-center justify-between gap-3">
              <div class="flex items-center gap-2.5">
                <div
                  class="flex h-9 w-9 shrink-0 items-center justify-center rounded-corner-md bg-secondary-container text-on-secondary-container"
                >
                  <LuBarChart3 class="h-[18px] w-[18px]" aria-hidden="true" />
                </div>
                <div>
                  <h2 class="text-title-medium text-on-surface">
                    {{ t('history.weeklyRhythmTitle') }}
                  </h2>
                  <p class="text-body-small text-on-surface-variant">
                    {{ t('history.weeklyRhythmDesc') }}
                  </p>
                </div>
              </div>

              <span
                v-if="topWeekday"
                class="text-label-small shrink-0 rounded-corner-full bg-primary-container px-2.5 py-1 text-on-primary-container"
              >
                {{ t('history.mostActiveWeekday', { day: topWeekday.label }) }}
              </span>
            </div>

            <!-- 7 Vertical MD3 Pill Bars -->
            <div class="grid h-44 grid-cols-7 items-end gap-2.5 pt-4 sm:gap-3">
              <div
                v-for="bar in weekdayStats"
                :key="bar.idx"
                class="group flex h-full flex-col items-center justify-end gap-2"
                :title="`${bar.label}: ${formatHeroDuration(bar.secs)}`"
              >
                <span
                  class="text-label-small tabular-nums transition-opacity duration-150"
                  :class="
                    bar.isPeak
                      ? 'font-semibold text-primary'
                      : bar.secs > 0
                        ? 'text-on-surface-variant'
                        : 'opacity-0'
                  "
                >
                  {{ formatShortBarDuration(bar.secs) || '·' }}
                </span>

                <div
                  class="flex w-full max-w-9 flex-1 items-end rounded-corner-full bg-surface-container p-1"
                >
                  <div
                    class="w-full rounded-corner-full transition-all duration-300 ease-emphasized-decel"
                    :class="
                      bar.isPeak
                        ? 'bg-primary'
                        : bar.secs > 0
                          ? 'bg-primary/45 group-hover:bg-primary/70'
                          : 'bg-surface-container-highest'
                    "
                    :style="{
                      height: bar.secs > 0 ? `${Math.max(14, Math.round(bar.ratio * 100))}%` : '10%',
                    }"
                  />
                </div>

                <span
                  class="text-label-medium select-none"
                  :class="bar.isPeak ? 'font-semibold text-primary' : 'text-on-surface-variant'"
                >
                  {{ bar.shortLabel }}
                </span>
              </div>
            </div>
          </section>

          <!-- Recent Domain Activity / Quick Resume -->
          <section
            class="flex flex-col rounded-corner-xl bg-surface-container-low p-5 lg:col-span-7"
            :aria-label="t('history.recentItemsTitle')"
          >
            <div class="mb-4 flex items-center justify-between gap-3">
              <div class="flex items-center gap-2.5">
                <div
                  class="flex h-9 w-9 shrink-0 items-center justify-center rounded-corner-md bg-tertiary-container text-on-tertiary-container"
                >
                  <LuHistory class="h-[18px] w-[18px]" aria-hidden="true" />
                </div>
                <div>
                  <h2 class="text-title-medium text-on-surface">
                    {{ t('history.recentItemsTitle') }}
                  </h2>
                  <p class="text-body-small text-on-surface-variant">
                    {{ t('history.recentItemsDesc') }}
                  </p>
                </div>
              </div>

              <MdButton
                variant="text"
                :to="{ path: `/${domain}/library`, query: { tab: 'continue' } }"
              >
                <span>{{ t('history.viewAllRecent') }}</span>
                <template #trailing>
                  <LuArrowUpRight class="h-4 w-4" aria-hidden="true" />
                </template>
              </MdButton>
            </div>

            <div
              v-if="recentShelf.items.value.length > 0"
              class="flex flex-col gap-2"
            >
              <button
                v-for="(item, idx) in recentShelf.items.value"
                :key="`recent-${idx}-${item.title}`"
                type="button"
                class="md-state group flex w-full cursor-pointer items-center gap-3.5 rounded-corner-lg bg-surface-container/65 px-3 py-2.5 text-left transition-colors duration-150 ease-standard hover:bg-surface-container"
                @click="openRecentItem(item)"
              >
                <div class="shrink-0 overflow-hidden rounded-corner-sm" :class="itemAspectClass()">
                  <MediaCoverImage
                    :src="item.cover ?? null"
                    :alt="item.title"
                    :aspect-class="
                      domain === 'video'
                        ? 'aspect-video'
                        : domain === 'music'
                          ? 'aspect-square'
                          : 'aspect-[2/3]'
                    "
                    rounded="rounded-corner-sm"
                    bg-class="bg-surface-container-high"
                    :fallback-icon="domainIcon"
                    fallback-icon-class="h-4 w-4 text-on-surface-variant/60"
                  />
                </div>

                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <p class="text-title-small truncate text-on-surface group-hover:text-primary">
                      {{ item.title }}
                    </p>
                    <span
                      v-if="item.badge?.source"
                      class="text-label-small hidden shrink-0 rounded-corner-full bg-secondary-container px-2 py-0.5 text-on-secondary-container sm:inline"
                    >
                      {{ item.badge.source }}
                    </span>
                  </div>
                  <p v-if="item.subtitle" class="text-body-small mt-0.5 truncate text-on-surface-variant">
                    {{ item.subtitle }}
                  </p>
                  <div
                    v-if="item.badge?.progressPercent !== undefined"
                    class="mt-1.5 flex items-center gap-2"
                  >
                    <MdLinearProgress
                      :value="item.badge.progressPercent"
                      class="max-w-44 flex-1"
                    />
                    <span class="text-label-small tabular-nums text-primary">
                      {{ Math.round(item.badge.progressPercent) }}%
                    </span>
                  </div>
                </div>

                <div class="flex shrink-0 items-center gap-2">
                  <span
                    v-if="item.sortTimestamp"
                    class="text-label-small hidden tabular-nums text-on-surface-variant sm:inline"
                  >
                    {{ formatRelativeTime(item.sortTimestamp) }}
                  </span>
                  <span
                    class="flex h-8 w-8 items-center justify-center rounded-corner-full bg-surface-container-high text-on-surface-variant transition-colors duration-150 group-hover:bg-primary group-hover:text-on-primary"
                  >
                    <LuPlay class="h-3.5 w-3.5" aria-hidden="true" />
                  </span>
                </div>
              </button>
            </div>

            <MdEmptyState
              v-else-if="!recentShelf.loading.value"
              compact
              :icon="domainIcon"
              :title="t('history.emptyRecentTitle')"
              :description="t('history.emptyRecentDesc')"
              class="my-auto py-8"
            >
              <template #action>
                <MdButton variant="tonal" :icon="LuLibrary" :to="`/${domain}/library`">
                  {{ t('nav.library') }}
                </MdButton>
                <MdButton
                  variant="text"
                  :icon="LuCompass"
                  :to="libraryRoute(domain, { scope: 'online' })"
                >
                  {{ t('domainUi.exploreOnlineBtn') }}
                </MdButton>
              </template>
            </MdEmptyState>
          </section>
        </div>
      </template>
    </div>
  </div>
</template>
