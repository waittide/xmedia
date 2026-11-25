import { postKeepaliveJson, requestJson } from '@/api/client'
import type {
  OnlineItem,
  OnlineItemDomain,
  OnlineItemListDto,
  OnlineItemQuery,
  OnlineItemRefInput,
  OnlineProgress,
  PatchOnlineItemInput,
  UpsertOnlineItemInput,
} from './types'

/** Matches `MAX_LOOKUP_ITEMS` in `crates/xmedia-core/src/application/online_items.rs`. */
export const MAX_ONLINE_LOOKUP_ITEMS = 200

function basePath(domain: OnlineItemDomain): string {
  return `/${domain}/online/items`
}

function buildListQueryString(query?: OnlineItemQuery): string {
  if (!query) return ''
  const sp = new URLSearchParams()
  if (query.view && query.view !== 'all') sp.set('view', query.view)
  if (query.source_kind) sp.set('source_kind', query.source_kind)
  if (query.source_id?.trim()) sp.set('source_id', query.source_id.trim())
  if (query.q?.trim()) sp.set('q', query.q.trim())
  if (query.limit !== undefined && Number.isFinite(query.limit) && query.limit > 0) {
    sp.set('limit', String(Math.floor(query.limit)))
  }
  if (query.offset !== undefined && Number.isFinite(query.offset) && query.offset >= 0) {
    sp.set('offset', String(Math.floor(query.offset)))
  }
  const qs = sp.toString()
  return qs ? `?${qs}` : ''
}

export interface OnlineItemsRequestOptions {
  signal?: AbortSignal
}

export const onlineItemsApi = {
  list: (
    domain: OnlineItemDomain,
    query?: OnlineItemQuery,
    options?: OnlineItemsRequestOptions,
  ): Promise<OnlineItemListDto> =>
    requestJson<OnlineItemListDto>(`${basePath(domain)}${buildListQueryString(query)}`, {
      signal: options?.signal,
    }),

  upsert: (
    domain: OnlineItemDomain,
    input: UpsertOnlineItemInput,
    options?: OnlineItemsRequestOptions,
  ): Promise<OnlineItem> =>
    requestJson<OnlineItem>(basePath(domain), {
      method: 'PUT',
      body: JSON.stringify(input),
      signal: options?.signal,
    }),

  /**
   * Batch lookup by natural keys. Automatically chunks into batches of 200
   * to stay within the backend `MAX_LOOKUP_ITEMS` limit.
   */
  lookup: async (
    domain: OnlineItemDomain,
    items: readonly OnlineItemRefInput[],
    options?: OnlineItemsRequestOptions,
  ): Promise<OnlineItemListDto> => {
    if (items.length === 0) {
      return { items: [], total: 0 }
    }
    if (items.length <= MAX_ONLINE_LOOKUP_ITEMS) {
      return requestJson<OnlineItemListDto>(`${basePath(domain)}/lookup`, {
        method: 'POST',
        body: JSON.stringify({ items }),
        signal: options?.signal,
      })
    }

    const merged: OnlineItem[] = []
    for (let i = 0; i < items.length; i += MAX_ONLINE_LOOKUP_ITEMS) {
      const chunk = items.slice(i, i + MAX_ONLINE_LOOKUP_ITEMS)
      const res = await requestJson<OnlineItemListDto>(`${basePath(domain)}/lookup`, {
        method: 'POST',
        body: JSON.stringify({ items: chunk }),
        signal: options?.signal,
      })
      merged.push(...res.items)
    }
    return { items: merged, total: merged.length }
  },

  get: (
    domain: OnlineItemDomain,
    id: string,
    options?: OnlineItemsRequestOptions,
  ): Promise<OnlineItem> =>
    requestJson<OnlineItem>(`${basePath(domain)}/${encodeURIComponent(id)}`, {
      signal: options?.signal,
    }),

  patch: (
    domain: OnlineItemDomain,
    id: string,
    input: PatchOnlineItemInput,
    options?: OnlineItemsRequestOptions,
  ): Promise<OnlineItem> =>
    requestJson<OnlineItem>(`${basePath(domain)}/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify(input),
      signal: options?.signal,
    }),

  delete: (
    domain: OnlineItemDomain,
    id: string,
    options?: OnlineItemsRequestOptions,
  ): Promise<void> =>
    requestJson<void>(`${basePath(domain)}/${encodeURIComponent(id)}`, {
      method: 'DELETE',
      signal: options?.signal,
    }),

  setProgress: (
    domain: OnlineItemDomain,
    id: string,
    progress: OnlineProgress,
    options?: OnlineItemsRequestOptions,
  ): Promise<OnlineItem> =>
    requestJson<OnlineItem>(`${basePath(domain)}/${encodeURIComponent(id)}/progress`, {
      method: 'PUT',
      body: JSON.stringify(progress),
      signal: options?.signal,
    }),

  clearProgress: (
    domain: OnlineItemDomain,
    id: string,
    options?: OnlineItemsRequestOptions,
  ): Promise<OnlineItem> =>
    requestJson<OnlineItem>(`${basePath(domain)}/${encodeURIComponent(id)}/progress`, {
      method: 'DELETE',
      signal: options?.signal,
    }),

  /**
   * Best-effort keepalive flush for `pagehide` so debounced progress is not lost
   * when navigating away or closing the tab.
   */
  flushProgressKeepalive: (
    domain: OnlineItemDomain,
    payload:
      | { id: string; progress: OnlineProgress }
      | { upsert: UpsertOnlineItemInput },
  ): void => {
    const path =
      'id' in payload
        ? `${basePath(domain)}/${encodeURIComponent(payload.id)}/progress`
        : basePath(domain)
    const body = 'id' in payload ? payload.progress : payload.upsert
    postKeepaliveJson(path, body, 'PUT')
  },
}

export interface DomainOnlineItemsApi {
  list: (query?: OnlineItemQuery, options?: OnlineItemsRequestOptions) => Promise<OnlineItemListDto>
  upsert: (input: UpsertOnlineItemInput, options?: OnlineItemsRequestOptions) => Promise<OnlineItem>
  lookup: (
    items: readonly OnlineItemRefInput[],
    options?: OnlineItemsRequestOptions,
  ) => Promise<OnlineItemListDto>
  get: (id: string, options?: OnlineItemsRequestOptions) => Promise<OnlineItem>
  patch: (
    id: string,
    input: PatchOnlineItemInput,
    options?: OnlineItemsRequestOptions,
  ) => Promise<OnlineItem>
  delete: (id: string, options?: OnlineItemsRequestOptions) => Promise<void>
  setProgress: (
    id: string,
    progress: OnlineProgress,
    options?: OnlineItemsRequestOptions,
  ) => Promise<OnlineItem>
  clearProgress: (id: string, options?: OnlineItemsRequestOptions) => Promise<OnlineItem>
}

export function createDomainOnlineItemsApi(domain: OnlineItemDomain): DomainOnlineItemsApi {
  return {
    list: (query, options) => onlineItemsApi.list(domain, query, options),
    upsert: (input, options) => onlineItemsApi.upsert(domain, input, options),
    lookup: (items, options) => onlineItemsApi.lookup(domain, items, options),
    get: (id, options) => onlineItemsApi.get(domain, id, options),
    patch: (id, input, options) => onlineItemsApi.patch(domain, id, input, options),
    delete: (id, options) => onlineItemsApi.delete(domain, id, options),
    setProgress: (id, progress, options) => onlineItemsApi.setProgress(domain, id, progress, options),
    clearProgress: (id, options) => onlineItemsApi.clearProgress(domain, id, options),
  }
}
