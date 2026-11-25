import {
  computed,
  onScopeDispose,
  reactive,
  ref,
  toValue,
  watch,
  type ComputedRef,
  type MaybeRefOrGetter,
  type Ref,
} from 'vue'
import { useI18n } from 'vue-i18n'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'
import { onlineItemsApi } from './onlineItemsApi'
import {
  isAvailabilityBlocked,
  playPositionToOnlineProgress,
  sanitizeCoverUrlForApi,
  toOnlineItemSnapshot,
  type OnlineItem,
  type OnlineItemAvailability,
  type OnlineItemMetadataInput,
  type OnlineItemRefInput,
  type OnlineItemSnapshot,
  type OnlineItemSourceKind,
  type PlayPosition,
  type TrackedItemRef,
  type UpsertOnlineItemInput,
} from './types'

const DEFAULT_PROGRESS_DEBOUNCE_MS = 3000

/**
 * Shared reactive cache of persisted online/remote item snapshots keyed by natural key.
 * - `OnlineItemSnapshot`: persisted on backend
 * - `null`: looked up and confirmed not persisted yet
 * - absent (`!has(key)`): not yet queried
 */
const snapshotCache = reactive(new Map<string, OnlineItemSnapshot | null>())

export function buildOnlineItemKey(
  domain: MediaKind,
  sourceKind: OnlineItemSourceKind,
  sourceId: string,
  itemKey: string,
): string {
  return `${domain}:${sourceKind}:${sourceId.trim()}:${itemKey.trim()}`
}

export function buildRefNaturalKey(itemRef: TrackedItemRef): string {
  return buildOnlineItemKey(itemRef.domain, itemRef.scope, itemRef.sourceId, itemRef.ref)
}

export function clearOnlineItemStateCache(): void {
  snapshotCache.clear()
}

export function seedOnlineItemStateCache(items: readonly OnlineItem[]): void {
  for (const dto of items) {
    const snap = toOnlineItemSnapshot(dto)
    const key = buildOnlineItemKey(dto.media_kind, dto.source_kind, dto.source_id, dto.item_key)
    snapshotCache.set(key, snap)
  }
}

function buildUpsertPayload(
  itemRef: TrackedItemRef,
  meta: OnlineItemMetadataInput | null | undefined,
  existing: OnlineItemSnapshot | null,
  overrides: { favorite?: boolean; progress?: PlayPosition | null },
): UpsertOnlineItemInput {
  const title =
    meta?.title?.trim() ||
    existing?.title?.trim() ||
    itemRef.hints?.title?.trim() ||
    itemRef.ref

  const rawCover =
    meta?.coverUrl !== undefined
      ? meta.coverUrl
      : (existing?.coverUrl ?? itemRef.hints?.cover ?? null)

  const extra =
    meta?.extra && typeof meta.extra === 'object' && !Array.isArray(meta.extra)
      ? meta.extra
      : (existing?.extra ?? null)

  return {
    source_kind: itemRef.scope,
    source_id: itemRef.sourceId,
    item_key: itemRef.ref,
    title,
    subtitle: meta?.subtitle ?? existing?.subtitle ?? itemRef.hints?.author ?? null,
    cover_url: sanitizeCoverUrlForApi(rawCover),
    intro: meta?.intro ?? existing?.intro ?? null,
    kind: meta?.kind ?? existing?.kind ?? null,
    extra,
    favorite: overrides.favorite,
    progress:
      overrides.progress === undefined
        ? undefined
        : overrides.progress === null
          ? null
          : playPositionToOnlineProgress(overrides.progress),
  }
}

