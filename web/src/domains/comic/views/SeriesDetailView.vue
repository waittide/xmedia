<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import {
  LuArrowUpDown,
  LuBookOpen,
  LuBookmark,
  LuCheck,
  LuCheckCheck,
  LuChevronDown,
  LuChevronRight,
  LuHeart,
  LuImage,
  LuPencil,
  LuPlay,
  LuTrash2,
  LuUpload,
} from 'vue-icons-plus/lu'
import { api, withAuthUrl } from '@/domains/comic/api'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSidebarStore } from '@/domains/comic/store'
import { useUploadStore } from '@/stores/uploads'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppDialog from '@/components/ui/AppDialog.vue'
import SeriesMetadataModal from '@/domains/comic/components/SeriesMetadataModal.vue'
import ChapterMetadataModal from '@/domains/comic/components/ChapterMetadataModal.vue'
import type { ChapterItemDto, SeriesDetailDto, ShelfDetailDto } from '@/domains/comic/types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const dialog = useDialogStore()
const sidebarStore = useSidebarStore()
const upload = useUploadStore()

const detail = ref<SeriesDetailDto | null>(null)
const loading = ref(true)
const orderAsc = ref(true)
const groupByVolume = ref(true)
const collapsedVolumes = ref<Record<string, boolean>>({})

const editSeriesOpen = ref(false)
const editingChapter = ref<ChapterItemDto | null>(null)
const shelfModalOpen = ref(false)
const allShelves = ref<ShelfDetailDto[]>([])
const selectedShelfIds = ref<string[]>([])

const coverInputRef = ref<HTMLInputElement | null>(null)
const chapterUploadRef = ref<HTMLInputElement | null>(null)

const seriesId = computed(() => String(route.params.id))

async function loadDetail(): Promise<void> {
  loading.value = true
  try {
    detail.value = await api.getSeriesDetail(seriesId.value)
  } finally {
    loading.value = false
  }
}

onMounted(loadDetail)
watch([seriesId, () => upload.catalogVersion], loadDetail)

const sortedChapters = computed<ChapterItemDto[]>(() => {
  if (!detail.value) return []
  const list = [...detail.value.chapters]
  return orderAsc.value ? list : list.reverse()
})

const hasAnyVolumes = computed(() =>
  Boolean(detail.value?.chapters.some((c) => c.volume_number !== null))
)

interface VolumeGroup {
  key: string
  volumeNumber: number | null
  title: string
  chapters: ChapterItemDto[]
}

const volumeGroups = computed<VolumeGroup[]>(() => {
  const groups = new Map<string, VolumeGroup>()
  for (const ch of sortedChapters.value) {
    const key = ch.volume_number !== null ? String(ch.volume_number) : 'unassigned'
    let group = groups.get(key)
    if (!group) {
      group = {
        key,
        volumeNumber: ch.volume_number,
        title:
          ch.volume_number !== null
            ? t('series.volumeGroupTitle', { vol: ch.volume_number })
            : t('series.unassignedVolume'),
        chapters: [],
      }
      groups.set(key, group)
    }
    group.chapters.push(ch)
  }
  return Array.from(groups.values())
})

function toggleVolumeCollapse(key: string): void {
  collapsedVolumes.value[key] = !collapsedVolumes.value[key]
}

function primaryReadAction(): void {
  if (!detail.value) return
  if (detail.value.continue_reading) {
    router.push(
      '/comic/read/' +
        detail.value.continue_reading.chapter_id +
        '?page=' +
        detail.value.continue_reading.current_page
    )
    return
  }
  const first = detail.value.chapters.find((c) => !c.is_missing)
  if (first) {
    router.push('/comic/read/' + first.id + '?page=1')
  }
}

function openChapter(ch: ChapterItemDto): void {
  if (ch.is_missing) return
  const startPage = ch.current_page > 0 && !ch.is_completed ? ch.current_page : 1
  router.push('/comic/read/' + ch.id + '?page=' + startPage)
}

