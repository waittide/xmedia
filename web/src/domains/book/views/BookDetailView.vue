<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import {
  LuArrowLeft,
  LuBookOpen,
  LuHeart,
  LuFolder,
  LuDownload,
  LuMoreVertical,
  LuPencil,
  LuImage,
  LuTrash2,
  LuCheck,
  LuBookmark,
  LuX,
} from 'vue-icons-plus/lu'
import { api } from '@/domains/book/api'
import { useAuthStore } from '@/stores/auth'
import { useCatalogStore } from '@/domains/book/store'
import { useThemeStore } from '@/stores/theme'
import { useDialogStore } from '@/stores/dialog'
import type { Bookmark, BookSummaryDto, TocItem } from '@/domains/book/types'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import TocTree from '@/domains/book/components/TocTree.vue'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const catalogStore = useCatalogStore()
const themeStore = useThemeStore()
const dialogStore = useDialogStore()

const book = ref<BookSummaryDto | null>(null)
const toc = ref<TocItem[]>([])
const bookmarks = ref<Bookmark[]>([])
const loading = ref(true)
const statusMsg = ref('')
const editErrorMsg = ref('')

const shelfPopoverOpen = ref(false)
const moreMenuOpen = ref(false)
const editModalOpen = ref(false)
const saving = ref(false)

const coverInputRef = ref<HTMLInputElement | null>(null)
const actionsAreaRef = ref<HTMLElement | null>(null)

const formTitle = ref('')
const formAuthors = ref('')
const formTags = ref('')
const formIsbn = ref('')
const formLanguage = ref('')
const formPublisher = ref('')
const formPublishedAt = ref('')
const formDescription = ref('')

function onOutsideClick(ev: MouseEvent): void {
  if (actionsAreaRef.value && !actionsAreaRef.value.contains(ev.target as Node)) {
    shelfPopoverOpen.value = false
    moreMenuOpen.value = false
  }
}

async function loadDetail(id: string): Promise<void> {
  loading.value = true
  statusMsg.value = ''
  try {
    const [b, tocItems, bms] = await Promise.all([
      api.getBook(id),
      api.getToc(id).catch(() => []),
      api.listBookmarks(id).catch(() => []),
    ])
    book.value = b
    toc.value = tocItems
    bookmarks.value = bms
    catalogStore.activeBookId = b.id
  } catch {
    book.value = null
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  document.addEventListener('mousedown', onOutsideClick)
  void loadDetail(String(route.params.id))
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', onOutsideClick)
})

watch(
  () => route.params.id,
  (nextId) => {
    if (nextId) void loadDetail(String(nextId))
  },
)

const hasProgress = computed(
  () => Boolean(book.value?.progress && book.value.progress.progress_percent > 0),
)

const formattedLanguage = computed(() => {
  const raw = book.value?.metadata.language
  if (!raw) return null
  try {
    const dn = new Intl.DisplayNames([themeStore.locale], { type: 'language' })
    return dn.of(raw) || raw
  } catch {
    return raw
  }
})

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
}

function goBack(): void {
  const prev = (window.history.state as { back?: string | null } | null)?.back
  if (typeof prev === 'string' && !prev.startsWith('/read/')) {
    router.back()
  } else {
    void router.push('/book/catalog')
  }
}

function filterByAuthor(author: string): void {
  catalogStore.selectSection('all')
  catalogStore.filterAuthors = [author]
  void router.push({ path: '/book/catalog', query: { author } })
}

function filterByTag(tag: string): void {
  catalogStore.selectSection('tag', tag)
  void router.push({ path: '/book/catalog', query: { tag } })
}

async function toggleFavorite(): Promise<void> {
  if (!book.value) return
  const next = !book.value.is_favorite
  await api.setFavorite(book.value.id, next)
  book.value = { ...book.value, is_favorite: next }
  catalogStore.updateBookInList(book.value)
}