function buildOptimisticSnapshot(
  itemRef: TrackedItemRef,
  meta: OnlineItemMetadataInput | null | undefined,
  existing: OnlineItemSnapshot | null,
  patch: { favorite?: boolean; progress?: PlayPosition | null },
): OnlineItemSnapshot {
  const nowIso = new Date().toISOString()
  const availability: OnlineItemAvailability =
    existing?.availability ?? (itemRef.scope === 'remote' ? 'unverified' : 'ok')
  const nextFavorite = patch.favorite ?? existing?.favorite ?? false
  const nextProgress = patch.progress !== undefined ? patch.progress : (existing?.progress ?? null)

  let nextRatio = existing?.progressRatio ?? null
  if (patch.progress !== undefined) {
    if (patch.progress === null) {
      nextRatio = null
    } else if (patch.progress.completed) {
      nextRatio = 1
    } else if (patch.progress.duration && patch.progress.duration > 0) {
      const inUnit = Math.min(1, Math.max(0, patch.progress.position / patch.progress.duration))
      if (
        patch.progress.unitIndex !== undefined &&
        patch.progress.totalUnits &&
        patch.progress.totalUnits > 0
      ) {
        nextRatio = Math.min(
          1,
          Math.max(0, (patch.progress.unitIndex + inUnit) / patch.progress.totalUnits),
        )
      } else {
        nextRatio = inUnit
      }
    }
  }

  return {
    id: existing?.id ?? '',
    mediaKind: itemRef.domain,
    sourceKind: itemRef.scope,
    sourceId: itemRef.sourceId,
    sourceName: existing?.sourceName ?? '',
    availability,
    blocked: isAvailabilityBlocked(availability),
    itemKey: itemRef.ref,
    title: meta?.title?.trim() || existing?.title || itemRef.hints?.title || itemRef.ref,
    subtitle: meta?.subtitle ?? existing?.subtitle ?? itemRef.hints?.author,
    coverUrl: meta?.coverUrl ?? existing?.coverUrl ?? itemRef.hints?.cover,
    intro: meta?.intro ?? existing?.intro,
    kind: meta?.kind ?? existing?.kind,
    extra: meta?.extra ?? existing?.extra,
    favorite: nextFavorite,
    favoritedAt: nextFavorite ? (existing?.favoritedAt ?? nowIso) : undefined,
    progress: nextProgress,
    progressRatio: nextRatio,
    progressAt: nextProgress ? nowIso : undefined,
    createdAt: existing?.createdAt ?? nowIso,
    updatedAt: nowIso,
  }
}

export interface UseOnlineItemStateOptions {
  /** Automatically look up persisted state when ref changes. Default: `true`. */
  autoLoad?: boolean
  /** Debounce interval (ms) for `saveProgress`. Default: `3000`. */
  progressDebounceMs?: number
}

export interface UseOnlineItemStateReturn {
  record: ComputedRef<OnlineItemSnapshot | null>
  favorite: ComputedRef<boolean>
  progress: ComputedRef<PlayPosition | null>
  availability: ComputedRef<OnlineItemAvailability>
  /** `true` only for `source_disabled` or `source_missing` (`ok` & `unverified` are playable). */
  blocked: ComputedRef<boolean>
  /** Convenience inverse of `blocked`. */
  canPlay: ComputedRef<boolean>
  loading: Ref<boolean>
  error: Ref<string | null>
  refresh: (force?: boolean) => Promise<OnlineItemSnapshot | null>
  toggleFavorite: () => Promise<void>
  saveProgress: (pos: PlayPosition, opts?: { immediate?: boolean }) => void
  clearProgress: () => Promise<void>
  remove: () => Promise<void>
  flushPendingProgress: () => Promise<void>
  flushProgressKeepalive: (pos?: PlayPosition) => void
}