function chapterKindBadge(ch: ChapterItemDto): string {
  if (ch.kind === 'volume') return t('series.kindVolume', { num: ch.number_label })
  if (ch.kind === 'special') return t('series.kindSpecial', { num: ch.number_label })
  return t('series.kindChapter', { num: ch.number_label })
}

async function toggleFavorite(): Promise<void> {
  if (!detail.value) return
  const next = !detail.value.is_favorite
  await api.setFavorite(detail.value.series.id, next)
  detail.value.is_favorite = next
}

async function openShelfDialog(): Promise<void> {
  if (!detail.value) return
  allShelves.value = await api.listShelves()
  selectedShelfIds.value = [...detail.value.shelf_ids]
  shelfModalOpen.value = true
}

function setShelfChecked(id: string, checked: boolean): void {
  if (checked) {
    if (!selectedShelfIds.value.includes(id)) selectedShelfIds.value.push(id)
  } else {
    selectedShelfIds.value = selectedShelfIds.value.filter((x) => x !== id)
  }
}

async function saveShelvesSelection(): Promise<void> {
  if (!detail.value) return
  await api.setSeriesShelves(detail.value.series.id, selectedShelfIds.value)
  detail.value.shelf_ids = [...selectedShelfIds.value]
  shelfModalOpen.value = false
  await sidebarStore.refresh()
}

async function onSeriesMetadataSaved(): Promise<void> {
  await Promise.all([loadDetail(), sidebarStore.refresh()])
}

async function toggleChapterRead(ch: ChapterItemDto): Promise<void> {
  await api.setChapterCompleted(ch.id, !ch.is_completed)
  await loadDetail()
}

async function handleMarkPreviousRead(ch: ChapterItemDto): Promise<void> {
  await api.markPreviousRead(ch.id)
  await loadDetail()
}

async function handleMarkSeriesRead(completed: boolean): Promise<void> {
  if (!detail.value) return
  await api.markSeriesRead(detail.value.series.id, completed)
  await loadDetail()
}

async function handleMarkVolumeRead(volNum: number): Promise<void> {
  if (!detail.value) return
  await api.markVolumeRead(detail.value.series.id, volNum, true)
  await loadDetail()
}

async function onPickCover(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file || !detail.value) return
  await api.uploadSeriesCover(detail.value.series.id, file)
  input.value = ''
  await loadDetail()
}

function onUploadChaptersToSeries(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0 && detail.value) {
    upload.enqueueFiles(input.files, { targetSeriesId: detail.value.series.id })
    input.value = ''
  }
}

async function handleDeleteSeries(): Promise<void> {
  if (!detail.value) return
  const ok = await dialog.confirm({
    title: t('series.deleteConfirmTitle'),
    description: t('series.deleteConfirmDesc'),
    danger: true,
    confirmText: t('common.delete'),
  })
  if (!ok) return
  await api.deleteSeries(detail.value.series.id)
  upload.notifyCatalogChanged()
  router.replace('/comic/series')
}
</script>

