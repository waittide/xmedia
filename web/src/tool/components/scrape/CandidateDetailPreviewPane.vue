<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuImage,
  LuImageOff,
  LuLayers,
  LuLoader2,
  LuRotateCcw,
  LuSave,
  LuSkipForward,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import type { MediaKind } from '@/types/api'
import { resolveScraperCoverUrl } from '../../api'
import type { FieldSourceBadge, ScrapedMetadata } from '../../types'
import InspectorBookForm from '../inspector/InspectorBookForm.vue'
import InspectorComicForm from '../inspector/InspectorComicForm.vue'
import InspectorMusicForm from '../inspector/InspectorMusicForm.vue'
import InspectorVideoForm from '../inspector/InspectorVideoForm.vue'
import { useMetadataForm, type ApplyFillMode } from '../inspector/useMetadataForm'

export interface CandidatePreviewState {
  key: string
  isMerged: boolean
  sourceLabel: string
  meta: ScrapedMetadata | null
  loading: boolean
  fieldSources: Record<string, FieldSourceBadge> | null
  fillMode: ApplyFillMode
}

export interface CandidateApplyOptions {
  organizeAfterSave?: boolean
  writeAlbumCover?: boolean
}

const props = withDefaults(
  defineProps<{
    mediaKind: MediaKind
    preview: CandidatePreviewState
    hasNext?: boolean
    closable?: boolean
    saving?: boolean
  }>(),
  {
    hasNext: false,
    closable: true,
    saving: false,
  },
)

const emit = defineEmits<{
  directApply: [meta: ScrapedMetadata, mode: ApplyFillMode, opts?: CandidateApplyOptions]
  directApplyNext: [meta: ScrapedMetadata, mode: ApplyFillMode, opts?: CandidateApplyOptions]
  close: []
}>()

const candidateForm = useMetadataForm()
const brokenCoverUrls = ref<Set<string>>(new Set())
const organizeAfterSave = ref(false)
const writeAlbumCover = ref(false)
const useCover = ref(true)
const baselineUseCover = ref(true)

function hydrateDraft(meta: ScrapedMetadata | null): void {
  candidateForm.hydrateFromScrapedMetadata(props.mediaKind, meta)
  const hasCover = Boolean(meta?.cover_url)
  useCover.value = hasCover
  baselineUseCover.value = hasCover
}

watch(
  () => [props.preview.key, props.preview.meta, props.preview.loading, props.mediaKind] as const,
  ([, meta]) => {
    hydrateDraft(meta)
  },
  { immediate: true },
)

const isDraftEdited = computed(
  () => candidateForm.isDirty.value || useCover.value !== baselineUseCover.value,
)

function resetDraft(): void {
  hydrateDraft(props.preview.meta)
}

function isCoverUsable(url: string | null | undefined): boolean {
  const trimmed = url?.trim()
  return Boolean(trimmed && !brokenCoverUrls.value.has(trimmed))
}

function markCoverBroken(url: string | null | undefined): void {
  const trimmed = url?.trim()
  if (!trimmed || brokenCoverUrls.value.has(trimmed)) return
  const next = new Set(brokenCoverUrls.value)
  next.add(trimmed)
  brokenCoverUrls.value = next
}

function sourceBadgeFor(...keys: string[]): string | null {
  const map = props.preview.fieldSources
  if (!map) return null
  for (const k of keys) {
    const hit = map[k]
    if (hit?.provider_name) return hit.provider_name
  }
  return null
}

const SOURCE_FIELD_LABELS: Record<string, string> = {
  title: '标题',
  original_title: '原名',
  series_or_album: '系列/专辑',
  creators: '作者/艺人',
  illustrators_or_directors: '导演/画师',
  cast: '主演',
  publisher_or_studio: '出品',
  year: '年份',
  published_date: '日期',
  isbn: 'ISBN',
  rating: '评分',
  language: '语言',
  disc_or_season_number: '季/碟',
  track_or_episode_number: '集/轨',
  genres: '流派',
  tags: '标签',
  summary: '简介',
  lyrics: '歌词',
  cover_url: '封面',
}

const sourceBadgesList = computed(() => {
  const map = props.preview.fieldSources
  if (!map) return []
  return Object.entries(map)
    .filter(([, badge]) => Boolean(badge?.provider_name))
    .map(([key, badge]) => ({
      key,
      label: SOURCE_FIELD_LABELS[key] ?? key,
      provider: badge.provider_name,
    }))
})

const displayDraftTitle = computed(() => {
  if (props.mediaKind === 'book') return candidateForm.bookForm.title
  if (props.mediaKind === 'comic') return candidateForm.comicForm.title
  if (props.mediaKind === 'music') return candidateForm.musicForm.title
  return candidateForm.videoForm.title
})

