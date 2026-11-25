import { computed, onBeforeUnmount, ref } from 'vue'
import { buildPosterUrl } from '../api'

export function parseCsvList(raw: string): string[] {
  return raw
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
}

export function parseOptionalInt(raw: string): number | null {
  const trimmed = raw.trim()
  if (!trimmed) return null
  const num = Number.parseInt(trimmed, 10)
  return Number.isFinite(num) ? num : null
}

export function usePosterUploadPreview(
  getPosterKey: () => string | null | undefined,
  size = 320,
) {
  const pendingPosterFile = ref<File | null>(null)
  const pendingPosterPreviewUrl = ref<string | null>(null)

  function clearPendingPoster(): void {
    if (pendingPosterPreviewUrl.value) {
      URL.revokeObjectURL(pendingPosterPreviewUrl.value)
      pendingPosterPreviewUrl.value = null
    }
    pendingPosterFile.value = null
  }

  function selectPosterFile(file: File | null | undefined): void {
    if (!file) return
    clearPendingPoster()
    pendingPosterFile.value = file
    pendingPosterPreviewUrl.value = URL.createObjectURL(file)
  }

  const previewPosterUrl = computed(
    () => pendingPosterPreviewUrl.value || buildPosterUrl(getPosterKey(), size),
  )

  onBeforeUnmount(clearPendingPoster)

  return {
    pendingPosterFile,
    previewPosterUrl,
    clearPendingPoster,
    selectPosterFile,
  }
}