export function useOnlineItemState(
  itemRef: MaybeRefOrGetter<TrackedItemRef | null>,
  meta?: MaybeRefOrGetter<OnlineItemMetadataInput | null | undefined>,
  options: UseOnlineItemStateOptions = {},
): UseOnlineItemStateReturn {
  const autoLoad = options.autoLoad ?? true
  const progressDebounceMs = options.progressDebounceMs ?? DEFAULT_PROGRESS_DEBOUNCE_MS
  const snackbar = useSnackbarStore()
  const { t } = useI18n()

  const loading = ref(false)
  const error = ref<string | null>(null)

  const currentRef = computed(() => toValue(itemRef))
  const naturalKey = computed(() => (currentRef.value ? buildRefNaturalKey(currentRef.value) : null))

  const record = computed<OnlineItemSnapshot | null>(() => {
    const key = naturalKey.value
    if (!key) return null
    return snapshotCache.get(key) ?? null
  })

  const favorite = computed(() => Boolean(record.value?.favorite))
  const progress = computed<PlayPosition | null>(() => record.value?.progress ?? null)

  const availability = computed<OnlineItemAvailability>(() => {
    if (record.value) return record.value.availability
    return currentRef.value?.scope === 'remote' ? 'unverified' : 'ok'
  })

  const blocked = computed(() => isAvailabilityBlocked(availability.value))
  const canPlay = computed(() => !blocked.value)

  let lookupSeq = 0
  let progressTimer: ReturnType<typeof setTimeout> | null = null
  let pendingProgress: {
    ref: TrackedItemRef
    meta: OnlineItemMetadataInput | null | undefined
    pos: PlayPosition
  } | null = null

  function clearProgressTimer(): void {
    if (progressTimer !== null) {
      clearTimeout(progressTimer)
      progressTimer = null
    }
  }

  async function refresh(force = false): Promise<OnlineItemSnapshot | null> {
    const target = currentRef.value
    if (!target) return null
    const key = buildRefNaturalKey(target)
    if (!force && snapshotCache.has(key)) {
      return snapshotCache.get(key) ?? null
    }

    const seq = ++lookupSeq
    loading.value = true
    error.value = null
    try {
      const res = await onlineItemsApi.lookup(target.domain, [
        {
          source_kind: target.scope,
          source_id: target.sourceId,
          item_key: target.ref,
        },
      ])
      const found = res.items[0] ? toOnlineItemSnapshot(res.items[0]) : null
      snapshotCache.set(key, found)
      return found
    } catch (err) {
      if (seq === lookupSeq) {
        error.value = err instanceof Error ? err.message : String(err)
      }
      return snapshotCache.get(key) ?? null
    } finally {
      if (seq === lookupSeq) {
        loading.value = false
      }
    }
  }

  async function applyFavoriteState(
    target: TrackedItemRef,
    nextFavorite: boolean,
    showUndoOnRemove: boolean,
  ): Promise<void> {
    const key = buildRefNaturalKey(target)
    const prev = snapshotCache.get(key) ?? null
    const currentMeta = toValue(meta)

    snapshotCache.set(
      key,
      buildOptimisticSnapshot(target, currentMeta, prev, { favorite: nextFavorite }),
    )
    error.value = null

    try {
      const payload = buildUpsertPayload(target, currentMeta, prev, { favorite: nextFavorite })
      const saved = await onlineItemsApi.upsert(target.domain, payload)
      snapshotCache.set(key, toOnlineItemSnapshot(saved))

      if (!nextFavorite && showUndoOnRemove) {
        snackbar.show({
          message: t('common.removedFromFavorites'),
          actionLabel: t('common.undo'),
          onAction: async () => {
            await applyFavoriteState(target, true, false)
          },
        })
      }
    } catch (err) {
      snapshotCache.set(key, prev)
      const msg = err instanceof Error ? err.message : String(err)
      error.value = msg
      snackbar.show({ message: msg, tone: 'error' })
    }
  }

  async function toggleFavorite(): Promise<void> {
    const target = currentRef.value
    if (!target) return
    await applyFavoriteState(target, !favorite.value, true)
  }

  async function commitPendingProgress(
    item: NonNullable<typeof pendingProgress>,
  ): Promise<void> {
    const key = buildRefNaturalKey(item.ref)
    const existing = snapshotCache.get(key) ?? null
    if (existing && isAvailabilityBlocked(existing.availability)) {
      return
    }

    try {
      const progressDto = playPositionToOnlineProgress(item.pos)
      const saved = existing?.id
        ? await onlineItemsApi.setProgress(item.ref.domain, existing.id, progressDto)
        : await onlineItemsApi.upsert(
            item.ref.domain,
            buildUpsertPayload(item.ref, item.meta, existing, { progress: item.pos }),
          )
      snapshotCache.set(key, toOnlineItemSnapshot(saved))
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err)
    }
  }

  async function flushPendingProgress(): Promise<void> {
    clearProgressTimer()
    const item = pendingProgress
    if (!item) return
    pendingProgress = null
    await commitPendingProgress(item)
  }

  function flushProgressKeepalive(overridePos?: PlayPosition): void {
    clearProgressTimer()
    let item = pendingProgress
    pendingProgress = null
    if (overridePos) {
      const target = currentRef.value
      if (target && !blocked.value) {
        item = { ref: target, meta: toValue(meta), pos: overridePos }
      }
    }
    if (!item) return

    const key = buildRefNaturalKey(item.ref)
    const existing = snapshotCache.get(key) ?? null
    if (existing && isAvailabilityBlocked(existing.availability)) return

    const progressDto = playPositionToOnlineProgress(item.pos)
    if (existing?.id) {
      onlineItemsApi.flushProgressKeepalive(item.ref.domain, {
        id: existing.id,
        progress: progressDto,
      })
    } else {
      onlineItemsApi.flushProgressKeepalive(item.ref.domain, {
        upsert: buildUpsertPayload(item.ref, item.meta, existing, { progress: item.pos }),
      })
    }
  }

  function onPageHide(): void {
    flushProgressKeepalive()
  }

  function saveProgress(pos: PlayPosition, opts?: { immediate?: boolean }): void {
    const target = currentRef.value
    if (!target || blocked.value) return
    const key = buildRefNaturalKey(target)
    const currentMeta = toValue(meta)
    const existing = snapshotCache.get(key) ?? null

    snapshotCache.set(key, buildOptimisticSnapshot(target, currentMeta, existing, { progress: pos }))

    pendingProgress = { ref: target, meta: currentMeta, pos }
    clearProgressTimer()

    if (opts?.immediate) {
      void flushPendingProgress()
    } else {
      progressTimer = setTimeout(() => {
        progressTimer = null
        void flushPendingProgress()
      }, progressDebounceMs)
    }
  }

  async function clearProgress(): Promise<void> {
    const target = currentRef.value
    if (!target) return
    clearProgressTimer()
    pendingProgress = null
    const key = buildRefNaturalKey(target)
    const existing = snapshotCache.get(key) ?? null
    if (!existing?.id) return

    snapshotCache.set(
      key,
      buildOptimisticSnapshot(target, toValue(meta), existing, { progress: null }),
    )
    try {
      const updated = await onlineItemsApi.clearProgress(target.domain, existing.id)
      snapshotCache.set(key, toOnlineItemSnapshot(updated))
    } catch (err) {
      snapshotCache.set(key, existing)
      const msg = err instanceof Error ? err.message : String(err)
      error.value = msg
      snackbar.show({ message: msg, tone: 'error' })
    }
  }

  async function remove(): Promise<void> {
    const target = currentRef.value
    if (!target) return
    clearProgressTimer()
    pendingProgress = null
    const key = buildRefNaturalKey(target)
    const existing = snapshotCache.get(key) ?? null
    if (!existing?.id) return

    snapshotCache.set(key, null)
    try {
      await onlineItemsApi.delete(target.domain, existing.id)
    } catch (err) {
      snapshotCache.set(key, existing)
      const msg = err instanceof Error ? err.message : String(err)
      error.value = msg
      snackbar.show({ message: msg, tone: 'error' })
    }
  }

  if (typeof window !== 'undefined') {
    window.addEventListener('pagehide', onPageHide)
  }

  onScopeDispose(() => {
    if (typeof window !== 'undefined') {
      window.removeEventListener('pagehide', onPageHide)
    }
    if (pendingProgress) {
      void flushPendingProgress()
    } else {
      clearProgressTimer()
    }
  })

  if (autoLoad) {
    watch(
      naturalKey,
      (next, prev) => {
        if (prev && prev !== next && pendingProgress) {
          void flushPendingProgress()
        }
        if (next) {
          void refresh(false)
        }
      },
      { immediate: true },
    )
  }

  return {
    record,
    favorite,
    progress,
    availability,
    blocked,
    canPlay,
    loading,
    error,
    refresh,
    toggleFavorite,
    saveProgress,
    clearProgress,
    remove,
    flushPendingProgress,
    flushProgressKeepalive,
  }
}

