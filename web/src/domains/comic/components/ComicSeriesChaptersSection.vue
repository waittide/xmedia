<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuArrowUpDown,
  LuBookOpen,
  LuCheck,
  LuCheckCheck,
  LuChevronDown,
  LuChevronRight,
  LuDownload,
  LuPencil,
  LuTrash2,
} from 'vue-icons-plus/lu'
import {
  AppCheckbox,
  MdButton,
  MdCard,
  MdEmptyState,
  MdMenuItem,
  MdTextField,
} from '@/components/ui'
import ComicChapterRow from '@/domains/comic/components/ComicChapterRow.vue'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import type { ChapterItemDto, SeriesDetailDto } from '@/domains/comic/types'

const props = defineProps<{
  detail: SeriesDetailDto
  isAdmin: boolean
  cachingChapterIds: Set<string>
}>()

const emit = defineEmits<{
  'open-chapter': [chapter: ChapterItemDto]
  'cache-chapter': [chapter: ChapterItemDto]
  'toggle-chapter-read': [chapter: ChapterItemDto]
  'mark-prev-read': [chapter: ChapterItemDto]
  'mark-volume-read': [volumeNumber: number]
  'edit-chapter': [chapter: ChapterItemDto]
  'delete-chapter': [chapter: ChapterItemDto]
}>()

const { t } = useI18n()

const orderAsc = ref(true)
const groupByVolume = ref(true)
const chapterQuery = ref('')
const collapsedVolumes = ref<Record<string, boolean>>({})

watch(
  () => props.detail.series.id,
  () => {
    collapsedVolumes.value = {}
    chapterQuery.value = ''
  },
)

function chapterKindBadge(ch: ChapterItemDto): string {
  if (ch.kind === 'volume') return t('series.kindVolume', { num: ch.number_label })
  if (ch.kind === 'special') return t('series.kindSpecial', { num: ch.number_label })
  return t('series.kindChapter', { num: ch.number_label })
}

function chapterMetaText(ch: ChapterItemDto): string {
  if (ch.online_tracking) {
    if (ch.online_tracking.cached) {
      return `${t('series.pagesCount', { count: ch.page_count })} · ${t('online.trackedCached')}`
    }
    return t('online.trackedUncached')
  }
  return t('series.pagesCount', { count: ch.page_count })
}

function chapterProgressPercent(ch: ChapterItemDto): number | null {
  if (ch.is_completed) return 100
  if (ch.current_page <= 0 || ch.page_count <= 0) return null
  return Math.min(100, Math.round((ch.current_page / ch.page_count) * 100))
}

const filteredSortedChapters = computed<ChapterItemDto[]>(() => {
  const q = chapterQuery.value.trim().toLowerCase()
  const base = orderAsc.value ? [...props.detail.chapters] : [...props.detail.chapters].reverse()
  if (!q) return base
  return base.filter(
    (ch) =>
      ch.title.toLowerCase().includes(q) ||
      ch.number_label.toLowerCase().includes(q) ||
      chapterKindBadge(ch).toLowerCase().includes(q),
  )
})

const hasAnyVolumes = computed(() =>
  props.detail.chapters.some((c) => c.volume_number !== null),
)

interface VolumeGroup {
  key: string
  volumeNumber: number | null
  title: string
  chapters: ChapterItemDto[]
}

