<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch, type Component } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuArrowRight,
  LuBookOpen,
  LuClapperboard,
  LuCornerDownLeft,
  LuDisc3,
  LuLayers,
  LuLoader2,
  LuMusic,
  LuSearch,
  LuSettings,
  LuX,
} from 'vue-icons-plus/lu'
import { requestJson } from '@/api/client'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import type { BookSummaryDto, PagedResult as BookPagedResult } from '@/domains/book/types'
import type { SeriesSummaryDto } from '@/domains/comic/types'
import type { SearchResultDto as MusicSearchResultDto } from '@/domains/music/types'
import type { VideoSummaryDto } from '@/domains/video/types'
import type { MediaKind } from '@/types/api'

const PER_DOMAIN_LIMIT = 5

interface QuickResultItem {
  id: string
  group: MediaKind | 'nav'
  title: string
  subtitle?: string
  to: string
  icon: Component
  domain?: MediaKind
}

interface DomainResultSection {
  domain: MediaKind
  label: string
  icon: Component
  items: QuickResultItem[]
  viewAllTo: { path: string; query: { q: string } }
}

const props = defineProps<{
  open: boolean
}>()

const emit = defineEmits<{
  close: []
}>()

const { t, locale } = useI18n()
const router = useRouter()
const authStore = useAuthStore()
const domainStore = useDomainStore()
const escapeStack = useEscapeStack()

escapeStack.register(
  () => props.open,
  () => emit('close'),
)

const isZh = computed(() => locale.value.toLowerCase().startsWith('zh'))
const viewAllLabel = computed(() => (isZh.value ? '查看全部' : 'View all'))
const quickNavHeading = computed(() => (isZh.value ? '快捷导航' : 'Quick navigation'))

const inputRef = ref<HTMLInputElement | null>(null)
const listRef = ref<HTMLElement | null>(null)
const query = ref('')
const loading = ref(false)
const domainSections = ref<DomainResultSection[]>([])
const activeIndex = ref(0)

let debounceTimer: ReturnType<typeof setTimeout> | null = null
let activeController: AbortController | null = null
let searchSeq = 0

const DOMAIN_META: Record<MediaKind, { icon: Component; path: string }> = {
  book: { icon: LuBookOpen, path: '/book' },
  comic: { icon: LuLayers, path: '/comic' },
  music: { icon: LuDisc3, path: '/music' },
  video: { icon: LuClapperboard, path: '/video' },
}

function buildDomainSearchRoute(kind: MediaKind, q: string): { path: string; query: { q: string } } {
  if (kind === 'book') return { path: '/book/catalog', query: { q } }
  if (kind === 'comic') return { path: '/comic/series', query: { q } }
  if (kind === 'music') return { path: '/music/search', query: { q } }
  return { path: '/video/catalog', query: { q } }
}

const quickNavItems = computed<QuickResultItem[]>(() => {
  const items: QuickResultItem[] = authStore.enabledModules.map((kind) => ({
    id: `nav:${kind}`,
    group: 'nav',
    title: t(`app.domains.${kind}`),
    to: DOMAIN_META[kind].path,
    icon: DOMAIN_META[kind].icon,
    domain: kind,
  }))
  items.push({
    id: 'nav:settings',
    group: 'nav',
    title: t('nav.settings'),
    to: '/settings',
    icon: LuSettings,
  })
  return items
})

const hasQuery = computed(() => query.value.trim().length > 0)

const flatItems = computed<QuickResultItem[]>(() =>
  hasQuery.value ? domainSections.value.flatMap((sec) => sec.items) : quickNavItems.value,
)

const itemIndexById = computed<Map<string, number>>(() => {
  const map = new Map<string, number>()
  flatItems.value.forEach((item, idx) => map.set(item.id, idx))
  return map
})

function abortPendingSearch(): void {
  if (debounceTimer) {
    clearTimeout(debounceTimer)
    debounceTimer = null
  }
  if (activeController) {
    activeController.abort()
    activeController = null
  }
}

watch(
  () => props.open,
  async (isOpen) => {
    if (isOpen) {
      abortPendingSearch()
      searchSeq += 1
      query.value = ''
      domainSections.value = []
      loading.value = false
      activeIndex.value = 0
      await nextTick()
      inputRef.value?.focus()
    } else {
      abortPendingSearch()
      searchSeq += 1
      loading.value = false
    }
  },
)

