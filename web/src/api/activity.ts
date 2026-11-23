import type { MediaKind } from '@/types/api'
import { requestJson } from './client'

export interface ActivityDayDto {
  date: string
  duration_secs: number
  active_sessions: number
  last_item_title?: string | null
}

export interface ActivitySummaryDto {
  total_secs: number
  today_secs: number
  week_secs: number
  month_secs: number
  active_days: number
  current_streak: number
  longest_streak: number
  avg_active_day_secs: number
  peak_day?: ActivityDayDto | null
  available_years: number[]
}

export interface ActivityHeatmapDto {
  media_kind: MediaKind
  today: string
  start_date: string
  end_date: string
  days: ActivityDayDto[]
  summary: ActivitySummaryDto
  /** Monday = 0 .. Sunday = 6 */
  weekday_totals: [number, number, number, number, number, number, number]
}

export interface RecordActivityInput {
  media_kind: MediaKind
  date: string
  delta_secs: number
  new_session?: boolean
  item_title?: string
}

export function formatLocalDate(date: Date = new Date()): string {
  const y = date.getFullYear()
  const m = String(date.getMonth() + 1).padStart(2, '0')
  const d = String(date.getDate()).padStart(2, '0')
  return `${y}-${m}-${d}`
}

export function formatActivityDuration(totalSecs: number, zh: boolean): string {
  const secs = Math.max(0, Math.floor(totalSecs))
  if (secs === 0) return zh ? '0 分钟' : '0m'
  if (secs < 60) return zh ? `${secs} 秒` : `${secs}s`
  const mins = Math.floor(secs / 60)
  if (mins < 60) return zh ? `${mins} 分钟` : `${mins}m`
  const hours = Math.floor(mins / 60)
  const remMins = mins % 60
  if (remMins === 0) return zh ? `${hours} 小时` : `${hours}h`
  return zh ? `${hours} 小时 ${remMins} 分钟` : `${hours}h ${remMins}m`
}

export const activityApi = {
  recordHeartbeat: (
    input: RecordActivityInput,
    options?: { keepalive?: boolean },
  ): Promise<ActivityDayDto> =>
    requestJson<ActivityDayDto>(
      '/activity/heartbeat',
      {
        method: 'POST',
        body: JSON.stringify(input),
        keepalive: options?.keepalive,
      },
      true,
    ),

  getHeatmap: (
    mediaKind: MediaKind,
    params?: { year?: number | null; today?: string },
  ): Promise<ActivityHeatmapDto> => {
    const sp = new URLSearchParams()
    sp.set('media_kind', mediaKind)
    sp.set('today', params?.today || formatLocalDate())
    if (params?.year !== undefined && params?.year !== null) {
      sp.set('year', String(params.year))
    }
    return requestJson<ActivityHeatmapDto>(`/activity/heatmap?${sp.toString()}`)
  },
}
