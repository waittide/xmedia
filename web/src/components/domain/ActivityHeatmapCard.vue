<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuAward, LuCalendar, LuSparkles } from 'vue-icons-plus/lu'
import { formatActivityDuration, type ActivityDayDto } from '@/api/activity'

interface HeatmapCell {
  date: string
  inRange: boolean
  isToday: boolean
  durationSecs: number
  sessions: number
  lastItemTitle?: string | null
  level: 0 | 1 | 2 | 3 | 4
  weekdayIndex: number // 0 = Mon .. 6 = Sun
  month: number // 1..12
  dayOfMonth: number
}

interface MonthMarker {
  colIndex: number
  label: string
}

const props = withDefaults(
  defineProps<{
    startDate: string
    endDate: string
    today: string
    days: readonly ActivityDayDto[]
    /** Unflushed live seconds on `today` so the current cell updates in real time. */
    liveTodayDeltaSecs?: number
    peakDay?: ActivityDayDto | null
  }>(),
  {
    liveTodayDeltaSecs: 0,
    peakDay: null,
  },
)

const { t, locale } = useI18n()
const isZh = computed(() => String(locale.value).startsWith('zh'))

const scrollWrapRef = ref<HTMLDivElement | null>(null)
const hoveredCell = ref<HeatmapCell | null>(null)
const selectedDate = ref<string>('')
const tooltipPos = ref<{
  x: number
  y: number
  align: 'left' | 'center' | 'right'
  placement: 'top' | 'bottom'
}>({
  x: 0,
  y: 0,
  align: 'center',
  placement: 'top',
})

const WEEKDAY_LABELS_ZH = ['一', '二', '三', '四', '五', '六', '日']
const WEEKDAY_LABELS_EN = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']
const MONTH_LABELS_ZH = [
  '1月',
  '2月',
  '3月',
  '4月',
  '5月',
  '6月',
  '7月',
  '8月',
  '9月',
  '10月',
  '11月',
  '12月',
]
const MONTH_LABELS_EN = [
  'Jan',
  'Feb',
  'Mar',
  'Apr',
  'May',
  'Jun',
  'Jul',
  'Aug',
  'Sep',
  'Oct',
  'Nov',
  'Dec',
]

function parseYmd(raw: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(raw.trim())
  if (!m) return null
  return new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]))
}

