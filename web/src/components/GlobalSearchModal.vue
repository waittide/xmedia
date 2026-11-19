<script setup lang="ts">
import { computed, nextTick, ref, watch, type Component } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
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
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import { api as bookApi } from '@/domains/book/api'
import { api as comicApi } from '@/domains/comic/api'
import { api as musicApi } from '@/domains/music/api'
import { api as videoApi } from '@/domains/video/api'
import type { MediaKind } from '@/types/api'

interface QuickResultItem {
  id: string
  group: MediaKind | 'nav'
  title: string
  subtitle: string
  to: string
  icon: Component
  domain?: MediaKind
}

const props = defineProps<{
  open: boolean
}>()

const emit = defineEmits<{
  close: []
}>()

const { t } = useI18n()
const router = useRouter()
const authStore = useAuthStore()
const domainStore = useDomainStore()

const inputRef = ref<HTMLInputElement | null>(null)
const query = ref('')
const loading = ref(false)
const results = ref<QuickResultItem[]>([])
const activeIndex = ref(0)

let debounceTimer: ReturnType<typeof setTimeout> | null = null
let searchSeq = 0

const DOMAIN_META: Record<MediaKind, { icon: Component; path: string }> = {
  book: { icon: LuBookOpen, path: '/book' },
  comic: { icon: LuLayers, path: '/comic' },
  music: { icon: LuDisc3, path: '/music' },
  video: { icon: LuClapperboard, path: '/video' },
}

const quickNavItems = computed<QuickResultItem[]>(() => {
  const items: QuickResultItem[] = authStore.enabledModules.map((kind) => ({
    id: `nav:${kind}`,
    group: 'nav',
    title: t(`app.domains.${kind}`),
    subtitle: DOMAIN_META[kind].path,
    to: DOMAIN_META[kind].path,
    icon: DOMAIN_META[kind].icon,
    domain: kind,
  }))
  items.push({
    id: 'nav:settings',
    group: 'nav',
    title: t('nav.settings'),
    subtitle: '/settings',
    to: '/settings',
    icon: LuSettings,
  })
  return items
})

const displayItems = computed<QuickResultItem[]>(() =>
  query.value.trim().length > 0 ? results.value : quickNavItems.value,
)

watch(
  () => props.open,
  async (isOpen) => {
    if (isOpen) {
      query.value = ''
      results.value = []
      activeIndex.value = 0
      await nextTick()
      inputRef.value?.focus()
    } else if (debounceTimer) {
      clearTimeout(debounceTimer)
      debounceTimer = null
    }
  },
)

watch(query, (val) => {
  if (debounceTimer) clearTimeout(debounceTimer)
  const q = val.trim()
  activeIndex.value = 0
  if (!q) {
    results.value = []
    loading.value = false
    return
  }
  loading.value = true
  debounceTimer = setTimeout(() => {
    void runCrossDomainSearch(q)
  }, 180)
})

