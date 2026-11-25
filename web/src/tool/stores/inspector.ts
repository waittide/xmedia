import { defineStore } from 'pinia'
import { reactive, ref } from 'vue'
import type { MediaFileDetail } from '../types'

export interface InspectorGuardCallbacks {
  isDirty: () => boolean
  confirmDiscard: () => Promise<boolean>
  requestClose: () => Promise<void>
}

export interface WorkbenchGuardCallbacks {
  isDirty: () => boolean
  confirmDiscard: () => Promise<boolean>
  requestPickEntry: (entry: MediaFileDetail['entry']) => Promise<void>
  requestStep: (delta: -1 | 1) => Promise<void>
  requestExit: () => Promise<void>
}

export const useToolInspectorStore = defineStore('toolInspector', () => {
  const inspectingPath = ref<string | null>(null)
  const activeDetail = ref<MediaFileDetail | null>(null)
  const loadingInspector = ref(false)
  const artworkVersion = ref(1)
  const fileArtworkVersions = reactive<Record<string, number>>({})

  let inspectSeq = 0
  let inspectorGuard: InspectorGuardCallbacks | null = null
  let workbenchGuard: WorkbenchGuardCallbacks | null = null

  function nextInspectSeq(): number {
    return ++inspectSeq
  }

  function currentInspectSeq(): number {
    return inspectSeq
  }

  function artworkVersionKey(rootId: string, relPath: string): string {
    return `${rootId}::${relPath}`
  }

  function bumpFileArtworkVersion(rootId: string, relPath: string): void {
    if (!rootId || !relPath) return
    const key = artworkVersionKey(rootId, relPath)
    fileArtworkVersions[key] = (fileArtworkVersions[key] ?? 0) + 1
  }

  function getFileArtworkVersion(rootId: string, relPath: string): number {
    return artworkVersion.value + (fileArtworkVersions[artworkVersionKey(rootId, relPath)] ?? 0)
  }

  function registerInspectorGuard(guard: InspectorGuardCallbacks | null): void {
    inspectorGuard = guard
  }

  function registerWorkbenchGuard(guard: WorkbenchGuardCallbacks | null): void {
    workbenchGuard = guard
  }

  function isInspectorDirty(): boolean {
    return Boolean(inspectorGuard?.isDirty())
  }

  function isWorkbenchDirty(): boolean {
    return Boolean(workbenchGuard?.isDirty())
  }

  async function confirmActiveEditorClean(): Promise<boolean> {
    if (workbenchGuard) {
      return workbenchGuard.confirmDiscard()
    }
    if (inspectorGuard) {
      return inspectorGuard.confirmDiscard()
    }
    return true
  }

  async function requestCloseInspector(): Promise<void> {
    if (inspectorGuard) {
      await inspectorGuard.requestClose()
      return
    }
    clearActiveDetail()
  }

  function getWorkbenchGuard(): WorkbenchGuardCallbacks | null {
    return workbenchGuard
  }

  function clearActiveDetail(): void {
    inspectSeq += 1
    activeDetail.value = null
    inspectingPath.value = null
    loadingInspector.value = false
  }

  return {
    inspectingPath,
    activeDetail,
    loadingInspector,
    artworkVersion,
    fileArtworkVersions,
    nextInspectSeq,
    currentInspectSeq,
    bumpFileArtworkVersion,
    getFileArtworkVersion,
    registerInspectorGuard,
    registerWorkbenchGuard,
    getWorkbenchGuard,
    isInspectorDirty,
    isWorkbenchDirty,
    confirmActiveEditorClean,
    requestCloseInspector,
    clearActiveDetail,
  }
})
