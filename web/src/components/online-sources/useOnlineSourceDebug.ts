import { onBeforeUnmount, ref, watch, type Ref } from 'vue'
import {
  extractApiError,
  type BaseSourceTestReportDto,
  type OnlineSourceSummaryDto,
} from './types'

export interface UseOnlineSourceDebugOptions<
  S extends OnlineSourceSummaryDto,
  M extends string,
  R extends BaseSourceTestReportDto<M>,
  Tab extends string,
> {
  open: Ref<boolean>
  source: Ref<S | null>
  defaultMode: M
  defaultTab: Tab
  defaultKeyword: string
  runTest: (sourceId: string, signal: AbortSignal) => Promise<R>
  onReset: (src: S) => void
  onReportReady: (res: R, preferredTab?: Tab) => void
  onTested: () => void
}

export function useOnlineSourceDebug<
  S extends OnlineSourceSummaryDto,
  M extends string,
  R extends BaseSourceTestReportDto<M>,
  Tab extends string,
>(options: UseOnlineSourceDebugOptions<S, M, R, Tab>) {
  const mode = ref<M>(options.defaultMode) as Ref<M>
  const queryInput = ref('')
  const running = ref(false)
  const requestError = ref<string | null>(null)
  const report = ref<R | null>(null) as Ref<R | null>
  const activeTab = ref<Tab>(options.defaultTab) as Ref<Tab>

  let runSeq = 0
  let activeAbortController: AbortController | null = null

  function abortActiveRun(): void {
    runSeq += 1
    if (activeAbortController) {
      activeAbortController.abort()
      activeAbortController = null
    }
    running.value = false
  }

  watch(
    () => [options.open.value, options.source.value?.id] as const,
    ([isOpen, srcId], prev) => {
      if (!isOpen || !options.source.value) {
        abortActiveRun()
        return
      }
      if (!prev || !prev[0] || prev[1] !== srcId) {
        abortActiveRun()
        mode.value = options.defaultMode
        queryInput.value =
          options.source.value.check_keyword?.trim() || options.defaultKeyword
        report.value = null
        requestError.value = null
        activeTab.value = options.defaultTab
        options.onReset(options.source.value)
      }
    },
    { immediate: true },
  )

  onBeforeUnmount(() => {
    abortActiveRun()
  })

  async function executeTest(preferredTab?: Tab): Promise<void> {
    const src = options.source.value
    if (!src || running.value) return
    const targetSourceId = src.id
    const controller = new AbortController()
    activeAbortController = controller
    const seq = ++runSeq

    running.value = true
    requestError.value = null
    try {
      const res = await options.runTest(targetSourceId, controller.signal)
      if (seq !== runSeq || !options.open.value || options.source.value?.id !== targetSourceId) {
        return
      }
      options.onReportReady(res, preferredTab)
      options.onTested()
    } catch (err) {
      if (
        seq !== runSeq ||
        controller.signal.aborted ||
        (err instanceof DOMException && err.name === 'AbortError')
      ) {
        return
      }
      requestError.value = extractApiError(err)
    } finally {
      if (activeAbortController === controller) {
        activeAbortController = null
      }
      if (seq === runSeq) {
        running.value = false
      }
    }
  }

  return {
    mode,
    queryInput,
    running,
    requestError,
    report,
    activeTab,
    abortActiveRun,
    executeTest,
  }
}
