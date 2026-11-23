export interface FormatBytesOptions {
  kbDecimals?: number
  mbDecimals?: number
  gbDecimals?: number
}

export function formatBytes(bytes: number, options?: FormatBytesOptions): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '0 B'
  if (bytes < 1024) return `${Math.round(bytes)} B`
  const kbDecimals = options?.kbDecimals ?? 1
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(kbDecimals)} KB`
  const mbDecimals = options?.mbDecimals ?? 1
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(mbDecimals)} MB`
  const gbDecimals = options?.gbDecimals ?? 2
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(gbDecimals)} GB`
}

export function formatTime(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return '0:00'
  const total = Math.floor(secs)
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  if (h > 0) {
    return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  }
  return `${m}:${String(s).padStart(2, '0')}`
}

export function formatDuration(secs: number, emptyFallback = ''): string {
  if (!Number.isFinite(secs) || secs <= 0) return emptyFallback
  return formatTime(secs)
}

export function resolveFontStack(key: string): string {
  if (key === 'serif') {
    return '"Noto Serif SC", "Noto Serif CJK SC", "Source Han Serif SC", "Songti SC", "STSong", "SimSun", Georgia, serif'
  }
  if (key === 'mono') {
    return '"JetBrains Mono", "Sarasa Mono SC", "Cascadia Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace'
  }
  return '"Inter", "Noto Sans SC", "PingFang SC", "Microsoft YaHei", system-ui, sans-serif'
}