async function toggleShelf(shelfId: string, inShelf: boolean): Promise<void> {
  if (!book.value) return
  if (inShelf) {
    await api.removeBookFromShelf(shelfId, book.value.id)
  } else {
    await api.addBookToShelf(shelfId, book.value.id)
  }
  await catalogStore.refreshSidebarData()
}

function openEditModal(): void {
  if (!book.value) return
  moreMenuOpen.value = false
  editErrorMsg.value = ''
  const m = book.value.metadata
  formTitle.value = m.title
  formAuthors.value = m.authors.join(', ')
  formTags.value = m.tags.join(', ')
  formIsbn.value = m.isbn ?? ''
  formLanguage.value = m.language ?? ''
  formPublisher.value = m.publisher ?? ''
  formPublishedAt.value = m.published_at ?? ''
  formDescription.value = m.description ?? ''
  editModalOpen.value = true
}

async function saveMetadata(): Promise<void> {
  if (!book.value) return
  saving.value = true
  try {
    const splitCsv = (val: string) =>
      val
        .split(/[,，]/)
        .map((s) => s.trim())
        .filter(Boolean)

    const updated = await api.updateBook(book.value.id, {
      title: formTitle.value.trim(),
      authors: splitCsv(formAuthors.value),
      tags: splitCsv(formTags.value),
      isbn: formIsbn.value,
      language: formLanguage.value,
      publisher: formPublisher.value,
      published_at: formPublishedAt.value,
      description: formDescription.value,
    })
    book.value = updated
    catalogStore.updateBookInList(updated)
    await catalogStore.refreshSidebarData()
    editModalOpen.value = false
    statusMsg.value = t('detail.savedSuccess')
  } catch (err) {
    editErrorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    saving.value = false
  }
}

async function onCoverChange(ev: Event): Promise<void> {
  if (!book.value) return
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  try {
    const updated = await api.replaceCover(book.value.id, file)
    book.value = updated
    catalogStore.updateBookInList(updated)
  } catch (err) {
    statusMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    input.value = ''
  }
}

async function handleDeleteBook(): Promise<void> {
  if (!book.value) return
  moreMenuOpen.value = false
  const confirmed = await dialogStore.confirm({
    title: t('detail.deleteBook'),
    message: t('detail.confirmDeleteBook', { title: book.value.metadata.title }),
    danger: true,
  })
  if (!confirmed) return
  await api.deleteBook(book.value.id)
  await Promise.all([catalogStore.refreshSidebarData(), catalogStore.fetchBooks()])
  await router.replace('/book/catalog')
}

async function deleteBookmark(bmId: string): Promise<void> {
  await api.deleteBookmark(bmId)
  bookmarks.value = bookmarks.value.filter((b) => b.id !== bmId)
}

function openTocTargetInReader(href: string): void {
  if (!book.value) return
  const chMatch = /^chapter:(\d+)$/.exec(href)
  const pgMatch = /^page:(\d+)$/.exec(href)
  void router.push({
    path: `/book/read/${book.value.id}`,
    query: {
      locator: href,
      chapter: chMatch ? chMatch[1] : undefined,
      page: pgMatch ? pgMatch[1] : undefined,
    },
  })
}

function openBookmarkInReader(bm: Bookmark): void {
  if (!book.value) return
  void router.push({
    path: `/book/read/${book.value.id}`,
    query: {
      locator: bm.location.locator,
      chapter: bm.location.chapter_index !== null ? String(bm.location.chapter_index) : undefined,
      page: bm.location.page_number !== null ? String(bm.location.page_number) : undefined,
      ratio: bm.location.scroll_ratio !== null ? String(bm.location.scroll_ratio) : undefined,
    },
  })
}
</script>

