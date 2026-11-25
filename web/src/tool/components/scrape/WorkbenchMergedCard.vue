<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import {
  LuArrowDown,
  LuArrowUp,
  LuBan,
  LuImage,
  LuLayers,
  LuSlidersHorizontal,
  LuZap,
} from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import { resolveScraperCoverUrl } from '../../api'
import type { ProviderBriefDto, ScrapeCandidate, ScrapedMetadata } from '../../types'
import { countPopulatedFields } from './candidateMetaHelpers'

defineProps<{
  mergedMeta: ScrapedMetadata | null
  selected: boolean
  waterfallExpanded: boolean
  orderedActiveBriefs: ProviderBriefDto[]
  ignoredProviderIds: Set<string>
  selectedByProvider: Record<string, ScrapeCandidate>
  detailsByProvider: Record<string, ScrapedMetadata>
  providerCandidateCounts: Record<string, number>
  activeSourceFilter: string
  brokenCoverUrls: Set<string>
}>()

const emit = defineEmits<{
  'select-merged': []
  'toggle-waterfall': []
  'direct-apply': [meta: ScrapedMetadata]
  'clear-ignored': []
  'toggle-ignored': [providerId: string]
  'move-order': [idx: number, delta: -1 | 1]
  'filter-source': [providerId: string]
  'mark-cover-broken': [url: string | null | undefined]
}>()

const { t } = useI18n()

function isCoverUsable(url: string | null | undefined, broken: Set<string>): boolean {
  const trimmed = url?.trim()
  return Boolean(trimmed && !broken.has(trimmed))
}
</script>