const displayDraftCreators = computed(() => {
  if (props.mediaKind === 'book') return candidateForm.bookForm.authors
  if (props.mediaKind === 'comic') return candidateForm.comicForm.writers
  if (props.mediaKind === 'music') return candidateForm.musicForm.artist
  return candidateForm.videoForm.directors || candidateForm.videoForm.actors
})

function buildEditedMetadata(): ScrapedMetadata | null {
  const base = props.preview.meta
  if (!base) return null
  const includeCover = useCover.value && isCoverUsable(base.cover_url)
  return candidateForm.toScrapedMetadata(props.mediaKind, base, includeCover)
}

function emitDirectApply(advanceNext = false): void {
  const edited = buildEditedMetadata()
  if (!edited) return
  const opts: CandidateApplyOptions = {
    organizeAfterSave: organizeAfterSave.value,
    writeAlbumCover: writeAlbumCover.value,
  }
  if (advanceNext) {
    emit('directApplyNext', edited, props.preview.fillMode, opts)
  } else {
    emit('directApply', edited, props.preview.fillMode, opts)
  }
}

defineExpose({
  triggerSave: emitDirectApply,
  isEdited: isDraftEdited,
})
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-surface-container-low">
    <!-- Preview / Editable Draft Header -->
    <div
      class="flex shrink-0 items-center justify-between gap-2 border-b border-outline-variant/45 px-3.5 py-2.5"
    >
      <div class="flex min-w-0 items-center gap-1.5">
        <span
          class="truncate rounded-corner-full px-2 py-0.5 text-label-small"
          :class="
            preview.isMerged
              ? 'bg-primary-container text-on-primary-container'
              : 'bg-secondary-container text-on-secondary-container'
          "
        >
          {{ preview.sourceLabel }}
        </span>
        <span class="truncate text-label-medium text-on-surface-variant">
          待写入结果（可编辑）
        </span>
        <span
          v-if="isDraftEdited"
          class="shrink-0 rounded-corner-full bg-warning-container px-1.5 py-0.5 text-label-small text-on-warning-container"
        >
          已微调
        </span>
      </div>

      <div class="flex shrink-0 items-center gap-0.5">
        <button
          v-if="isDraftEdited"
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="恢复为此刮削源的原始信息"
          aria-label="恢复为此刮削源的原始信息"
          @click="resetDraft"
        >
          <LuRotateCcw class="h-3.5 w-3.5" />
        </button>

        <button
          v-if="closable"
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="关闭右侧条目预览"
          aria-label="关闭右侧条目预览"
          @click="emit('close')"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>

    <!-- Loading State -->
    <div
      v-if="preview.loading && !preview.meta"
      class="flex flex-1 flex-col items-center justify-center gap-2 px-4 text-body-small text-on-surface-variant"
    >
      <LuLoader2 class="h-5 w-5 animate-spin text-primary" />
      <span>正在拉取该候选源完整条目信息…</span>
    </div>

    <!-- Shared Inspector Domain Form Body -->
    <template v-else-if="preview.meta">
      <div class="flex-1 space-y-3.5 overflow-y-auto px-3.5 pt-3 pb-4">
        <!-- Cover Card -->
        <div class="flex gap-3 rounded-corner-lg bg-surface p-3">
          <div class="flex shrink-0 flex-col items-center gap-1.5">
            <div
              class="relative overflow-hidden rounded-corner-md bg-surface-container-high"
              :class="[
                mediaKind === 'music' ? 'h-24 w-24 album-shadow' : 'h-32 w-24 book-shadow',
                !useCover ? 'opacity-45' : '',
              ]"
            >
              <img
                v-if="useCover && isCoverUsable(preview.meta.cover_url)"
                :src="resolveScraperCoverUrl(preview.meta.cover_url)"
                alt="scraped cover"
                referrerpolicy="no-referrer"
                class="h-full w-full object-cover"
                @error="markCoverBroken(preview.meta.cover_url)"
              />
              <div
                v-else
                class="flex h-full w-full flex-col items-center justify-center gap-1 text-on-surface-variant"
              >
                <LuBookOpen v-if="mediaKind === 'book'" class="h-6 w-6 opacity-50" />
                <LuLayers v-else-if="mediaKind === 'comic'" class="h-6 w-6 opacity-50" />
                <LuDisc3 v-else-if="mediaKind === 'music'" class="h-6 w-6 opacity-50" />
                <LuClapperboard v-else class="h-6 w-6 opacity-50" />
                <span class="text-label-small">
                  {{
                    !useCover
                      ? '已取消封面'
                      : preview.meta.cover_url
                        ? '封面加载失败'
                        : '无封面'
                  }}
                </span>
              </div>
            </div>
            <span
              v-if="useCover && isCoverUsable(preview.meta.cover_url) && sourceBadgeFor('cover_url')"
              class="rounded-corner-full bg-primary-container px-2 py-0.5 text-label-small text-on-primary-container"
            >
              封面 · {{ sourceBadgeFor('cover_url') }}
            </span>
          </div>

          <div class="flex min-w-0 flex-1 flex-col justify-between gap-2">
            <div>
              <div class="line-clamp-2 text-title-small text-on-surface">
                {{ displayDraftTitle || preview.meta.title || '—' }}
              </div>
              <div
                v-if="displayDraftCreators"
                class="mt-1 truncate text-body-small text-on-surface-variant"
              >
                {{ displayDraftCreators }}
              </div>
              <div
                v-if="preview.loading"
                class="mt-1.5 inline-flex items-center gap-1 text-label-small text-primary"
              >
                <LuLoader2 class="h-3 w-3 animate-spin" />
                <span>补全详情中…</span>
              </div>
            </div>

            <div class="flex flex-wrap gap-1.5">
              <button
                v-if="isCoverUsable(preview.meta.cover_url)"
                type="button"
                class="md-chip md-state"
                :class="!useCover ? 'md-chip-selected' : ''"
                @click="useCover = !useCover"
              >
                <LuImageOff v-if="useCover" class="h-3.5 w-3.5" />
                <LuImage v-else class="h-3.5 w-3.5" />
                <span>{{ useCover ? '不用此封面' : '恢复封面' }}</span>
              </button>
            </div>
          </div>
        </div>

        <!-- Waterfall Field Provenance Summary -->
        <div
          v-if="sourceBadgesList.length > 0"
          class="flex flex-wrap items-center gap-1 rounded-corner-md bg-surface px-2.5 py-2"
        >
          <span class="text-label-small text-on-surface-variant">字段来源：</span>
          <span
            v-for="item in sourceBadgesList"
            :key="item.key"
            class="rounded-corner-full bg-surface-container-high px-2 py-0.5 text-label-small text-on-surface"
          >
            {{ item.label }} · <strong class="text-primary">{{ item.provider }}</strong>
          </span>
        </div>

        <!-- Shared Domain Metadata Form -->
        <InspectorMusicForm
          v-if="mediaKind === 'music'"
          :form="candidateForm.musicForm"
          sidecar-lrc-filename="同名 .lrc"
          :file-key="preview.key"
        />
        <InspectorVideoForm
          v-else-if="mediaKind === 'video'"
          :form="candidateForm.videoForm"
          hide-locks
        />
        <InspectorComicForm
          v-else-if="mediaKind === 'comic'"
          :form="candidateForm.comicForm"
        />
        <InspectorBookForm
          v-else
          :form="candidateForm.bookForm"
          book-format="epub"
          :writable="true"
          :is-epub="true"
          :has-cover="Boolean(preview.meta.cover_url)"
          :polishing-book="false"
          hide-locks
        />
      </div>

      <!-- Bottom Action Bar -->
      <div
        class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-t border-outline-variant/45 bg-surface-container/75 px-3.5 py-2.5"
      >
        <div class="flex flex-wrap items-center gap-3 text-label-small text-on-surface-variant">
          <span>
            写入模式：{{ preview.fillMode === 'overwrite' ? '覆盖' : '仅补空' }}
          </span>
          <span title="写入元数据后按设置页的默认命名模板重命名并归档目录">
            <AppCheckbox v-model="organizeAfterSave" label="保存并归档" />
          </span>
          <span
            v-if="mediaKind === 'music' && useCover && isCoverUsable(preview.meta.cover_url)"
            title="写入内嵌封面时同步在专辑目录保存 cover 图片"
          >
            <AppCheckbox v-model="writeAlbumCover" label="同步专辑 cover" />
          </span>
        </div>

        <div class="ml-auto flex items-center gap-1.5">
          <button
            v-if="hasNext"
            type="button"
            :disabled="saving"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            title="写入当前文件并自动切换到下一项 (⌘Enter)"
            @click="emitDirectApply(true)"
          >
            <LuSkipForward class="h-3.5 w-3.5" />
            <span>写入并下一项</span>
          </button>

          <button
            type="button"
            :disabled="saving"
            class="md-btn md-btn-sm md-btn-filled md-state"
            title="直接将此结果写入本地媒体文件 (⌘S)"
            @click="emitDirectApply(false)"
          >
            <LuLoader2 v-if="saving" class="h-3.5 w-3.5 animate-spin" />
            <LuSave v-else class="h-3.5 w-3.5" />
            <span>{{ saving ? '写入中…' : '应用并保存' }}</span>
          </button>
        </div>
      </div>
    </template>
  </div>
</template>