<template>
  <div v-if="detail" class="mx-auto max-w-6xl space-y-10 p-6 sm:p-8">
    <!-- Widescreen Hero Section -->
    <section
      class="flex flex-col gap-8 rounded-3xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-6 shadow-sm md:flex-row md:p-8"
    >
      <!-- 3D Book-Shadow Cover -->
      <div class="mx-auto w-48 shrink-0 md:mx-0 md:w-56">
        <div class="book-shadow aspect-[2/3] w-full overflow-hidden rounded-2xl bg-[var(--bg-mantle)]">
          <img
            v-if="detail.cover_url"
            :src="withAuthUrl(detail.cover_url)"
            :alt="detail.series.title"
            class="h-full w-full object-cover"
          />
          <div
            v-else
            class="flex h-full w-full items-center justify-center text-[var(--text-secondary)]"
          >
            <LuBookOpen class="h-12 w-12 opacity-40" />
          </div>
        </div>
      </div>

      <!-- Series Info & Actions -->
      <div class="flex min-w-0 flex-1 flex-col justify-between space-y-5">
        <div class="space-y-3.5">
          <!-- Meta Badges -->
          <div class="flex flex-wrap items-center gap-2 text-xs">
            <span class="rounded-full bg-[var(--accent)]/15 px-3 py-1 font-semibold text-[var(--accent)]">
              {{
                detail.series.status === 'completed'
                  ? t('series.statusCompleted')
                  : detail.series.status === 'hiatus'
                    ? t('series.statusHiatus')
                    : t('series.statusOngoing')
              }}
            </span>
            <span class="rounded-full bg-[var(--bg-mantle)] px-3 py-1 font-medium text-[var(--text-secondary)]">
              {{
                detail.series.reading_direction === 'ltr'
                  ? t('series.directionLtr')
                  : detail.series.reading_direction === 'webtoon'
                    ? t('series.directionWebtoon')
                    : t('series.directionRtl')
              }}
            </span>
            <span
              v-if="detail.series.year"
              class="rounded-full bg-[var(--bg-mantle)] px-3 py-1 font-medium text-[var(--text-secondary)]"
            >
              {{ detail.series.year }}
            </span>
            <span
              v-if="detail.series.age_rating"
              class="rounded-full bg-amber-500/15 px-3 py-1 font-semibold text-amber-600 dark:text-amber-400"
            >
              {{ detail.series.age_rating }}
            </span>
            <span
              v-if="detail.series.publisher"
              class="rounded-full bg-[var(--bg-mantle)] px-3 py-1 font-medium text-[var(--text-secondary)]"
            >
              {{ detail.series.publisher }}
            </span>
          </div>

          <div>
            <h1 class="text-2xl font-extrabold tracking-tight text-[var(--text-primary)] sm:text-3xl">
              {{ detail.series.title }}
            </h1>
            <p
              v-if="detail.series.alt_titles.length > 0"
              class="mt-1 text-xs text-[var(--text-secondary)]"
            >
              {{ detail.series.alt_titles.join(' / ') }}
            </p>
          </div>

          <!-- Clickable Writer & Penciller Filter Pills -->
          <div
            v-if="detail.series.writers.length > 0 || detail.series.pencillers.length > 0"
            class="flex flex-wrap items-center gap-2"
          >
            <button
              v-for="w in detail.series.writers"
              :key="'w-' + w"
              type="button"
              class="rounded-full border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-1 text-xs font-medium text-[var(--text-primary)] transition hover:border-[var(--accent)] hover:text-[var(--accent)]"
              @click="router.push({ path: '/comic/series', query: { writer: w } })"
            >
              {{ t('series.writers') }}: {{ w }}
            </button>
            <button
              v-for="p in detail.series.pencillers"
              :key="'p-' + p"
              type="button"
              class="rounded-full border border-[var(--border-subtle)] bg-[var(--bg-base)] px-3 py-1 text-xs font-medium text-[var(--text-primary)] transition hover:border-[var(--accent)] hover:text-[var(--accent)]"
              @click="router.push({ path: '/comic/series', query: { penciller: p } })"
            >
              {{ t('series.pencillers') }}: {{ p }}
            </button>
          </div>

          <!-- Summary -->
          <p class="text-sm leading-relaxed whitespace-pre-line text-[var(--text-secondary)]">
            {{ detail.series.summary || t('series.summaryEmpty') }}
          </p>

          <!-- Genres & Tags -->
          <div
            v-if="detail.series.genres.length > 0 || detail.series.tags.length > 0"
            class="flex flex-wrap gap-1.5 pt-1"
          >
            <button
              v-for="g in detail.series.genres"
              :key="'g-' + g"
              type="button"
              class="rounded-lg bg-[var(--accent)]/12 px-2.5 py-1 text-xs font-medium text-[var(--accent)] transition hover:bg-[var(--accent)]/20"
              @click="router.push({ path: '/comic/series', query: { genre: g } })"
            >
              {{ g }}
            </button>
            <button
              v-for="tag in detail.series.tags"
              :key="'t-' + tag"
              type="button"
              class="rounded-lg bg-[var(--bg-mantle)] px-2.5 py-1 text-xs font-medium text-[var(--text-secondary)] transition hover:text-[var(--text-primary)]"
              @click="router.push({ path: '/comic/series', query: { tag } })"
            >
              #{{ tag }}
            </button>
          </div>
        </div>

        <!-- Primary Capsule Button + Circular Action Buttons -->
        <div class="flex flex-wrap items-center gap-3 pt-2">
          <button
            type="button"
            :disabled="detail.chapters.length === 0"
            class="inline-flex h-11 items-center gap-2.5 rounded-full bg-[var(--accent)] px-6 text-sm font-bold text-[var(--accent-contrast)] shadow-md transition hover:opacity-92 disabled:opacity-40"
            @click="primaryReadAction"
          >
            <LuPlay class="h-4 w-4 fill-current" />
            <span>
              {{
                detail.continue_reading
                  ? t('series.continueChapter', {
                      chapter: detail.continue_reading.chapter_number,
                      page: detail.continue_reading.current_page,
                    })
                  : t('series.startReading')
              }}
            </span>
          </button>

          <!-- Circular Action Buttons -->
          <button
            type="button"
            :title="detail.is_favorite ? t('series.favoriteRemove') : t('series.favoriteAdd')"
            class="flex h-10 w-10 items-center justify-center rounded-full border border-[var(--border-subtle)] bg-[var(--bg-base)] transition hover:border-rose-500/60"
            :class="detail.is_favorite ? 'text-rose-500' : 'text-[var(--text-secondary)]'"
            @click="toggleFavorite"
          >
            <LuHeart class="h-4.5 w-4.5" :class="{ 'fill-current': detail.is_favorite }" />
          </button>

          <button
            type="button"
            :title="t('series.addToShelf')"
            class="flex h-10 w-10 items-center justify-center rounded-full border border-[var(--border-subtle)] bg-[var(--bg-base)] text-[var(--text-secondary)] transition hover:border-[var(--accent)]/60 hover:text-[var(--accent)]"
            @click="openShelfDialog"
          >
            <LuBookmark class="h-4.5 w-4.5" :class="{ 'fill-current text-[var(--accent)]': detail.shelf_ids.length > 0 }" />
          </button>

          <input
            ref="chapterUploadRef"
            type="file"
            multiple
            accept=".cbz,.zip,.epub"
            class="hidden"
            @change="onUploadChaptersToSeries"
          />
          <button
            type="button"
            :title="t('series.uploadToSeries')"
            class="flex h-10 w-10 items-center justify-center rounded-full border border-[var(--border-subtle)] bg-[var(--bg-base)] text-[var(--text-secondary)] transition hover:border-[var(--accent)]/60 hover:text-[var(--text-primary)]"
            @click="chapterUploadRef?.click()"
          >
            <LuUpload class="h-4.5 w-4.5" />
          </button>

          <button
            type="button"
            :title="
              detail.completed_chapters >= detail.series.chapter_count && detail.series.chapter_count > 0
                ? t('series.markAllUnread')
                : t('series.markAllRead')
            "
            class="flex h-10 w-10 items-center justify-center rounded-full border border-[var(--border-subtle)] bg-[var(--bg-base)] text-[var(--text-secondary)] transition hover:border-[var(--accent)]/60 hover:text-[var(--text-primary)]"
            @click="
              handleMarkSeriesRead(
                !(detail.completed_chapters >= detail.series.chapter_count && detail.series.chapter_count > 0)
              )
            "
          >
            <LuCheckCheck class="h-4.5 w-4.5" />
          </button>

          <template v-if="auth.isAdmin">
            <button
              type="button"
              :title="t('series.editMetadata')"
              class="flex h-10 w-10 items-center justify-center rounded-full border border-[var(--border-subtle)] bg-[var(--bg-base)] text-[var(--text-secondary)] transition hover:border-[var(--accent)]/60 hover:text-[var(--text-primary)]"
              @click="editSeriesOpen = true"
            >
              <LuPencil class="h-4 w-4" />
            </button>

            <input
              ref="coverInputRef"
              type="file"
              accept="image/*"
              class="hidden"
              @change="onPickCover"
            />
            <button
              type="button"
              :title="t('series.changeCover')"
              class="flex h-10 w-10 items-center justify-center rounded-full border border-[var(--border-subtle)] bg-[var(--bg-base)] text-[var(--text-secondary)] transition hover:border-[var(--accent)]/60 hover:text-[var(--text-primary)]"
              @click="coverInputRef?.click()"
            >
              <LuImage class="h-4 w-4" />
            </button>

            <button
              type="button"
              :title="t('series.deleteSeries')"
              class="flex h-10 w-10 items-center justify-center rounded-full border border-[var(--border-subtle)] bg-[var(--bg-base)] text-[var(--text-secondary)] transition hover:border-red-500/60 hover:text-red-500"
              @click="handleDeleteSeries"
            >
              <LuTrash2 class="h-4 w-4" />
            </button>
          </template>
        </div>
      </div>
    </section>

    <!-- Volumes / Chapters List Section -->
    <section class="space-y-4">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="flex items-center gap-3">
          <h2 class="text-lg font-bold text-[var(--text-primary)]">
            {{ t('series.chaptersHeader') }}
          </h2>
          <span class="rounded-full bg-[var(--bg-mantle)] px-2.5 py-0.5 text-xs font-semibold text-[var(--text-secondary)]">
            {{ detail.completed_chapters }} / {{ detail.series.chapter_count }}
          </span>
        </div>

        <div class="flex items-center gap-4">
          <AppCheckbox
            v-if="hasAnyVolumes"
            v-model="groupByVolume"
            :label="t('series.groupByVolume')"
          />

          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-1.5 text-xs font-medium text-[var(--text-primary)] transition hover:border-[var(--accent)]/60"
            @click="orderAsc = !orderAsc"
          >
            <LuArrowUpDown class="h-3.5 w-3.5" />
            <span>{{ orderAsc ? t('series.orderAsc') : t('series.orderDesc') }}</span>
          </button>
        </div>
      </div>

      <!-- Volume Grouped Mode -->
      <div v-if="hasAnyVolumes && groupByVolume" class="space-y-4">
        <div
          v-for="group in volumeGroups"
          :key="group.key"
          class="overflow-hidden rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)]"
        >
          <div
            class="flex cursor-pointer items-center justify-between bg-[var(--bg-mantle)]/65 px-4 py-3"
            @click="toggleVolumeCollapse(group.key)"
          >
            <div class="flex items-center gap-2 text-sm font-bold text-[var(--text-primary)]">
              <LuChevronRight v-if="collapsedVolumes[group.key]" class="h-4 w-4" />
              <LuChevronDown v-else class="h-4 w-4" />
              <span>{{ group.title }}</span>
              <span class="text-xs font-normal text-[var(--text-secondary)]">
                ({{ group.chapters.length }})
              </span>
            </div>
            <button
              v-if="group.volumeNumber !== null"
              type="button"
              class="rounded-lg px-2.5 py-1 text-xs font-medium text-[var(--accent)] hover:bg-[var(--accent)]/12"
              @click.stop="handleMarkVolumeRead(group.volumeNumber)"
            >
              {{ t('series.markVolumeRead') }}
            </button>
          </div>

          <div v-if="!collapsedVolumes[group.key]" class="divide-y divide-[var(--border-subtle)]">
            <div
              v-for="ch in group.chapters"
              :key="ch.id"
              class="group flex items-center justify-between gap-4 px-4 py-3.5 transition hover:bg-[var(--surface-hover)]"
              :class="ch.is_missing ? 'opacity-50' : 'cursor-pointer'"
              @click="openChapter(ch)"
            >
              <div class="min-w-0 flex-1 space-y-1">
                <div class="flex flex-wrap items-center gap-2">
                  <span class="rounded-md bg-[var(--bg-mantle)] px-2 py-0.5 text-xs font-semibold text-[var(--text-primary)]">
                    {{ chapterKindBadge(ch) }}
                  </span>
                  <span
                    class="truncate text-sm font-semibold"
                    :class="
                      ch.is_completed
                        ? 'text-[var(--text-secondary)]'
                        : 'text-[var(--text-primary)] group-hover:text-[var(--accent)]'
                    "
                  >
                    {{ ch.title }}
                  </span>
                  <span
                    v-if="ch.is_completed"
                    class="rounded-md bg-emerald-500/15 px-2 py-0.5 text-[10px] font-semibold text-emerald-500"
                  >
                    {{ t('series.completedBadge') }}
                  </span>
                  <span
                    v-if="ch.is_missing"
                    class="rounded-md bg-red-500/15 px-2 py-0.5 text-[10px] font-semibold text-red-500"
                  >
                    {{ t('series.missingBadge') }}
                  </span>
                </div>

                <div class="flex items-center gap-3 text-xs text-[var(--text-secondary)]">
                  <span>{{ t('series.pagesCount', { count: ch.page_count }) }}</span>
                  <span v-if="ch.current_page > 0 && !ch.is_completed" class="text-[var(--accent)]">
                    {{ t('series.readProgress', { page: ch.current_page, total: ch.page_count }) }}
                  </span>
                  <div
                    v-if="ch.current_page > 0 || ch.is_completed"
                    class="h-1 w-24 overflow-hidden rounded-full bg-[var(--bg-mantle)]"
                  >
                    <div
                      class="h-full rounded-full bg-[var(--accent)]"
                      :style="{
                        width:
                          (ch.is_completed
                            ? 100
                            : Math.min(100, Math.round((ch.current_page / Math.max(1, ch.page_count)) * 100))) + '%',
                      }"
                    />
                  </div>
                </div>
              </div>

              <!-- Row Action Buttons -->
              <div class="flex shrink-0 items-center gap-1.5" @click.stop>
                <button
                  type="button"
                  :title="ch.is_completed ? t('series.markUnread') : t('series.markRead')"
                  class="rounded-lg p-2 transition hover:bg-[var(--bg-mantle)]"
                  :class="ch.is_completed ? 'text-emerald-500' : 'text-[var(--text-secondary)]'"
                  @click="toggleChapterRead(ch)"
                >
                  <LuCheck class="h-4 w-4" />
                </button>
                <button
                  type="button"
                  :title="t('series.markPrevRead')"
                  class="rounded-lg p-2 text-[var(--text-secondary)] transition hover:bg-[var(--bg-mantle)] hover:text-[var(--accent)]"
                  @click="handleMarkPreviousRead(ch)"
                >
                  <LuCheckCheck class="h-4 w-4" />
                </button>
                <button
                  v-if="auth.isAdmin"
                  type="button"
                  :title="t('series.editChapter')"
                  class="rounded-lg p-2 text-[var(--text-secondary)] transition hover:bg-[var(--bg-mantle)] hover:text-[var(--text-primary)]"
                  @click="editingChapter = ch"
                >
                  <LuPencil class="h-3.5 w-3.5" />
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Flat Chapter List Mode -->
      <div
        v-else
        class="divide-y divide-[var(--border-subtle)] overflow-hidden rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)]"
      >
        <div
          v-for="ch in sortedChapters"
          :key="ch.id"
          class="group flex items-center justify-between gap-4 px-5 py-3.5 transition hover:bg-[var(--surface-hover)]"
          :class="ch.is_missing ? 'opacity-50' : 'cursor-pointer'"
          @click="openChapter(ch)"
        >
          <div class="min-w-0 flex-1 space-y-1">
            <div class="flex flex-wrap items-center gap-2">
              <span class="rounded-md bg-[var(--bg-mantle)] px-2 py-0.5 text-xs font-semibold text-[var(--text-primary)]">
                {{ chapterKindBadge(ch) }}
              </span>
              <span
                class="truncate text-sm font-semibold"
                :class="
                  ch.is_completed
                    ? 'text-[var(--text-secondary)]'
                    : 'text-[var(--text-primary)] group-hover:text-[var(--accent)]'
                "
              >
                {{ ch.title }}
              </span>
              <span
                v-if="ch.is_completed"
                class="rounded-md bg-emerald-500/15 px-2 py-0.5 text-[10px] font-semibold text-emerald-500"
              >
                {{ t('series.completedBadge') }}
              </span>
              <span
                v-if="ch.is_missing"
                class="rounded-md bg-red-500/15 px-2 py-0.5 text-[10px] font-semibold text-red-500"
              >
                {{ t('series.missingBadge') }}
              </span>
            </div>

            <div class="flex items-center gap-3 text-xs text-[var(--text-secondary)]">
              <span>{{ t('series.pagesCount', { count: ch.page_count }) }}</span>
              <span v-if="ch.current_page > 0 && !ch.is_completed" class="text-[var(--accent)]">
                {{ t('series.readProgress', { page: ch.current_page, total: ch.page_count }) }}
              </span>
              <div
                v-if="ch.current_page > 0 || ch.is_completed"
                class="h-1 w-24 overflow-hidden rounded-full bg-[var(--bg-mantle)]"
              >
                <div
                  class="h-full rounded-full bg-[var(--accent)]"
                  :style="{
                    width:
                      (ch.is_completed
                        ? 100
                        : Math.min(100, Math.round((ch.current_page / Math.max(1, ch.page_count)) * 100))) + '%',
                  }"
                />
              </div>
            </div>
          </div>

          <div class="flex shrink-0 items-center gap-1.5" @click.stop>
            <button
              type="button"
              :title="ch.is_completed ? t('series.markUnread') : t('series.markRead')"
              class="rounded-lg p-2 transition hover:bg-[var(--bg-mantle)]"
              :class="ch.is_completed ? 'text-emerald-500' : 'text-[var(--text-secondary)]'"
              @click="toggleChapterRead(ch)"
            >
              <LuCheck class="h-4 w-4" />
            </button>
            <button
              type="button"
              :title="t('series.markPrevRead')"
              class="rounded-lg p-2 text-[var(--text-secondary)] transition hover:bg-[var(--bg-mantle)] hover:text-[var(--accent)]"
              @click="handleMarkPreviousRead(ch)"
            >
              <LuCheckCheck class="h-4 w-4" />
            </button>
            <button
              v-if="auth.isAdmin"
              type="button"
              :title="t('series.editChapter')"
              class="rounded-lg p-2 text-[var(--text-secondary)] transition hover:bg-[var(--bg-mantle)] hover:text-[var(--text-primary)]"
              @click="editingChapter = ch"
            >
              <LuPencil class="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </div>
    </section>

    <!-- Modals -->
    <SeriesMetadataModal
      :open="editSeriesOpen"
      :series="detail.series"
      @close="editSeriesOpen = false"
      @saved="onSeriesMetadataSaved"
    />

    <ChapterMetadataModal
      :open="Boolean(editingChapter)"
      :chapter="editingChapter"
      @close="editingChapter = null"
      @saved="loadDetail"
    />

    <AppDialog
      :open="shelfModalOpen"
      :title="t('shelves.selectShelvesTitle')"
      max-width-class="max-w-md"
      @close="shelfModalOpen = false"
    >
      <div class="space-y-3">
        <p v-if="allShelves.length === 0" class="text-sm text-[var(--text-secondary)]">
          {{ t('shelves.emptyShelves') }}
        </p>
        <div
          v-for="sh in allShelves"
          :key="sh.shelf.id"
          class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-base)]/60 p-3"
        >
          <AppCheckbox
            :model-value="selectedShelfIds.includes(sh.shelf.id)"
            :label="sh.shelf.name"
            :description="sh.shelf.description || undefined"
            @update:model-value="setShelfChecked(sh.shelf.id, $event)"
          />
        </div>
      </div>
      <template #footer>
        <button
          type="button"
          class="rounded-xl border border-[var(--border-subtle)] px-4 py-2 text-sm text-[var(--text-secondary)]"
          @click="shelfModalOpen = false"
        >
          {{ t('common.cancel') }}
        </button>
        <button
          type="button"
          class="rounded-xl bg-[var(--accent)] px-4 py-2 text-sm font-semibold text-[var(--accent-contrast)]"
          @click="saveShelvesSelection"
        >
          {{ t('common.save') }}
        </button>
      </template>
    </AppDialog>
  </div>
</template>