const volumeGroups = computed<VolumeGroup[]>(() => {
  const groups = new Map<string, VolumeGroup>()
  for (const ch of filteredSortedChapters.value) {
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

const displayChapterGroups = computed<Array<VolumeGroup & { showHeader: boolean }>>(() => {
  if (hasAnyVolumes.value && groupByVolume.value) {
    return volumeGroups.value.map((g) => ({ ...g, showHeader: true }))
  }
  return [
    {
      key: '__flat__',
      volumeNumber: null,
      title: '',
      chapters: filteredSortedChapters.value,
      showHeader: false,
    },
  ]
})

function toggleVolumeCollapse(key: string): void {
  collapsedVolumes.value[key] = !collapsedVolumes.value[key]
}
</script>

<template>
  <section class="space-y-4">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="flex items-center gap-2.5">
        <h2 class="text-title-large text-on-surface">
          {{ t('series.chaptersHeader') }}
        </h2>
        <span
          class="text-label-medium rounded-corner-full bg-surface-container-high px-2.5 py-0.5 tabular-nums text-on-surface-variant"
        >
          {{ detail.completed_chapters }} / {{ detail.series.chapter_count }}
        </span>
      </div>

      <div class="flex flex-wrap items-center gap-2.5">
        <div v-if="detail.chapters.length > 10" class="w-48">
          <MdTextField
            v-model="chapterQuery"
            type="search"
            size="sm"
            clearable
            :placeholder="t('series.filterChaptersPlaceholder')"
          />
        </div>

        <AppCheckbox
          v-if="hasAnyVolumes"
          v-model="groupByVolume"
          :label="t('series.groupByVolume')"
        />

        <MdButton
          variant="outlined"
          :icon="LuArrowUpDown"
          @click="orderAsc = !orderAsc"
        >
          {{ orderAsc ? t('series.orderAsc') : t('series.orderDesc') }}
        </MdButton>
      </div>
    </div>

    <MdEmptyState
      v-if="filteredSortedChapters.length === 0"
      compact
      :icon="LuBookOpen"
      :title="t('series.noMatchingChapters')"
    />

    <div v-else class="space-y-3">
      <MdCard
        v-for="group in displayChapterGroups"
        :key="group.key"
        variant="filled"
        padding="none"
        class="overflow-hidden"
      >
        <div
          v-if="group.showHeader"
          class="flex items-center justify-between gap-2 bg-surface-container-high/60 px-4 py-2"
        >
          <button
            type="button"
            :aria-expanded="!collapsedVolumes[group.key]"
            class="md-state text-title-small flex min-w-0 flex-1 cursor-pointer items-center gap-2 rounded-corner-sm px-1.5 py-1.5 text-left text-on-surface"
            @click="toggleVolumeCollapse(group.key)"
          >
            <LuChevronRight
              v-if="collapsedVolumes[group.key]"
              class="h-4 w-4 shrink-0"
              aria-hidden="true"
            />
            <LuChevronDown v-else class="h-4 w-4 shrink-0" aria-hidden="true" />
            <span class="truncate">{{ group.title }}</span>
            <span class="text-body-small shrink-0 tabular-nums text-on-surface-variant">
              ({{ group.chapters.length }})
            </span>
          </button>

          <MdButton
            v-if="group.volumeNumber !== null"
            variant="text"
            @click="emit('mark-volume-read', group.volumeNumber)"
          >
            {{ t('series.markVolumeRead') }}
          </MdButton>
        </div>

        <div
          v-if="!group.showHeader || !collapsedVolumes[group.key]"
          class="divide-y divide-outline-variant/50"
        >
          <ComicChapterRow
            v-for="ch in group.chapters"
            :key="ch.id"
            :badge="chapterKindBadge(ch)"
            :title="ch.title || chapterKindBadge(ch)"
            :meta-text="chapterMetaText(ch)"
            :progress-text="
              ch.current_page > 0 && !ch.is_completed
                ? t('series.readProgress', { page: ch.current_page, total: ch.page_count })
                : ''
            "
            :progress-percent="chapterProgressPercent(ch)"
            :completed="ch.is_completed"
            :completed-label="t('series.completedBadge')"
            :missing="ch.is_missing"
            :missing-label="t('series.missingBadge')"
            :active="detail.continue_reading?.chapter_id === ch.id"
            :menu-aria-label="t('series.chapterActions')"
            @open="emit('open-chapter', ch)"
          >
            <template #menu="{ close }">
              <MdMenuItem
                v-if="ch.online_tracking && !ch.online_tracking.cached"
                :label="t('online.cacheChapterBtn')"
                :icon="LuDownload"
                :disabled="cachingChapterIds.has(ch.id)"
                @select="
                  close();
                  emit('cache-chapter', ch)
                "
              />
              <MdMenuItem
                :label="ch.is_completed ? t('series.markUnread') : t('series.markRead')"
                :icon="LuCheck"
                @select="
                  close();
                  emit('toggle-chapter-read', ch)
                "
              />
              <MdMenuItem
                :label="t('series.markPrevRead')"
                :icon="LuCheckCheck"
                @select="
                  close();
                  emit('mark-prev-read', ch)
                "
              />
              <MdMenuItem
                v-if="isAdmin"
                :label="t('series.editChapter')"
                :icon="LuPencil"
                @select="
                  close();
                  emit('edit-chapter', ch)
                "
              />
              <MdMenuItem
                v-if="isAdmin"
                :label="t('series.deleteChapter')"
                :icon="LuTrash2"
                destructive
                @select="
                  close();
                  emit('delete-chapter', ch)
                "
              />
            </template>
          </ComicChapterRow>
        </div>
      </MdCard>
    </div>
  </section>
</template>