// ---------------------------------------------------------------------------
// Batch Lookup Composable for Result Grids (§3.2 `useOnlineItemStates`)
// ---------------------------------------------------------------------------

export interface UseOnlineItemStatesReturn {
  getState: (itemRef: TrackedItemRef) => OnlineItemSnapshot | null
  isFavorite: (itemRef: TrackedItemRef) => boolean
  getProgress: (itemRef: TrackedItemRef) => PlayPosition | null
  /** Returns `true` only for `source_disabled` or `source_missing`. */
  isBlocked: (itemRef: TrackedItemRef) => boolean
  loading: Ref<boolean>
  error: Ref<string | null>
  refresh: (force?: boolean) => Promise<void>
}

export function useOnlineItemStates(
  refs: MaybeRefOrGetter<readonly TrackedItemRef[]>,
  options: { autoLoad?: boolean } = {},
): UseOnlineItemStatesReturn {
  const autoLoad = options.autoLoad ?? true
  const loading = ref(false)
  const error = ref<string | null>(null)
  let batchSeq = 0

  async function refresh(force = false): Promise<void> {
    const list = toValue(refs)
    if (list.length === 0) return

    const byDomain = new Map<MediaKind, { keys: Set<string>; inputs: OnlineItemRefInput[] }>()

    for (const itemRef of list) {
      const key = buildRefNaturalKey(itemRef)
      if (!force && snapshotCache.has(key)) continue

      let bucket = byDomain.get(itemRef.domain)
      if (!bucket) {
        bucket = { keys: new Set(), inputs: [] }
        byDomain.set(itemRef.domain, bucket)
      }
      if (!bucket.keys.has(key)) {
        bucket.keys.add(key)
        bucket.inputs.push({
          source_kind: itemRef.scope,
          source_id: itemRef.sourceId,
          item_key: itemRef.ref,
        })
      }
    }

    if (byDomain.size === 0) return

    const seq = ++batchSeq
    loading.value = true
    error.value = null
    try {
      const tasks: Promise<void>[] = []

      for (const [domain, bucket] of byDomain.entries()) {
        tasks.push(
          onlineItemsApi.lookup(domain, bucket.inputs).then((res) => {
            const matched = new Set<string>()
            for (const dto of res.items) {
              const snap = toOnlineItemSnapshot(dto)
              const k = buildOnlineItemKey(
                dto.media_kind,
                dto.source_kind,
                dto.source_id,
                dto.item_key,
              )
              snapshotCache.set(k, snap)
              matched.add(k)
            }
            for (const k of bucket.keys) {
              if (!matched.has(k)) {
                snapshotCache.set(k, null)
              }
            }
          }),
        )
      }

      await Promise.all(tasks)
    } catch (err) {
      if (seq === batchSeq) {
        error.value = err instanceof Error ? err.message : String(err)
      }
    } finally {
      if (seq === batchSeq) {
        loading.value = false
      }
    }
  }

  const refSignature = computed(() =>
    toValue(refs)
      .map((r) => buildRefNaturalKey(r))
      .join('|'),
  )

  if (autoLoad) {
    watch(
      refSignature,
      (sig) => {
        if (sig) {
          void refresh(false)
        }
      },
      { immediate: true },
    )
  }

  function getState(itemRef: TrackedItemRef): OnlineItemSnapshot | null {
    return snapshotCache.get(buildRefNaturalKey(itemRef)) ?? null
  }

  function isFavorite(itemRef: TrackedItemRef): boolean {
    return Boolean(getState(itemRef)?.favorite)
  }

  function getProgress(itemRef: TrackedItemRef): PlayPosition | null {
    return getState(itemRef)?.progress ?? null
  }

  function isBlocked(itemRef: TrackedItemRef): boolean {
    return isAvailabilityBlocked(getState(itemRef)?.availability)
  }

  return {
    getState,
    isFavorite,
    getProgress,
    isBlocked,
    loading,
    error,
    refresh,
  }
}