watch(query, (val) => {
  abortPendingSearch()
  const q = val.trim()
  activeIndex.value = 0
  if (!q) {
    searchSeq += 1
    domainSections.value = []
    loading.value = false
    return
  }
  loading.value = true
  debounceTimer = setTimeout(() => {
    debounceTimer = null
    void runCrossDomainSearch(q)
  }, 180)
})

onBeforeUnmount(() => {
  abortPendingSearch()
})

async function runCrossDomainSearch(q: string): Promise<void> {
  const seq = ++searchSeq
  const controller = new AbortController()
  activeController = controller
  const { signal } = controller
  const enabled = authStore.enabledModules
  const tasks: Array<Promise<DomainResultSection | null>> = []

  if (enabled.includes('book')) {
    const sp = new URLSearchParams({
      keyword: q,
      page: '1',
      page_size: String(PER_DOMAIN_LIMIT),
    })
    tasks.push(
      requestJson<BookPagedResult<BookSummaryDto>>(`/book/books?${sp.toString()}`, { signal })
        .then((res): DomainResultSection | null => {
          const items: QuickResultItem[] = res.items.slice(0, PER_DOMAIN_LIMIT).map((b) => ({
            id: `book:${b.id}`,
            group: 'book',
            title: b.metadata.title || b.id,
            subtitle: b.metadata.authors.join(', ') || b.format.toUpperCase(),
            to: `/book/items/${b.id}`,
            icon: LuBookOpen,
            domain: 'book',
          }))
          if (items.length === 0) return null
          return {
            domain: 'book',
            label: t('app.domains.book'),
            icon: LuBookOpen,
            items,
            viewAllTo: buildDomainSearchRoute('book', q),
          }
        })
        .catch(() => null),
    )
  }

  if (enabled.includes('comic')) {
    const sp = new URLSearchParams({
      search: q,
      limit: String(PER_DOMAIN_LIMIT),
    })
    tasks.push(
      requestJson<SeriesSummaryDto[]>(`/comic/series?${sp.toString()}`, { signal })
        .then((res): DomainResultSection | null => {
          const items: QuickResultItem[] = res.slice(0, PER_DOMAIN_LIMIT).map((s) => ({
            id: `comic:${s.id}`,
            group: 'comic',
            title: s.title,
            subtitle: s.writers?.join(', ') || `${s.chapter_count} ch`,
            to: `/comic/series/${s.id}`,
            icon: LuLayers,
            domain: 'comic',
          }))
          if (items.length === 0) return null
          return {
            domain: 'comic',
            label: t('app.domains.comic'),
            icon: LuLayers,
            items,
            viewAllTo: buildDomainSearchRoute('comic', q),
          }
        })
        .catch(() => null),
    )
  }

  if (enabled.includes('music')) {
    const sp = new URLSearchParams({
      q,
      limit: String(PER_DOMAIN_LIMIT),
    })
    tasks.push(
      requestJson<MusicSearchResultDto>(`/music/search?${sp.toString()}`, { signal })
        .then((res): DomainResultSection | null => {
          const items: QuickResultItem[] = []
          for (const alb of res.albums.slice(0, 2)) {
            items.push({
              id: `music:album:${alb.id}`,
              group: 'music',
              title: alb.title,
              subtitle: alb.album_artist_name || t('app.domains.music'),
              to: `/music/albums/${alb.id}`,
              icon: LuDisc3,
              domain: 'music',
            })
          }
          for (const tr of res.tracks.slice(0, Math.max(1, PER_DOMAIN_LIMIT - items.length))) {
            items.push({
              id: `music:track:${tr.id}`,
              group: 'music',
              title: tr.title,
              subtitle: [tr.artist_name, tr.album_title].filter(Boolean).join(' · '),
              to: tr.album_id
                ? `/music/albums/${tr.album_id}`
                : `/music/search?q=${encodeURIComponent(q)}`,
              icon: LuMusic,
              domain: 'music',
            })
          }
          if (items.length === 0) return null
          return {
            domain: 'music',
            label: t('app.domains.music'),
            icon: LuDisc3,
            items,
            viewAllTo: buildDomainSearchRoute('music', q),
          }
        })
        .catch(() => null),
    )
  }

  if (enabled.includes('video')) {
    const sp = new URLSearchParams({
      search: q,
      limit: String(PER_DOMAIN_LIMIT),
    })
    tasks.push(
      requestJson<VideoSummaryDto[]>(`/video/videos?${sp.toString()}`, { signal })
        .then((res): DomainResultSection | null => {
          const items: QuickResultItem[] = res.slice(0, PER_DOMAIN_LIMIT).map((v) => ({
            id: `video:${v.id}`,
            group: 'video',
            title: v.title,
            subtitle: v.year ? String(v.year) : t('app.domains.video'),
            to: v.show_id ? `/video/shows/${v.show_id}` : `/video/watch/${v.id}`,
            icon: LuClapperboard,
            domain: 'video',
          }))
          if (items.length === 0) return null
          return {
            domain: 'video',
            label: t('app.domains.video'),
            icon: LuClapperboard,
            items,
            viewAllTo: buildDomainSearchRoute('video', q),
          }
        })
        .catch(() => null),
    )
  }

  const settled = await Promise.all(tasks)
  if (signal.aborted || seq !== searchSeq) return
  if (activeController === controller) {
    activeController = null
  }
  domainSections.value = settled.filter((sec): sec is DomainResultSection => sec !== null)
  activeIndex.value = 0
  loading.value = false
}

