<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuArrowRight,
  LuBookOpen,
  LuCheckCircle2,
  LuClapperboard,
  LuDisc3,
  LuFileSignature,
  LuFileText,
  LuFolderPlus,
  LuImage,
  LuLayers,
  LuSparkles,
} from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdEmptyState from '@/components/ui/MdEmptyState.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdLinearProgress from '@/components/ui/MdLinearProgress.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { formatBytes } from '@/utils/format'
import { buildArtworkUrl } from '../api'
import { isEntryComplete } from '../completeness'
import { useToolInspectorStore } from '../stores/inspector'
import { useToolScrapeStore } from '../stores/scrape'
import { useToolWorkspaceStore } from '../stores/workspace'
import type { MediaFileEntry } from '../types'

const { t } = useI18n()
const workspaceStore = useToolWorkspaceStore()
const inspectorStore = useToolInspectorStore()
const scrapeStore = useToolScrapeStore()

const activeDomain = computed(() => workspaceStore.activeDomain)
const domainRoots = computed(() => workspaceStore.domainRoots)
const activeRootId = computed(() => workspaceStore.activeRootId)
const snapshot = computed(() => workspaceStore.snapshot)
const loadingDir = computed(() => workspaceStore.loadingDir)
const artworkVersion = computed(() => inspectorStore.artworkVersion)
const artworkVersions = computed(() => inspectorStore.fileArtworkVersions)
const stats = computed(() => workspaceStore.stats)
const domainLabels = computed(() => workspaceStore.domainLabels)

const scopeSubtitle = computed(() => {
  if (!activeRootId.value && domainRoots.value.length > 1) {
    return t('tool.home.scopeMultiRoot', { count: domainRoots.value.length })
  }
  const name = snapshot.value?.root_name || domainRoots.value[0]?.name || '—'
  const rel = snapshot.value?.current_rel_path
  return t('tool.home.scopeCurrentRoot', { name: `${name}${rel ? ` / ${rel}` : ''}` })
})

const completionPct = computed(() => {
  if (stats.value.total <= 0) return 0
  return Math.min(100, Math.round((stats.value.complete / stats.value.total) * 100))
})

const incompleteFiles = computed<MediaFileEntry[]>(() =>
  (snapshot.value?.files ?? []).filter((f) => !isEntryComplete(f)),
)

const unorganizedFiles = computed<MediaFileEntry[]>(() =>
  (snapshot.value?.files ?? []).filter((f) => !f.completeness.is_organized),
)