function toYmd(d: Date): string {
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

function addDays(d: Date, delta: number): Date {
  const next = new Date(d.getFullYear(), d.getMonth(), d.getDate() + delta)
  return next
}

function mondayZeroWeekday(d: Date): number {
  return (d.getDay() + 6) % 7
}

const dayMap = computed(() => {
  const map = new Map<string, ActivityDayDto>()
  for (const d of props.days) {
    map.set(d.date, d)
  }
  if (props.liveTodayDeltaSecs > 0 && props.today) {
    const existing = map.get(props.today)
    if (existing) {
      map.set(props.today, {
        ...existing,
        duration_secs: existing.duration_secs + props.liveTodayDeltaSecs,
      })
    } else if (props.today >= props.startDate && props.today <= props.endDate) {
      map.set(props.today, {
        date: props.today,
        duration_secs: props.liveTodayDeltaSecs,
        active_sessions: 1,
        last_item_title: null,
      })
    }
  }
  return map
})

const maxDaySecs = computed(() => {
  let peak = 0
  for (const item of dayMap.value.values()) {
    if (item.duration_secs > peak) peak = item.duration_secs
  }
  return peak
})

function computeIntensityLevel(secs: number, peak: number): 0 | 1 | 2 | 3 | 4 {
  if (secs <= 0) return 0
  // Gentle early thresholds when user has <10 min peak so early progress still shows rich gradations
  if (peak < 600) {
    if (secs < 60) return 1
    if (secs < 180) return 2
    if (secs < 360) return 3
    return 4
  }
  const scale = Math.max(peak, 1800)
  const ratio = secs / scale
  if (ratio <= 0.25) return 1
  if (ratio <= 0.5) return 2
  if (ratio <= 0.78) return 3
  return 4
}

const calendarMatrix = computed<{ weeks: HeatmapCell[][]; months: MonthMarker[] }>(() => {
  const start = parseYmd(props.startDate)
  const end = parseYmd(props.endDate)
  if (!start || !end || start > end) {
    return { weeks: [], months: [] }
  }

  // Align grid start to Monday of the week containing `start`, end to Sunday of week containing `end`
  const gridStart = addDays(start, -mondayZeroWeekday(start))
  const gridEnd = addDays(end, 6 - mondayZeroWeekday(end))
  const peak = maxDaySecs.value
  const monthNames = isZh.value ? MONTH_LABELS_ZH : MONTH_LABELS_EN

  const weeks: HeatmapCell[][] = []
  const months: MonthMarker[] = []
  let lastMarkedMonth = -1
  let lastMarkedCol = -10

  let cursor = gridStart
  let colIndex = 0
  while (cursor <= gridEnd) {
    const col: HeatmapCell[] = []
    let monthFoundInCol: number | null = null

    for (let row = 0; row < 7; row += 1) {
      const ymd = toYmd(cursor)
      const inRange = cursor >= start && cursor <= end
      const rec = inRange ? dayMap.value.get(ymd) : undefined
      const durationSecs = rec?.duration_secs ?? 0
      const month = cursor.getMonth() + 1
      const dayOfMonth = cursor.getDate()

      if (inRange && monthFoundInCol === null && (dayOfMonth <= 7 || colIndex === 0)) {
        monthFoundInCol = month
      }

      col.push({
        date: ymd,
        inRange,
        isToday: inRange && ymd === props.today,
        durationSecs,
        sessions: rec?.active_sessions ?? 0,
        lastItemTitle: rec?.last_item_title ?? null,
        level: inRange ? computeIntensityLevel(durationSecs, peak) : 0,
        weekdayIndex: row,
        month,
        dayOfMonth,
      })
      cursor = addDays(cursor, 1)
    }

    if (
      monthFoundInCol !== null &&
      monthFoundInCol !== lastMarkedMonth &&
      colIndex - lastMarkedCol >= 3
    ) {
      months.push({
        colIndex,
        label: monthNames[monthFoundInCol - 1] ?? String(monthFoundInCol),
      })
      lastMarkedMonth = monthFoundInCol
      lastMarkedCol = colIndex
    }

    weeks.push(col)
    colIndex += 1
  }

  return { weeks, months }
})

const selectedCell = computed<HeatmapCell | null>(() => {
  const target = selectedDate.value || props.today
  for (const col of calendarMatrix.value.weeks) {
    for (const cell of col) {
      if (cell.inRange && cell.date === target) return cell
    }
  }
  return null
})

const effectivePeakDay = computed<ActivityDayDto | null>(() => {
  let best: ActivityDayDto | null = props.peakDay ?? null
  for (const item of dayMap.value.values()) {
    if (!best || item.duration_secs > best.duration_secs) {
      best = item
    }
  }
  return best && best.duration_secs > 0 ? best : null
})

function cellBackgroundStyle(level: 0 | 1 | 2 | 3 | 4): Record<string, string> {
  switch (level) {
    case 1:
      return {
        backgroundColor:
          'color-mix(in srgb, var(--md-sys-color-primary) 26%, var(--md-sys-color-surface-container-high))',
      }
    case 2:
      return {
        backgroundColor:
          'color-mix(in srgb, var(--md-sys-color-primary) 50%, var(--md-sys-color-surface-container-high))',
      }
    case 3:
      return {
        backgroundColor:
          'color-mix(in srgb, var(--md-sys-color-primary) 75%, var(--md-sys-color-surface-container-high))',
      }
    case 4:
      return {
        backgroundColor: 'var(--md-sys-color-primary)',
      }
    case 0:
    default:
      return {
        backgroundColor: 'var(--md-sys-color-surface-container-high)',
      }
  }
}

function formatDateHeading(ymd: string, weekdayIdx?: number): string {
  const d = parseYmd(ymd)
  if (!d) return ymd
  const wd = weekdayIdx ?? mondayZeroWeekday(d)
  if (isZh.value) {
    const wdLabel = WEEKDAY_LABELS_ZH[wd] ?? ''
    return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 · 周${wdLabel}`
  }
  const monthLabel = MONTH_LABELS_EN[d.getMonth()] ?? ''
  const wdLabel = WEEKDAY_LABELS_EN[wd] ?? ''
  return `${wdLabel}, ${monthLabel} ${d.getDate()}, ${d.getFullYear()}`
}

function formatShortDate(ymd: string): string {
  const d = parseYmd(ymd)
  if (!d) return ymd
  if (isZh.value) {
    return `${d.getMonth() + 1}月${d.getDate()}日`
  }
  return `${MONTH_LABELS_EN[d.getMonth()] ?? ''} ${d.getDate()}`
}

function formatCellDuration(secs: number): string {
  if (secs <= 0) return t('history.noActivityOnDay')
  return formatActivityDuration(secs, isZh.value)
}

const activeRovingDate = computed<string>(() => {
  if (selectedCell.value?.inRange) return selectedCell.value.date
  for (let w = calendarMatrix.value.weeks.length - 1; w >= 0; w -= 1) {
    const week = calendarMatrix.value.weeks[w]!
    for (let r = week.length - 1; r >= 0; r -= 1) {
      if (week[r]!.inRange) return week[r]!.date
    }
  }
  return ''
})

function handleCellKeydown(colIdx: number, rowIdx: number, ev: KeyboardEvent): void {
  let nextCol = colIdx
  let nextRow = rowIdx
  if (ev.key === 'ArrowRight') nextCol += 1
  else if (ev.key === 'ArrowLeft') nextCol -= 1
  else if (ev.key === 'ArrowDown') nextRow += 1
  else if (ev.key === 'ArrowUp') nextRow -= 1
  else return

  ev.preventDefault()
  const target = calendarMatrix.value.weeks[nextCol]?.[nextRow]
  if (!target || !target.inRange) return
  selectedDate.value = target.date
  const grid = (ev.currentTarget as HTMLElement)?.closest('[role="grid"]')
  const nextBtn = grid?.querySelector<HTMLButtonElement>(`button[data-date="${target.date}"]`)
  nextBtn?.focus()
}

function handleCellEnter(cell: HeatmapCell, ev: Event): void {
  if (!cell.inRange) return
  hoveredCell.value = cell
  const target = ev.currentTarget as HTMLElement | null
  if (!target || typeof window === 'undefined') return

  const cellRect = target.getBoundingClientRect()
  const vw = window.innerWidth
  const margin = 12
  const centerX = cellRect.left + cellRect.width / 2

  let align: 'left' | 'center' | 'right' = 'center'
  let x = centerX
  if (centerX < 130) {
    align = 'left'
    x = Math.max(margin, cellRect.left)
  } else if (vw - centerX < 130) {
    align = 'right'
    x = Math.min(vw - margin, cellRect.right)
  }

  const placement: 'top' | 'bottom' = cellRect.top < 88 ? 'bottom' : 'top'
  const y = placement === 'top' ? cellRect.top - 10 : cellRect.bottom + 10

  tooltipPos.value = { x, y, align, placement }
}

function handleCellLeave(): void {
  hoveredCell.value = null
}

function handleCellSelect(cell: HeatmapCell): void {
  if (!cell.inRange) return
  selectedDate.value = cell.date
}

const tooltipTransformClass = computed(() => {
  const tx =
    tooltipPos.value.align === 'left'
      ? ''
      : tooltipPos.value.align === 'right'
        ? '-translate-x-full'
        : '-translate-x-1/2'
  const ty = tooltipPos.value.placement === 'top' ? '-translate-y-full' : ''
  return `${tx} ${ty}`.trim()
})

async function scrollToLatest(): Promise<void> {
  await nextTick()
  const syncScrollEnd = () => {
    const el = scrollWrapRef.value
    if (el && el.scrollWidth > el.clientWidth) {
      el.scrollLeft = el.scrollWidth
    }
  }
  syncScrollEnd()
  if (typeof requestAnimationFrame === 'function') {
    requestAnimationFrame(syncScrollEnd)
  }
}

onMounted(() => {
  void scrollToLatest()
  window.addEventListener('scroll', handleCellLeave, true)
  window.addEventListener('resize', handleCellLeave)
})

onBeforeUnmount(() => {
  window.removeEventListener('scroll', handleCellLeave, true)
  window.removeEventListener('resize', handleCellLeave)
})

watch(
  () => [props.startDate, props.endDate],
  () => {
    selectedDate.value = ''
    hoveredCell.value = null
    void scrollToLatest()
  },
)

const weekdayRailLabels = computed(() =>
  isZh.value
    ? [
        { row: 0, text: '一' },
        { row: 2, text: '三' },
        { row: 4, text: '五' },
        { row: 6, text: '日' },
      ]
    : [
        { row: 0, text: 'Mon' },
        { row: 2, text: 'Wed' },
        { row: 4, text: 'Fri' },
        { row: 6, text: 'Sun' },
      ],
)

const monthHeaderMap = computed(() => {
  const m = new Map<number, string>()
  for (const item of calendarMatrix.value.months) {
    m.set(item.colIndex, item.label)
  }
  return m
})

const LEGEND_LEVELS: readonly (0 | 1 | 2 | 3 | 4)[] = [0, 1, 2, 3, 4]
</script>

<template>
  <section
    class="relative rounded-corner-xl bg-surface-container-low p-4 sm:p-6"
    :aria-label="t('history.heatmapTitle')"
  >
    <!-- Card Header -->
    <div class="mb-5 flex flex-wrap items-center justify-between gap-3">
      <div class="flex items-center gap-2.5">
        <div
          class="flex h-9 w-9 shrink-0 items-center justify-center rounded-corner-md bg-primary-container text-on-primary-container"
        >
          <LuCalendar class="h-[18px] w-[18px]" aria-hidden="true" />
        </div>
        <div>
          <h2 class="text-title-medium text-on-surface">
            {{ t('history.heatmapTitle') }}
          </h2>
          <p class="text-body-small tabular-nums text-on-surface-variant">
            {{ startDate }} — {{ endDate }}
          </p>
        </div>
      </div>

      <!-- Selected Day Inspector Pill (touch & desktop friendly) -->
      <div
        v-if="selectedCell"
        class="flex flex-wrap items-center gap-2 rounded-corner-full bg-surface-container px-3.5 py-1.5"
      >
        <span
          class="h-2.5 w-2.5 shrink-0 rounded-[3px]"
          :style="cellBackgroundStyle(selectedCell.level)"
          aria-hidden="true"
        />
        <span class="text-label-medium text-on-surface-variant">
          {{ formatDateHeading(selectedCell.date, selectedCell.weekdayIndex) }}
        </span>
        <span class="text-label-medium font-semibold tabular-nums text-primary">
          {{ formatCellDuration(selectedCell.durationSecs) }}
        </span>
        <span
          v-if="selectedCell.lastItemTitle"
          class="text-label-small hidden max-w-44 truncate text-on-surface-variant sm:inline"
        >
          · {{ selectedCell.lastItemTitle }}
        </span>
      </div>
    </div>

    <!-- Scrollable Heatmap Viewport -->
    <div
      ref="scrollWrapRef"
      class="relative overflow-x-auto pb-2"
      @scroll.passive="handleCellLeave"
    >
      <div class="inline-flex min-w-full items-start gap-2.5 pt-1">
        <!-- Left Weekday Labels Rail -->
        <div
          class="sticky left-0 z-10 flex shrink-0 flex-col bg-surface-container-low pr-1 pt-6 select-none"
          aria-hidden="true"
        >
          <div class="grid grid-rows-7 gap-[4px]">
            <span
              v-for="rowIdx in 7"
              :key="`wd-${rowIdx}`"
              class="text-label-small flex h-[13px] items-center justify-end text-on-surface-variant/80 sm:h-[14px]"
            >
              {{ weekdayRailLabels.find((w) => w.row === rowIdx - 1)?.text ?? '' }}
            </span>
          </div>
        </div>

        <!-- Calendar Columns (pr-2 pb-1 reserves room so rightmost selection ring is never clipped) -->
        <div class="flex flex-1 shrink-0 flex-col pr-2 pb-1">
          <!-- Month Header Row -->
          <div class="mb-1.5 flex h-4.5 gap-[4px] select-none" aria-hidden="true">
            <div
              v-for="(_, colIdx) in calendarMatrix.weeks"
              :key="`mh-${colIdx}`"
              class="relative w-[13px] shrink-0 sm:w-[14px]"
            >
              <span
                v-if="monthHeaderMap.has(colIdx)"
                class="text-label-small absolute top-0 whitespace-nowrap text-on-surface-variant"
                :class="colIdx >= calendarMatrix.weeks.length - 2 ? 'right-0' : 'left-0'"
              >
                {{ monthHeaderMap.get(colIdx) }}
              </span>
            </div>
          </div>

          <!-- 7 x N Rounded Heatmap Grid -->
          <div class="flex gap-[4px]" role="grid" :aria-label="t('history.heatmapTitle')">
            <div
              v-for="(week, colIdx) in calendarMatrix.weeks"
              :key="`wk-${colIdx}`"
              class="grid grid-rows-7 gap-[4px]"
              role="rowgroup"
            >
              <button
                v-for="(cell, rowIdx) in week"
                :key="cell.date"
                :data-date="cell.date"
                type="button"
                role="gridcell"
                :disabled="!cell.inRange"
                :tabindex="cell.inRange && cell.date === activeRovingDate ? 0 : -1"
                :aria-label="
                  cell.inRange
                    ? `${formatDateHeading(cell.date, cell.weekdayIndex)}: ${formatCellDuration(cell.durationSecs)}`
                    : undefined
                "
                :aria-selected="cell.inRange && selectedCell?.date === cell.date"
                class="h-[13px] w-[13px] rounded-[4px] transition-transform duration-150 ease-standard focus-visible:z-20 focus-visible:scale-125 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-primary sm:h-[14px] sm:w-[14px]"
                :class="[
                  cell.inRange
                    ? 'cursor-pointer hover:z-20 hover:scale-[1.22]'
                    : 'pointer-events-none opacity-0',
                  cell.isToday
                    ? 'ring-1 ring-primary ring-offset-1 ring-offset-surface-container-low'
                    : '',
                  cell.inRange && selectedCell?.date === cell.date
                    ? 'ring-2 ring-on-surface/75'
                    : '',
                ]"
                :style="cell.inRange ? cellBackgroundStyle(cell.level) : undefined"
                @mouseenter="handleCellEnter(cell, $event)"
                @mouseleave="handleCellLeave"
                @focus="handleCellEnter(cell, $event)"
                @blur="handleCellLeave"
                @click="handleCellSelect(cell)"
                @keydown="handleCellKeydown(colIdx, rowIdx, $event)"
              />
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Footer: Peak Day Highlight + MD3 5-Level Intensity Legend -->
    <div
      class="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-corner-md bg-surface-container px-3.5 py-2.5"
    >
      <div class="flex items-center gap-2 text-body-small text-on-surface-variant">
        <template v-if="effectivePeakDay">
          <span
            class="inline-flex h-6 w-6 items-center justify-center rounded-corner-full bg-tertiary-container text-on-tertiary-container"
          >
            <LuAward class="h-3.5 w-3.5" aria-hidden="true" />
          </span>
          <span>
            {{ t('history.peakDayLabel') }}
            <strong class="font-medium text-on-surface">
              {{ formatShortDate(effectivePeakDay.date) }}
            </strong>
            ·
            <span class="tabular-nums text-primary">
              {{ formatActivityDuration(effectivePeakDay.duration_secs, isZh) }}
            </span>
          </span>
        </template>
        <template v-else>
          <LuSparkles class="h-4 w-4 text-primary" aria-hidden="true" />
          <span>{{ t('history.trackingAutoHint') }}</span>
        </template>
      </div>

      <!-- 5-Step Swatch Scale -->
      <div class="flex items-center gap-1.5 select-none">
        <span class="text-label-small text-on-surface-variant">{{ t('history.legendLess') }}</span>
        <span
          v-for="lv in LEGEND_LEVELS"
          :key="`lg-${lv}`"
          class="h-3 w-3 rounded-[3.5px]"
          :style="cellBackgroundStyle(lv)"
          aria-hidden="true"
        />
        <span class="text-label-small text-on-surface-variant">{{ t('history.legendMore') }}</span>
      </div>
    </div>

    <!-- Floating MD3 Inverse Surface Tooltip (teleported to avoid overflow clipping) -->
    <Teleport to="body">
      <div
        v-if="hoveredCell"
        role="tooltip"
        class="pointer-events-none fixed z-[var(--z-tooltip)] w-max max-w-60 rounded-corner-md bg-inverse-surface px-3 py-2 text-inverse-on-surface shadow-elevation-2"
        :class="tooltipTransformClass"
        :style="{
          left: `${Math.round(tooltipPos.x)}px`,
          top: `${Math.round(tooltipPos.y)}px`,
        }"
      >
        <p class="text-label-small opacity-80">
          {{ formatDateHeading(hoveredCell.date, hoveredCell.weekdayIndex) }}
        </p>
        <p class="text-label-large mt-0.5 tabular-nums">
          {{ formatCellDuration(hoveredCell.durationSecs) }}
          <span v-if="hoveredCell.sessions > 0" class="text-label-small ml-1 opacity-75">
            · {{ t('history.sessionsCount', { count: hoveredCell.sessions }) }}
          </span>
        </p>
        <p
          v-if="hoveredCell.lastItemTitle"
          class="text-label-small mt-0.5 truncate opacity-80"
        >
          {{ hoveredCell.lastItemTitle }}
        </p>
      </div>
    </Teleport>
  </section>
</template>