function selectItem(item: QuickResultItem): void {
  if (item.domain) {
    domainStore.setActiveDomain(item.domain)
  }
  emit('close')
  void router.push(item.to)
}

function openDomainViewAll(section: DomainResultSection): void {
  domainStore.setActiveDomain(section.domain)
  emit('close')
  void router.push(section.viewAllTo)
}

function scrollActiveIntoView(): void {
  void nextTick(() => {
    const activeEl = listRef.value?.querySelector<HTMLElement>('[data-active="true"]')
    activeEl?.scrollIntoView({ block: 'nearest' })
  })
}

function handleKeydown(ev: KeyboardEvent): void {
  const list = flatItems.value
  if (ev.key === 'ArrowDown') {
    ev.preventDefault()
    if (list.length > 0) {
      activeIndex.value = (activeIndex.value + 1) % list.length
      scrollActiveIntoView()
    }
  } else if (ev.key === 'ArrowUp') {
    ev.preventDefault()
    if (list.length > 0) {
      activeIndex.value = (activeIndex.value - 1 + list.length) % list.length
      scrollActiveIntoView()
    }
  } else if (ev.key === 'Enter') {
    const item = list[activeIndex.value]
    if (item) {
      ev.preventDefault()
      selectItem(item)
    }
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="md-fade">
      <div
        v-if="open"
        class="fixed inset-0 z-[var(--z-modal)] flex items-start justify-center bg-black/45 px-4 pt-[8vh] backdrop-blur-[2px] sm:pt-[10vh]"
        @mousedown.self="emit('close')"
      >
        <Transition name="md-dialog" appear>
          <div
            role="dialog"
            aria-modal="true"
            class="flex w-full max-w-xl flex-col overflow-hidden rounded-[28px] bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
            @keydown="handleKeydown"
          >
            <!-- MD3 Search Dialog Header -->
            <div
              class="flex items-center gap-3 border-b border-[var(--md-sys-color-outline-variant)]/50 px-5 py-3.5"
            >
              <LuLoader2
                v-if="loading"
                class="h-5 w-5 shrink-0 animate-spin text-[var(--md-sys-color-primary)]"
              />
              <LuSearch
                v-else
                class="h-5 w-5 shrink-0 text-[var(--md-sys-color-on-surface-variant)]"
              />

              <input
                ref="inputRef"
                v-model="query"
                type="text"
                :placeholder="
                  t('common.search') +
                  ' (' +
                  authStore.enabledModules.map((m) => t('app.domains.' + m)).join(' / ') +
                  ')…'
                "
                class="min-w-0 flex-1 bg-transparent text-sm text-[var(--md-sys-color-on-surface)] outline-none placeholder:text-[var(--md-sys-color-on-surface-variant)]"
              />

              <button
                v-if="query"
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                :aria-label="t('common.cancel')"
                @click="query = ''"
              >
                <LuX class="h-4 w-4" />
              </button>

              <button
                type="button"
                class="md-state rounded-full bg-[var(--md-sys-color-surface-container)] px-2.5 py-0.5 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
                @click="emit('close')"
              >
                ESC
              </button>
            </div>

            <!-- Results Body -->
            <div ref="listRef" class="max-h-[60vh] overflow-y-auto p-3">
              <!-- Initial loading state -->
              <div
                v-if="loading && hasQuery && flatItems.length === 0"
                class="flex items-center justify-center gap-2 py-12 text-xs text-[var(--md-sys-color-on-surface-variant)]"
              >
                <LuLoader2 class="h-4 w-4 animate-spin text-[var(--md-sys-color-primary)]" />
                <span>{{ t('common.loading') }}</span>
              </div>

              <!-- Empty search state -->
              <div
                v-else-if="!loading && hasQuery && flatItems.length === 0"
                class="py-12 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ t('common.noResults') }}
              </div>

              <!-- Grouped cross-domain results -->
              <div v-else-if="hasQuery" class="space-y-4">
                <section
                  v-for="section in domainSections"
                  :key="section.domain"
                  class="space-y-1"
                >
                  <div class="flex items-center justify-between px-2.5 py-1">
                    <div class="flex items-center gap-1.5 text-[11px] font-semibold tracking-wider text-[var(--md-sys-color-on-surface-variant)] uppercase">
                      <component :is="section.icon" class="h-3.5 w-3.5" />
                      <span>{{ section.label }}</span>
                    </div>

                    <button
                      type="button"
                      class="md-state inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-[11px] font-medium text-[var(--md-sys-color-primary)]"
                      @click="openDomainViewAll(section)"
                    >
                      <span>{{ viewAllLabel }}</span>
                      <LuArrowRight class="h-3 w-3" />
                    </button>
                  </div>

                  <div class="space-y-0.5">
                    <button
                      v-for="item in section.items"
                      :key="item.id"
                      type="button"
                      :data-active="itemIndexById.get(item.id) === activeIndex"
                      class="md-state flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors"
                      :class="
                        itemIndexById.get(item.id) === activeIndex
                          ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                          : 'text-[var(--md-sys-color-on-surface)]'
                      "
                      @mouseenter="activeIndex = itemIndexById.get(item.id) ?? 0"
                      @click="selectItem(item)"
                    >
                      <span
                        class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg"
                        :class="
                          itemIndexById.get(item.id) === activeIndex
                            ? 'bg-[var(--md-sys-color-on-secondary-container)]/10'
                            : 'bg-[var(--md-sys-color-surface-container)] text-[var(--md-sys-color-on-surface-variant)]'
                        "
                      >
                        <component :is="item.icon" class="h-4 w-4" />
                      </span>
                      <div class="min-w-0 flex-1">
                        <p class="truncate text-[13px] font-medium">{{ item.title }}</p>
                        <p
                          v-if="item.subtitle"
                          class="truncate text-[11px]"
                          :class="
                            itemIndexById.get(item.id) === activeIndex
                              ? 'opacity-80'
                              : 'text-[var(--md-sys-color-on-surface-variant)]'
                          "
                        >
                          {{ item.subtitle }}
                        </p>
                      </div>
                      <LuCornerDownLeft
                        v-if="itemIndexById.get(item.id) === activeIndex"
                        class="h-3.5 w-3.5 shrink-0 opacity-75"
                      />
                    </button>
                  </div>
                </section>
              </div>

              <!-- Empty query: Quick Navigation (no raw route path subtitles) -->
              <div v-else class="space-y-1">
                <div class="px-2.5 py-1 text-[11px] font-semibold tracking-wider text-[var(--md-sys-color-on-surface-variant)] uppercase">
                  {{ quickNavHeading }}
                </div>
                <button
                  v-for="(item, idx) in quickNavItems"
                  :key="item.id"
                  type="button"
                  :data-active="idx === activeIndex"
                  class="md-state flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors"
                  :class="
                    idx === activeIndex
                      ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                      : 'text-[var(--md-sys-color-on-surface)]'
                  "
                  @mouseenter="activeIndex = idx"
                  @click="selectItem(item)"
                >
                  <span
                    class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg"
                    :class="
                      idx === activeIndex
                        ? 'bg-[var(--md-sys-color-on-secondary-container)]/10'
                        : 'bg-[var(--md-sys-color-surface-container)] text-[var(--md-sys-color-on-surface-variant)]'
                    "
                  >
                    <component :is="item.icon" class="h-4 w-4" />
                  </span>
                  <span class="min-w-0 flex-1 truncate text-[13px] font-medium">
                    {{ item.title }}
                  </span>
                  <LuCornerDownLeft
                    v-if="idx === activeIndex"
                    class="h-3.5 w-3.5 shrink-0 opacity-75"
                  />
                </button>
              </div>
            </div>
          </div>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