const previewIncomplete = computed<MediaFileEntry[]>(() => incompleteFiles.value.slice(0, 8))
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-6 p-4 sm:p-6">
    <!-- Empty State: No Mounted Directories Configured -->
    <MdEmptyState
      v-if="domainRoots.length === 0"
      :icon="LuFolderPlus"
      :title="domainLabels.emptyRootsTitle"
      :description="domainLabels.emptyRootsDesc"
      class="mx-auto mt-10 max-w-lg rounded-corner-xl bg-surface-container-low"
    >
      <template #action>
        <MdButton variant="filled" :icon="LuFolderPlus" @click="workspaceStore.openSettingsTab('roots')">
          {{ t('tool.roots.addBtn') }}
        </MdButton>
      </template>
    </MdEmptyState>

    <template v-else>
      <!-- Top Header & Actions -->
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 class="text-headline-small text-on-surface">
            {{ t('tool.home.overviewTitle', { domain: t(`tool.domains.${activeDomain}`) }) }}
          </h1>
          <p class="text-body-small mt-0.5 text-on-surface-variant">
            {{ scopeSubtitle }}
          </p>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <RefreshButton size="md" :action="() => workspaceStore.reloadCurrentScope()" :busy="loadingDir" />

          <MdButton
            v-if="incompleteFiles.length > 0"
            variant="filled"
            :icon="LuSparkles"
            :title="t('tool.home.scrapeIncompleteTitle')"
            @click="scrapeStore.enqueueScrapeFiles(incompleteFiles, t('tool.filters.incomplete'))"
          >
            {{ t('tool.home.scrapeIncomplete', { count: incompleteFiles.length }) }}
          </MdButton>

          <MdButton
            v-if="unorganizedFiles.length > 0"
            variant="tonal"
            :icon="LuFileSignature"
            :title="t('tool.home.batchRenameTitle')"
            @click="workspaceStore.openRenameModal(snapshot?.files ?? [])"
          >
            {{ t('tool.home.organizeBtn', { count: unorganizedFiles.length }) }}
          </MdButton>
        </div>
      </div>

      <!-- 4 MD3 Tonal Metric Cards -->
      <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <button
          type="button"
          class="md-card-filled md-state flex cursor-pointer items-center justify-between p-4 text-left"
          @click="workspaceStore.selectCompletenessFilter('all')"
        >
          <div>
            <div class="text-body-small text-on-surface-variant">{{ t('tool.cards.totalFiles') }}</div>
            <div class="text-headline-small mt-1">{{ stats.total }}</div>
          </div>
          <div
            class="flex h-11 w-11 items-center justify-center rounded-corner-lg bg-primary-container text-on-primary-container"
          >
            <LuBookOpen v-if="activeDomain === 'book'" class="h-5 w-5" aria-hidden="true" />
            <LuLayers v-else-if="activeDomain === 'comic'" class="h-5 w-5" aria-hidden="true" />
            <LuDisc3 v-else-if="activeDomain === 'music'" class="h-5 w-5" aria-hidden="true" />
            <LuClapperboard v-else class="h-5 w-5" aria-hidden="true" />
          </div>
        </button>

        <button
          type="button"
          class="md-card-filled md-state flex cursor-pointer items-center justify-between p-4 text-left"
          @click="workspaceStore.selectCompletenessFilter('complete')"
        >
          <div>
            <div class="text-body-small text-on-surface-variant">{{ t('tool.cards.completeFiles') }}</div>
            <div class="mt-1 flex items-baseline gap-1.5">
              <span class="text-headline-small text-success">
                {{ stats.complete }}
              </span>
              <span class="text-label-medium font-mono text-on-surface-variant">{{ completionPct }}%</span>
            </div>
          </div>
          <div
            class="flex h-11 w-11 items-center justify-center rounded-corner-lg bg-success-container text-on-success-container"
          >
            <LuCheckCircle2 class="h-5 w-5" aria-hidden="true" />
          </div>
        </button>

        <button
          type="button"
          class="md-card-filled md-state flex cursor-pointer items-center justify-between p-4 text-left"
          @click="workspaceStore.selectCompletenessFilter('missing_cover')"
        >
          <div>
            <div class="text-body-small text-on-surface-variant">{{ domainLabels.missingCover }}</div>
            <div class="text-headline-small mt-1 text-warning">
              {{ stats.missingCover }}
            </div>
          </div>
          <div
            class="flex h-11 w-11 items-center justify-center rounded-corner-lg bg-warning-container text-on-warning-container"
          >
            <LuImage class="h-5 w-5" aria-hidden="true" />
          </div>
        </button>

        <button
          type="button"
          class="md-card-filled md-state flex cursor-pointer items-center justify-between p-4 text-left"
          @click="workspaceStore.selectCompletenessFilter('missing_summary')"
        >
          <div>
            <div class="text-body-small text-on-surface-variant">{{ domainLabels.missingSummary }}</div>
            <div class="text-headline-small mt-1 text-primary">
              {{ stats.missingSummary }}
            </div>
          </div>
          <div
            class="flex h-11 w-11 items-center justify-center rounded-corner-lg bg-secondary-container text-on-secondary-container"
          >
            <LuFileText class="h-5 w-5" aria-hidden="true" />
          </div>
        </button>
      </div>

      <!-- Metadata Completeness Progress Card -->
      <section class="md-card-filled space-y-2.5 p-5">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex items-center gap-2">
            <span class="text-title-small text-on-surface">{{ t('tool.home.completenessTitle') }}</span>
            <span
              class="text-label-medium rounded-corner-full bg-primary-container px-2.5 py-0.5 font-mono text-on-primary-container"
            >
              {{ completionPct }}%
            </span>
          </div>
          <span class="text-body-small text-on-surface-variant">
            {{
              t('tool.home.completenessSummary', {
                complete: stats.complete,
                incomplete: stats.incomplete,
                missingTag: stats.missingTag,
              })
            }}
          </span>
        </div>
        <MdLinearProgress
          :value="completionPct"
          tone="success"
          :aria-label="t('tool.home.completenessTitle')"
        />
      </section>

      <!-- Pending / Incomplete Items Section -->
      <section class="space-y-3">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2">
            <h2 class="text-title-small text-on-surface">
              {{
                incompleteFiles.length > 0
                  ? t('tool.home.pendingHeading')
                  : t('tool.home.allCompleteHeading')
              }}
            </h2>
            <span
              v-if="incompleteFiles.length > 0"
              class="text-label-small rounded-corner-full bg-warning-container px-2 py-0.5 font-mono text-on-warning-container"
            >
              {{ incompleteFiles.length }}
            </span>
          </div>
          <MdButton
            variant="text"
            @click="workspaceStore.selectCompletenessFilter(incompleteFiles.length > 0 ? 'incomplete' : 'all')"
          >
            <span>{{ t('tool.home.enterItems') }}</span>
            <LuArrowRight class="h-4 w-4" aria-hidden="true" />
          </MdButton>
        </div>

        <MdEmptyState
          v-if="previewIncomplete.length === 0"
          compact
          :icon="stats.total === 0 ? LuFolderPlus : LuCheckCircle2"
          :title="stats.total === 0 ? t('tool.home.emptyNoMedia') : t('tool.home.emptyAllReady')"
          class="rounded-corner-xl bg-surface-container-low"
        />

        <div
          v-else
          class="divide-y divide-outline-variant/35 overflow-hidden rounded-corner-lg-increased bg-surface-container-low"
        >
          <div
            v-for="file in previewIncomplete"
            :key="`${file.root_id}::${file.rel_path}`"
            class="flex items-center gap-3 px-4 py-2.5"
          >
            <button
              type="button"
              class="md-state flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-corner-md py-0.5 text-left"
              @click="workspaceStore.inspectEntry(file)"
            >
              <div
                class="relative flex h-11 w-11 shrink-0 items-center justify-center overflow-hidden rounded-corner-md bg-surface-container-high"
              >
                <img
                  v-if="file.completeness.has_cover"
                  :src="
                    buildArtworkUrl(
                      file.root_id,
                      file.rel_path,
                      file.media_kind,
                      artworkVersion + (artworkVersions?.[`${file.root_id}::${file.rel_path}`] ?? 0),
                      file.media_kind === 'video' ? 'poster' : 'cover',
                    )
                  "
                  :alt="file.display_title"
                  loading="lazy"
                  referrerpolicy="no-referrer"
                  class="h-full w-full object-cover"
                />
                <LuAlertCircle v-else class="h-4 w-4 text-warning" aria-hidden="true" />
              </div>

              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="text-title-small truncate text-on-surface">
                    {{ file.display_title }}
                  </span>
                  <span
                    class="text-label-small shrink-0 rounded-corner-full bg-surface-container-high px-2 py-0.5 font-mono uppercase text-on-surface-variant"
                  >
                    {{ file.extension }}
                  </span>
                </div>
                <div class="text-body-small mt-0.5 truncate text-on-surface-variant">
                  {{ file.display_subtitle }} · {{ formatBytes(file.file_size) }}
                </div>
              </div>
            </button>

            <div class="hidden shrink-0 items-center gap-1.5 sm:flex">
              <span
                v-if="!file.completeness.has_cover"
                class="text-label-small rounded-corner-full bg-warning-container px-2 py-0.5 text-on-warning-container"
              >
                {{ t('tool.home.badgeMissingCover') }}
              </span>
              <span
                v-if="!file.completeness.has_summary_or_lyrics"
                class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
              >
                {{
                  file.media_kind === 'music'
                    ? t('tool.home.badgeMissingLyrics')
                    : t('tool.home.badgeMissingSummary')
                }}
              </span>
              <span
                v-if="!file.completeness.has_nfo_or_tag_block"
                class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
              >
                {{ t('tool.home.badgeMissingTag') }}
              </span>
            </div>

            <MdIconButton
              size="sm"
              :icon="LuSparkles"
              :tooltip="t('tool.home.quickScrapeItem')"
              :aria-label="t('tool.home.quickScrapeItem')"
              @click="scrapeStore.enqueueScrapeFiles([file], file.display_title)"
            />
          </div>
        </div>
      </section>
    </template>
  </div>
</template>