<template>
  <div class="mx-auto max-w-5xl px-6 py-6 sm:px-8">
    <!-- Top Back & Refresh Bar -->
    <div class="mb-6 flex items-center justify-between gap-3">
      <button
        type="button"
        class="inline-flex items-center gap-1.5 rounded-xl px-2.5 py-1.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-mantle)] hover:text-[var(--ctp-text)]"
        @click="goBack"
      >
        <LuArrowLeft class="h-4 w-4" />
        <span>{{ t('detail.back') }}</span>
      </button>

      <RefreshButton
        :action="() => loadDetail(String(route.params.id))"
        :busy="loading"
      />
    </div>

    <div v-if="loading" class="py-20 text-center text-sm text-[var(--ctp-subtext0)]">
      {{ t('common.loading') }}
    </div>

    <div v-else-if="!book" class="py-20 text-center text-sm text-[var(--ctp-subtext0)]">
      {{ t('detail.notFound') }}
    </div>

    <div v-else class="space-y-12">
      <!-- BeePub-style Hero Section: Cover + Title/Author/Progress/Actions -->
      <section class="flex flex-col gap-8 md:flex-row md:gap-12">
        <!-- Left Cover Column -->
        <div class="mx-auto w-52 shrink-0 md:mx-0 md:w-60">
          <div class="book-shadow group relative aspect-[2/3] w-full overflow-hidden rounded-xl bg-[var(--ctp-mantle)]">
            <img
              v-if="book.has_cover"
              :src="api.coverUrl(book.id, book.updated_at)"
              :alt="book.metadata.title"
              class="h-full w-full object-cover"
            />
            <div
              v-else
              class="flex h-full w-full flex-col justify-between p-5"
            >
              <span class="self-end rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-xs font-bold uppercase text-[var(--ctp-accent)]">
                {{ book.format }}
              </span>
              <p class="text-base font-bold text-[var(--ctp-text)]">{{ book.metadata.title }}</p>
            </div>

            <template v-if="authStore.isAdmin">
              <button
                type="button"
                class="absolute inset-0 flex items-center justify-center gap-1.5 bg-black/55 text-xs font-semibold text-white opacity-0 transition group-hover:opacity-100"
                @click="coverInputRef?.click()"
              >
                <LuImage class="h-4 w-4" />
                <span>{{ t('detail.replaceCover') }}</span>
              </button>
              <input
                ref="coverInputRef"
                type="file"
                accept="image/*"
                class="hidden"
                @change="onCoverChange"
              />
            </template>
          </div>
        </div>

        <!-- Right Info & Action Column -->
        <div class="flex min-w-0 flex-1 flex-col pt-1">
          <div>
            <div class="mb-2 flex flex-wrap items-center gap-2">
              <span class="rounded-full bg-[var(--ctp-accent-soft)] px-3 py-0.5 text-xs font-semibold uppercase text-[var(--ctp-accent)]">
                {{ book.format }}
              </span>
              <span
                v-if="book.is_missing"
                class="rounded-full bg-[var(--ctp-red)]/15 px-3 py-0.5 text-xs font-semibold text-[var(--ctp-red)]"
              >
                {{ t('catalog.missingBadge') }}
              </span>
            </div>

            <h1 class="font-serif text-3xl font-bold leading-tight text-[var(--ctp-text)] sm:text-4xl">
              {{ book.metadata.title }}
            </h1>

            <p v-if="book.metadata.authors.length > 0" class="mt-2.5 text-base text-[var(--ctp-subtext1)]">
              <template v-for="(author, idx) in book.metadata.authors" :key="author">
                <button
                  type="button"
                  class="hover:text-[var(--ctp-accent)] hover:underline transition-colors"
                  @click="filterByAuthor(author)"
                >
                  {{ author }}
                </button><span v-if="idx < book.metadata.authors.length - 1">, </span>
              </template>
            </p>
            <p v-else class="mt-2 text-sm text-[var(--ctp-subtext0)]">
              {{ t('catalog.unknownAuthor') }}
            </p>
          </div>

          <!-- Reading Progress Strip -->
          <div class="mt-6 max-w-md rounded-2xl bg-[var(--ctp-shell)] p-3.5 card-soft">
            <div class="flex items-center justify-between text-xs">
              <span class="font-medium text-[var(--ctp-subtext1)]">
                {{
                  hasProgress && book.progress
                    ? t('catalog.progressLabel', { percent: Math.round(book.progress.progress_percent) })
                    : t('catalog.notStarted')
                }}
              </span>
              <span v-if="hasProgress && book.progress" class="font-mono font-semibold text-[var(--ctp-accent)]">
                {{ Math.round(book.progress.progress_percent) }}%
              </span>
            </div>
            <div class="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-[var(--ctp-surface0)]">
              <div
                class="h-full rounded-full bg-[var(--ctp-accent)] transition-all"
                :style="{ width: `${book.progress ? Math.min(100, book.progress.progress_percent) : 0}%` }"
              />
            </div>
          </div>

          <p v-if="statusMsg" class="mt-3 text-xs font-medium text-[var(--ctp-green)]">
            {{ statusMsg }}
          </p>

          <!-- BeePub-style Action Button Bar -->
          <div ref="actionsAreaRef" class="mt-auto flex flex-wrap items-center gap-2.5 pt-7">
            <button
              type="button"
              :disabled="book.is_missing"
              class="inline-flex h-11 items-center justify-center gap-2 rounded-full bg-[var(--ctp-accent)] px-6 text-sm font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90 disabled:opacity-40"
              @click="router.push(`/book/read/${book.id}`)"
            >
              <LuBookOpen class="h-4 w-4" />
              <span>{{ hasProgress ? t('catalog.continueReading') : t('catalog.startReading') }}</span>
            </button>

            <!-- Favorite Circle Button -->
            <button
              type="button"
              class="card-soft flex h-11 w-11 items-center justify-center rounded-full bg-[var(--ctp-shell)] transition hover:shadow-sm"
              :class="book.is_favorite ? 'text-[var(--ctp-red)]' : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'"
              :title="book.is_favorite ? t('detail.favoriteRemove') : t('detail.favoriteAdd')"
              @click="toggleFavorite"
            >
              <LuHeart class="h-4 w-4" :class="book.is_favorite ? 'fill-current' : ''" />
            </button>

            <!-- Shelves Dropdown Circle Button -->
            <div class="relative">
              <button
                type="button"
                class="card-soft flex h-11 w-11 items-center justify-center rounded-full bg-[var(--ctp-shell)] text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)] hover:shadow-sm"
                :title="t('detail.shelvesMenu')"
                @click="shelfPopoverOpen = !shelfPopoverOpen; moreMenuOpen = false"
              >
                <LuFolder class="h-4 w-4" />
              </button>

              <div
                v-if="shelfPopoverOpen"
                class="absolute left-0 top-13 z-30 w-56 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-2 shadow-2xl"
              >
                <div class="px-2.5 py-1.5 text-[11px] font-semibold text-[var(--ctp-subtext0)]">
                  {{ t('detail.shelvesMenu') }}
                </div>
                <p
                  v-if="catalogStore.shelves.length === 0"
                  class="px-2.5 py-2 text-xs text-[var(--ctp-subtext0)]"
                >
                  {{ t('detail.noShelvesYet') }}
                </p>
                <button
                  v-for="sh in catalogStore.shelves"
                  :key="sh.id"
                  type="button"
                  class="flex w-full items-center justify-between rounded-xl px-2.5 py-2 text-xs font-medium transition hover:bg-[var(--ctp-surface0)]"
                  @click="toggleShelf(sh.id, sh.book_ids.includes(book.id))"
                >
                  <span class="truncate">{{ sh.name }}</span>
                  <LuCheck v-if="sh.book_ids.includes(book.id)" class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                </button>
              </div>
            </div>

            <!-- Download Circle Button -->
            <a
              v-if="!book.is_missing"
              :href="api.fileUrl(book.id, true)"
              class="card-soft flex h-11 w-11 items-center justify-center rounded-full bg-[var(--ctp-shell)] text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)] hover:shadow-sm"
              :title="t('catalog.download')"
            >
              <LuDownload class="h-4 w-4" />
            </a>

            <!-- More Actions Dropdown (Admin only: Edit Metadata / Replace Cover / Delete) -->
            <div v-if="authStore.isAdmin" class="relative">
              <button
                type="button"
                class="card-soft flex h-11 w-11 items-center justify-center rounded-full bg-[var(--ctp-shell)] text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)] hover:shadow-sm"
                :title="t('detail.moreActions')"
                @click="moreMenuOpen = !moreMenuOpen; shelfPopoverOpen = false"
              >
                <LuMoreVertical class="h-4 w-4" />
              </button>

              <div
                v-if="moreMenuOpen"
                class="absolute left-0 top-13 z-30 w-48 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-1.5 shadow-2xl"
              >
                <button
                  type="button"
                  class="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium text-[var(--ctp-text)] hover:bg-[var(--ctp-surface0)]"
                  @click="openEditModal"
                >
                  <LuPencil class="h-3.5 w-3.5 text-[var(--ctp-subtext0)]" />
                  <span>{{ t('detail.editMetadata') }}</span>
                </button>

                <button
                  type="button"
                  class="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium text-[var(--ctp-text)] hover:bg-[var(--ctp-surface0)]"
                  @click="moreMenuOpen = false; coverInputRef?.click()"
                >
                  <LuImage class="h-3.5 w-3.5 text-[var(--ctp-subtext0)]" />
                  <span>{{ t('detail.replaceCover') }}</span>
                </button>

                <template v-if="authStore.isAdmin">
                  <div class="my-1 h-px bg-[var(--ctp-surface0)]" />
                  <button
                    type="button"
                    class="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-left text-xs font-medium text-[var(--ctp-red)] hover:bg-[var(--ctp-red)]/10"
                    @click="handleDeleteBook"
                  >
                    <LuTrash2 class="h-3.5 w-3.5" />
                    <span>{{ t('detail.deleteBook') }}</span>
                  </button>
                </template>
              </div>
            </div>
          </div>
        </div>
      </section>

      <!-- Lower Split Section: Synopsis & Bookmarks + Metadata Specs Sidebar -->
      <section class="flex flex-col gap-10 border-t border-[var(--ctp-surface0)]/70 pt-8 lg:flex-row">
        <!-- Main Column -->
        <div class="min-w-0 flex-1 space-y-8">
          <div>
            <h2 class="mb-3 text-sm font-bold uppercase tracking-wider text-[var(--ctp-subtext0)]">
              {{ t('detail.synopsis') }}
            </h2>
            <p
              v-if="book.metadata.description"
              class="whitespace-pre-line text-sm leading-7 text-[var(--ctp-text)]"
            >
              {{ book.metadata.description }}
            </p>
            <p v-else class="text-sm italic text-[var(--ctp-subtext0)]">
              {{ t('detail.noSynopsis') }}
            </p>

            <!-- Clickable Subject Tags -->
            <div v-if="book.metadata.tags.length > 0" class="mt-5 flex flex-wrap gap-2">
              <button
                v-for="tg in book.metadata.tags"
                :key="tg"
                type="button"
                class="rounded-full bg-[var(--ctp-mantle)] px-3 py-1 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-accent-soft)] hover:text-[var(--ctp-accent)]"
                @click="filterByTag(tg)"
              >
                #{{ tg }}
              </button>
            </div>
          </div>

          <!-- Hierarchical Volume / Chapter Table of Contents Preview -->
          <div v-if="toc.length > 0">
            <h2 class="mb-3 text-sm font-bold uppercase tracking-wider text-[var(--ctp-subtext0)]">
              {{ t('reader.toc') }} ({{ toc.length }})
            </h2>
            <div class="card-soft max-h-80 overflow-y-auto rounded-2xl bg-[var(--ctp-shell)] p-3">
              <TocTree
                :items="toc"
                :active-href="book.progress?.locator"
                @select="openTocTargetInReader"
              />
            </div>
          </div>

          <!-- Personal Bookmarks List -->
          <div v-if="bookmarks.length > 0">
            <h2 class="mb-3 text-sm font-bold uppercase tracking-wider text-[var(--ctp-subtext0)]">
              {{ t('detail.bookmarksSection') }} ({{ bookmarks.length }})
            </h2>
            <div class="space-y-2">
              <div
                v-for="bm in bookmarks"
                :key="bm.id"
                class="card-soft flex items-center justify-between gap-3 rounded-2xl bg-[var(--ctp-shell)] px-4 py-3"
              >
                <button
                  type="button"
                  class="flex min-w-0 flex-1 items-center gap-3 text-left"
                  @click="openBookmarkInReader(bm)"
                >
                  <LuBookmark class="h-4 w-4 shrink-0 text-[var(--ctp-accent)]" />
                  <div class="min-w-0">
                    <p class="truncate text-sm font-medium text-[var(--ctp-text)] hover:text-[var(--ctp-accent)]">
                      {{ bm.title }}
                    </p>
                    <p class="text-xs text-[var(--ctp-subtext0)]">
                      {{ Math.round(bm.location.progress_percent) }}%
                    </p>
                  </div>
                </button>
                <button
                  type="button"
                  class="rounded-lg p-1.5 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-red)]"
                  @click="deleteBookmark(bm.id)"
                >
                  <LuTrash2 class="h-3.5 w-3.5" />
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Metadata Specifications Sidebar (BeePub BookMetadataSidebar style) -->
        <aside class="w-full shrink-0 lg:w-72">
          <div class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-5">
            <h2 class="mb-4 text-xs font-bold uppercase tracking-wider text-[var(--ctp-subtext0)]">
              {{ t('detail.specifications') }}
            </h2>
            <dl class="space-y-3 text-xs">
              <div class="flex justify-between gap-3">
                <dt class="text-[var(--ctp-subtext0)]">{{ t('detail.library') }}</dt>
                <dd class="truncate font-medium text-[var(--ctp-text)]">{{ book.library_name || '—' }}</dd>
              </div>
              <div class="flex justify-between gap-3">
                <dt class="text-[var(--ctp-subtext0)]">{{ t('detail.sourceMode') }}</dt>
                <dd class="font-medium text-[var(--ctp-text)]">
                  {{ book.is_managed ? t('catalog.managedBadge') : t('catalog.mountedBadge') }}
                </dd>
              </div>
              <div class="flex justify-between gap-3">
                <dt class="text-[var(--ctp-subtext0)]">{{ t('detail.format') }}</dt>
                <dd class="font-mono font-semibold uppercase text-[var(--ctp-text)]">{{ book.format }}</dd>
              </div>
              <div class="flex justify-between gap-3">
                <dt class="text-[var(--ctp-subtext0)]">{{ t('detail.fileSize') }}</dt>
                <dd class="font-medium text-[var(--ctp-text)]">{{ formatBytes(book.file_size) }}</dd>
              </div>
              <div v-if="book.metadata.chapter_count || book.metadata.page_count" class="flex justify-between gap-3">
                <dt class="text-[var(--ctp-subtext0)]">{{ t('detail.structure') }}</dt>
                <dd class="font-medium text-[var(--ctp-text)]">
                  <template v-if="book.metadata.page_count">
                    {{ t('detail.pages', { count: book.metadata.page_count }) }}
                  </template>
                  <template v-else-if="book.metadata.chapter_count">
                    {{ t('detail.chapters', { count: book.metadata.chapter_count }) }}
                  </template>
                </dd>
              </div>
              <div v-if="formattedLanguage" class="flex justify-between gap-3">
                <dt class="text-[var(--ctp-subtext0)]">{{ t('detail.language') }}</dt>
                <dd class="font-medium text-[var(--ctp-text)]">{{ formattedLanguage }}</dd>
              </div>
              <div v-if="book.metadata.publisher" class="flex justify-between gap-3">
                <dt class="text-[var(--ctp-subtext0)]">{{ t('detail.publisher') }}</dt>
                <dd class="truncate font-medium text-[var(--ctp-text)]">{{ book.metadata.publisher }}</dd>
              </div>
              <div v-if="book.metadata.published_at" class="flex justify-between gap-3">
                <dt class="text-[var(--ctp-subtext0)]">{{ t('detail.publishedAt') }}</dt>
                <dd class="font-medium text-[var(--ctp-text)]">{{ book.metadata.published_at }}</dd>
              </div>
              <div v-if="book.metadata.isbn" class="flex justify-between gap-3">
                <dt class="text-[var(--ctp-subtext0)]">{{ t('detail.isbn') }}</dt>
                <dd class="font-mono font-medium text-[var(--ctp-text)]">{{ book.metadata.isbn }}</dd>
              </div>
            </dl>
          </div>
        </aside>
      </section>
    </div>

    <!-- Clean Spacious Metadata Edit Dialog -->
    <div
      v-if="editModalOpen"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
    >
      <div class="w-full max-w-xl rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-6 shadow-2xl">
        <div class="mb-5 flex items-center justify-between">
          <h3 class="text-base font-bold">{{ t('detail.editMetadata') }}</h3>
          <button
            type="button"
            class="rounded-full p-1.5 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)]"
            @click="editModalOpen = false"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <div
          v-if="editErrorMsg"
          class="mb-4 rounded-xl bg-[var(--ctp-red)]/15 px-3.5 py-2.5 text-xs font-medium text-[var(--ctp-red)]"
        >
          {{ editErrorMsg }}
        </div>

        <form class="space-y-4" @submit.prevent="saveMetadata">
          <div>
            <label class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">{{ t('detail.bookTitle') }}</label>
            <input
              v-model="formTitle"
              type="text"
              required
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-3.5 py-2 text-sm outline-none focus:border-[var(--ctp-accent)]"
            />
          </div>

          <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div>
              <label class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">{{ t('detail.authors') }}</label>
              <input
                v-model="formAuthors"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-3.5 py-2 text-sm outline-none focus:border-[var(--ctp-accent)]"
              />
            </div>
            <div>
              <label class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">{{ t('detail.tags') }}</label>
              <input
                v-model="formTags"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-3.5 py-2 text-sm outline-none focus:border-[var(--ctp-accent)]"
              />
            </div>
          </div>

          <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <div>
              <label class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">{{ t('detail.language') }}</label>
              <input
                v-model="formLanguage"
                type="text"
                placeholder="zh-CN / en"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-3 py-2 text-xs outline-none focus:border-[var(--ctp-accent)]"
              />
            </div>
            <div>
              <label class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">{{ t('detail.isbn') }}</label>
              <input
                v-model="formIsbn"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-3 py-2 text-xs outline-none focus:border-[var(--ctp-accent)]"
              />
            </div>
            <div>
              <label class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">{{ t('detail.publisher') }}</label>
              <input
                v-model="formPublisher"
                type="text"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-3 py-2 text-xs outline-none focus:border-[var(--ctp-accent)]"
              />
            </div>
            <div>
              <label class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">{{ t('detail.publishedAt') }}</label>
              <input
                v-model="formPublishedAt"
                type="text"
                placeholder="2024"
                class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-3 py-2 text-xs outline-none focus:border-[var(--ctp-accent)]"
              />
            </div>
          </div>

          <div>
            <label class="mb-1 block text-xs font-medium text-[var(--ctp-subtext0)]">{{ t('detail.description') }}</label>
            <textarea
              v-model="formDescription"
              rows="5"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-3.5 py-2 text-sm outline-none focus:border-[var(--ctp-accent)]"
            />
          </div>

          <div class="flex justify-end gap-2 pt-2">
            <button
              type="button"
              class="rounded-xl px-4 py-2 text-xs font-medium text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]"
              @click="editModalOpen = false"
            >
              {{ t('detail.cancel') }}
            </button>
            <button
              type="submit"
              :disabled="saving"
              class="rounded-xl bg-[var(--ctp-accent)] px-5 py-2 text-xs font-semibold text-[var(--ctp-on-accent)]"
            >
              {{ t('detail.saveChanges') }}
            </button>
          </div>
        </form>
      </div>
    </div>
  </div>
</template>
