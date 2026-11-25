import type {
  BaseSourceTestReportDto,
  OnlineSourceSummaryDto,
} from '@/components/online-sources/types'

export interface SourceDebugFieldRow {
  label: string
  value: string
  mono?: boolean
  multiline?: boolean
}

export interface DebugToastSink {
  show: (options: { message: string; tone?: 'error' }) => void
}

export async function copyDebugTextToClipboard(
  text: string,
  snackbar: DebugToastSink,
  copiedMessage: string,
  failedMessage: string,
): Promise<void> {
  try {
    await navigator.clipboard.writeText(text)
    snackbar.show({ message: copiedMessage })
  } catch {
    snackbar.show({ message: failedMessage, tone: 'error' })
  }
}

export function formatSourceDebugReport(
  source: Pick<OnlineSourceSummaryDto, 'name' | 'source_url'>,
  report: BaseSourceTestReportDto,
  extraLines: readonly string[] = [],
): string {
  const lines: string[] = [
    `=== ${source.name} (${report.mode}) ===`,
    `URL: ${source.source_url}`,
    `Result: ${report.ok ? 'OK' : 'FAIL'} | Input: ${report.keyword} | Elapsed: ${report.total_elapsed_ms}ms`,
    '',
    '--- Steps ---',
    ...report.steps.map(
      (step, idx) =>
        `${idx + 1}. [${step.ok ? 'OK' : 'FAIL'}] ${step.label} (${step.elapsed_ms}ms) — ${step.summary}${step.error ? `\n   error: ${step.error}` : ''}`,
    ),
  ]

  if (extraLines.length > 0) {
    lines.push(...extraLines)
  }

  if (report.diagnostics.logs.length > 0) {
    lines.push('', '--- Diagnostics ---', ...report.diagnostics.logs)
  }

  return lines.join('\n')
}

export async function copySourceDebugReport(options: {
  source: Pick<OnlineSourceSummaryDto, 'name' | 'source_url'> | null
  report: BaseSourceTestReportDto | null
  snackbar: DebugToastSink
  copiedMessage: string
  failedMessage: string
  extraLines?: readonly string[]
}): Promise<void> {
  if (!options.source || !options.report) return
  const formatted = formatSourceDebugReport(options.source, options.report, options.extraLines)
  await copyDebugTextToClipboard(
    formatted,
    options.snackbar,
    options.copiedMessage,
    options.failedMessage,
  )
}