<template>
  <!-- All-Muted Fallback Banner -->
  <div
    v-if="!mergedMeta && ignoredProviderIds.size > 0"
    class="text-body-small flex items-center justify-between gap-2 rounded-corner-lg bg-warning-container px-3 py-2 text-on-warning-container"
  >
    <span>{{ t('tool.workbench.merged.allMutedBanner', { count: ignoredProviderIds.size }) }}</span>
    <MdButton variant="text" @click="emit('clear-ignored')">
      {{ t('tool.workbench.merged.restoreAll') }}
    </MdButton>
  </div>

  <!-- Multi-source Merged Quick Card (Non-Interactive Container + Sibling Preview Trigger & Action Controls) -->
  <div
    v-if="mergedMeta"
    class="rounded-corner-lg p-2.5 transition"
    :class="
      selected
        ? 'bg-primary-container/50 ring-1 ring-primary'
        : 'bg-primary-container/25 hover:bg-primary-container/35'
    "
  >
    <button
      type="button"
      class="md-state block w-full cursor-pointer rounded-corner-md p-1 text-left outline-none focus-visible:outline-2 focus-visible:outline-secondary"
      @click="emit('select-merged')"
    >
      <div class="flex items-center justify-between gap-2">
        <div class="text-label-large flex items-center gap-1.5 text-primary">
          <LuLayers class="h-3.5 w-3.5" aria-hidden="true" />
          <span>{{ t('tool.workbench.merged.title') }}</span>
          <span
            class="text-label-small rounded-corner-full bg-surface/85 px-1.5 py-0.5 text-on-surface-variant"
          >
            {{ t('tool.workbench.merged.fieldsCount', { count: countPopulatedFields(mergedMeta) }) }}
          </span>
        </div>
        <span
          v-if="selected"
          class="text-label-small rounded-corner-full bg-primary px-1.5 py-0.5 text-on-primary"
        >
          {{ t('tool.workbench.merged.previewing') }}
        </span>
      </div>

      <div class="mt-2 flex items-start gap-2.5">
        <div
          class="relative h-14 w-10 shrink-0 overflow-hidden rounded-corner-xs bg-surface"
        >
          <img
            v-if="isCoverUsable(mergedMeta.cover_url, brokenCoverUrls)"
            :src="resolveScraperCoverUrl(mergedMeta.cover_url)"
            alt=""
            loading="lazy"
            referrerpolicy="no-referrer"
            class="h-full w-full object-cover"
            @error="emit('mark-cover-broken', mergedMeta.cover_url)"
          />
          <div
            v-else
            class="flex h-full w-full flex-col items-center justify-center gap-0.5 text-on-surface-variant/75"
          >
            <LuImage class="h-3.5 w-3.5" aria-hidden="true" />
            <span class="text-label-small">{{ t('tool.workbench.merged.noImage') }}</span>
          </div>
        </div>

        <div class="min-w-0 flex-1">
          <div class="text-title-small line-clamp-2 text-on-surface">
            {{ mergedMeta.title || '—' }}
          </div>
          <div class="text-body-small mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-on-surface-variant">
            <span v-if="mergedMeta.creators.length > 0" class="truncate">
              {{ mergedMeta.creators.join(' / ') }}
            </span>
            <span v-if="mergedMeta.publisher_or_studio" class="truncate opacity-80">
              {{ mergedMeta.publisher_or_studio }}
            </span>
            <span v-if="mergedMeta.published_date || mergedMeta.year != null" class="font-mono">
              {{ mergedMeta.published_date || mergedMeta.year }}
            </span>
          </div>
        </div>
      </div>
    </button>

    <!-- Sibling Action Row -->
    <div class="mt-1.5 flex flex-wrap items-center gap-1.5 pl-12">
      <MdButton
        :variant="waterfallExpanded || ignoredProviderIds.size > 0 ? 'tonal' : 'outlined'"
        :icon="LuSlidersHorizontal"
        @click="emit('toggle-waterfall')"
      >
        {{ t('tool.workbench.merged.tuneSources') }}{{ ignoredProviderIds.size > 0 ? ` (-${ignoredProviderIds.size})` : '' }}
      </MdButton>

      <MdButton
        variant="filled"
        :icon="LuZap"
        @click="emit('direct-apply', mergedMeta)"
      >
        {{ t('tool.workbench.merged.directWrite') }}
      </MdButton>
    </div>

    <!-- Interactive Multi-Source Waterfall Priority & Mute Drawer -->
    <div
      v-if="waterfallExpanded"
      class="mt-2.5 space-y-1.5 pt-2"
    >
      <div class="text-label-small flex items-center justify-between text-on-surface-variant">
        <span>{{ t('tool.workbench.merged.waterfallHint') }}</span>
        <MdButton
          v-if="ignoredProviderIds.size > 0"
          variant="text"
          @click="emit('clear-ignored')"
        >
          {{ t('tool.workbench.merged.restoreAll') }}
        </MdButton>
      </div>
      <div class="space-y-1">
        <div
          v-for="(brief, idx) in orderedActiveBriefs"
          :key="brief.id"
          class="text-label-medium flex items-center justify-between gap-1.5 rounded-corner-md px-2.5 py-1.5 transition"
          :class="
            ignoredProviderIds.has(brief.id)
              ? 'bg-surface-container-low opacity-55'
              : 'bg-surface'
          "
        >
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-1">
              <span class="text-label-small font-mono text-primary">
                #{{ idx + 1 }}
              </span>
              <span
                class="text-on-surface"
                :class="ignoredProviderIds.has(brief.id) ? 'line-through' : ''"
              >
                {{ brief.name }}
              </span>
            </div>
            <div class="text-label-small truncate text-on-surface-variant">
              {{
                selectedByProvider[brief.id]?.title ||
                detailsByProvider[brief.id]?.title ||
                t('tool.workbench.merged.unmatched')
              }}
            </div>
          </div>

          <div class="flex shrink-0 items-center gap-0.5">
            <MdButton
              v-if="(providerCandidateCounts[brief.id] ?? 0) > 1"
              variant="text"
              @click="emit('filter-source', activeSourceFilter === brief.id ? 'all' : brief.id)"
            >
              {{ t('tool.workbench.merged.switchCandidate', { count: providerCandidateCounts[brief.id] }) }}
            </MdButton>
            <MdIconButton
              size="sm"
              :selected="ignoredProviderIds.has(brief.id)"
              :icon="LuBan"
              :tooltip="
                ignoredProviderIds.has(brief.id)
                  ? t('tool.workbench.merged.restoreSource')
                  : t('tool.workbench.merged.muteSource')
              "
              :aria-label="
                ignoredProviderIds.has(brief.id)
                  ? t('tool.workbench.merged.restoreSource')
                  : t('tool.workbench.merged.muteSource')
              "
              @click="emit('toggle-ignored', brief.id)"
            />
            <MdIconButton
              size="sm"
              :disabled="idx === 0"
              :icon="LuArrowUp"
              :tooltip="t('tool.workbench.merged.moveUp')"
              :aria-label="t('tool.workbench.merged.moveUp')"
              @click="emit('move-order', idx, -1)"
            />
            <MdIconButton
              size="sm"
              :disabled="idx === orderedActiveBriefs.length - 1"
              :icon="LuArrowDown"
              :tooltip="t('tool.workbench.merged.moveDown')"
              :aria-label="t('tool.workbench.merged.moveDown')"
              @click="emit('move-order', idx, 1)"
            />
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