async function runCrossDomainSearch(q: string): Promise<void> {
  const seq = ++searchSeq
  const enabled = authStore.enabledModules
  const tasks: Array<Promise<QuickResultItem[]>> = []

  if (enabled.includes('book')) {
    tasks.push(
      bookApi
        .searchBooks({ keyword: q, page: 1, page_size: 4 })
        .then((res) =>
          res.items.map((b) => ({
            id: `book:${b.id}`,
            group: 'book' as const,
            title: b.metadata.title || b.id,
            subtitle: b.metadata.authors.join(', ') || b.format.toUpperCase(),
            to: `/book/items/${b.id}`,
            icon: LuBookOpen,
            domain: 'book' as const,
          })),
        )
        .catch(() => []),
    )
  }

  if (enabled.includes('comic')) {
    tasks.push(
      comicApi
        .listSeries({ search: q })
        .then((res) =>
          res.slice(0, 4).map((s) => ({
            id: `comic:${s.id}`,
            group: 'comic' as const,
            title: s.title,
            subtitle: s.writers?.join(', ') || `${s.chapter_count} ch`,
            to: `/comic/series/${s.id}`,
            icon: LuLayers,
            domain: 'comic' as const,
          })),
        )
        .catch(() => []),
    )
  }

  if (enabled.includes('music')) {
    tasks.push(
      musicApi
        .search(q, 4)
        .then((res) => {
          const out: QuickResultItem[] = []
          for (const alb of res.albums.slice(0, 2)) {
            out.push({
              id: `music:album:${alb.id}`,
              group: 'music',
              title: alb.title,
              subtitle: alb.album_artist_name || t('app.domains.music'),
              to: `/music/albums/${alb.id}`,
              icon: LuDisc3,
              domain: 'music',
            })
          }
          for (const tr of res.tracks.slice(0, 3)) {
            out.push({
              id: `music:track:${tr.id}`,
              group: 'music',
              title: tr.title,
              subtitle: [tr.artist_name, tr.album_title].filter(Boolean).join(' · '),
              to: tr.album_id ? `/music/albums/${tr.album_id}` : `/music/search?q=${encodeURIComponent(q)}`,
              icon: LuMusic,
              domain: 'music',
            })
          }
          return out
        })
        .catch(() => []),
    )
  }

  if (enabled.includes('video')) {
    tasks.push(
      videoApi
        .listVideos({ search: q })
        .then((res) =>
          res.slice(0, 4).map((v) => ({
            id: `video:${v.id}`,
            group: 'video' as const,
            title: v.title,
            subtitle: v.year ? String(v.year) : t('app.domains.video'),
            to: v.show_id ? `/video/shows/${v.show_id}` : `/video/watch/${v.id}`,
            icon: LuClapperboard,
            domain: 'video' as const,
          })),
        )
        .catch(() => []),
    )
  }

  const settled = await Promise.all(tasks)
  if (seq !== searchSeq) return
  results.value = settled.flat()
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

function handleKeydown(ev: KeyboardEvent): void {
  const list = displayItems.value
  if (ev.key === 'Escape') {
    ev.preventDefault()
    emit('close')
  } else if (ev.key === 'ArrowDown') {
    ev.preventDefault()
    if (list.length > 0) {
      activeIndex.value = (activeIndex.value + 1) % list.length
    }
  } else if (ev.key === 'ArrowUp') {
    ev.preventDefault()
    if (list.length > 0) {
      activeIndex.value = (activeIndex.value - 1 + list.length) % list.length
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
    <div
      v-if="open"
      class="fixed inset-0 z-[9990] flex items-start justify-center bg-black/50 px-4 pt-[12vh] backdrop-blur-xs"
      @mousedown.self="emit('close')"
    >
      <div
        class="w-full max-w-xl overflow-hidden rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] shadow-2xl"
        @keydown="handleKeydown"
      >
        <!-- Search Input Header -->
        <div class="flex items-center gap-3 border-b border-[var(--ctp-surface0)] px-4 py-3">
          <LuLoader2
            v-if="loading"
            class="h-4 w-4 shrink-0 animate-spin text-[var(--ctp-accent)]"
          />
          <LuSearch v-else class="h-4 w-4 shrink-0 text-[var(--ctp-subtext0)]" />

          <input
            ref="inputRef"
            v-model="query"
            type="text"
            :placeholder="`${t('common.search')} (${authStore.enabledModules.map((m) => t(`app.domains.${m}`)).join(' / ')})…`"
            class="min-w-0 flex-1 bg-transparent text-sm text-[var(--ctp-text)] outline-none placeholder:text-[var(--ctp-subtext0)]"
          />

          <button
            v-if="query"
            type="button"
            class="rounded-lg p-1 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
            @click="query = ''"
          >
            <LuX class="h-4 w-4" />
          </button>

          <kbd
            class="hidden rounded-md border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--ctp-subtext0)] sm:inline"
          >
            ESC
          </kbd>
        </div>

        <!-- Results Body -->
        <div class="max-h-[55vh] overflow-y-auto p-2">
          <div
            v-if="!loading && query.trim() && displayItems.length === 0"
            class="py-10 text-center text-xs text-[var(--ctp-subtext0)]"
          >
            {{ t('common.noResults') }}
          </div>

          <button
            v-for="(item, idx) in displayItems"
            :key="item.id"
            type="button"
            class="flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition"
            :class="
              idx === activeIndex
                ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-text)] hover:bg-[var(--ctp-mantle)]'
            "
            @mouseenter="activeIndex = idx"
            @click="selectItem(item)"
          >
            <component
              :is="item.icon"
              class="h-4 w-4 shrink-0"
              :class="idx === activeIndex ? 'text-[var(--ctp-accent)]' : 'text-[var(--ctp-subtext0)]'"
            />
            <div class="min-w-0 flex-1">
              <p class="truncate text-xs font-semibold">{{ item.title }}</p>
              <p
                v-if="item.subtitle"
                class="truncate text-[11px] text-[var(--ctp-subtext0)]"
              >
                {{ item.subtitle }}
              </p>
            </div>
            <span
              v-if="item.group !== 'nav'"
              class="shrink-0 rounded-md bg-[var(--ctp-surface0)]/70 px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext1)]"
            >
              {{ t(`app.domains.${item.group}`) }}
            </span>
            <LuCornerDownLeft
              v-if="idx === activeIndex"
              class="h-3.5 w-3.5 shrink-0 opacity-75"
            />
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
