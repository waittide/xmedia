<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { LuCheck, LuImage, LuLayers, LuZap } from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import { resolveScraperCoverUrl } from '../../api'
import type { ScrapeCandidate } from '../../types'

defineProps<{
  candidate: ScrapeCandidate
  selected: boolean
  loadingDetail: boolean
  isRepresentative: boolean
  providerCandidateCount: number
  coverUsable: boolean
}>()

const emit = defineEmits<{
  'select-preview': [candidate: ScrapeCandidate]
  'use-for-merge': [candidate: ScrapeCandidate]
  'direct-apply': [candidate: ScrapeCandidate]
  'mark-cover-broken': [url: string | null | undefined]
}>()

const { t } = useI18n()
</script>

<template>
  <!-- Non-Interactive Card Container + Sibling Preview Trigger & Action Buttons -->
  <div
    class="rounded-corner-lg p-2.5 transition"
    :class="
      selected
        ? 'bg-primary-container/35 ring-1 ring-primary'
        : 'bg-surface hover:bg-surface-container-high/55'
    "
  >
    <button
      type="button"
      class="md-state flex w-full cursor-pointer items-start gap-2.5 rounded-corner-md p-1 text-left outline-none focus-visible:outline-2 focus-visible:outline-secondary"
      @click="emit('select-preview', candidate)"
    >
      <div
        class="relative h-14 w-10 shrink-0 overflow-hidden rounded-corner-xs bg-surface-container-high"
      >
        <img
          v-if="coverUsable"
          :src="resolveScraperCoverUrl(candidate.cover_url)"
          alt=""
          loading="lazy"
          referrerpolicy="no-referrer"
          class="h-full w-full object-cover"
          @error="emit('mark-cover-broken', candidate.cover_url)"
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
        <div class="flex items-start justify-between gap-1.5">
          <span class="text-title-small line-clamp-2 text-on-surface">
            {{ candidate.title }}
          </span>
          <span
            class="text-label-small shrink-0 rounded-corner-full px-2 py-0.5"
            :class="
              selected
                ? 'bg-primary text-on-primary'
                : 'bg-surface-container-high text-on-surface-variant'
            "
          >
            {{ candidate.provider_name }}
          </span>
        </div>

        <div class="text-body-small mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-on-surface-variant">
          <span v-if="candidate.subtitle" class="truncate">{{ candidate.subtitle }}</span>
          <span v-if="candidate.year != null" class="font-mono">{{ candidate.year }}</span>
        </div>
      </div>
    </button>

    <div class="mt-1.5 flex flex-wrap items-center gap-1.5 pl-12">
      <MdButton
        v-if="!isRepresentative"
        variant="tonal"
        size="sm"
        :disabled="loadingDetail"
        :icon="LuLayers"
        @click="emit('use-for-merge', candidate)"
      >
        {{ t('tool.workbench.merged.useForMerge') }}
      </MdButton>
      <span
        v-else-if="providerCandidateCount > 1"
        class="text-label-small inline-flex items-center gap-0.5 rounded-corner-full bg-primary-container px-2 py-1 text-on-primary-container"
      >
        <LuCheck class="h-3 w-3" aria-hidden="true" />
        <span>{{ t('tool.workbench.merged.merging') }}</span>
      </span>

      <MdButton
        variant="outlined"
        size="sm"
        :disabled="loadingDetail"
        :icon="LuZap"
        @click="emit('direct-apply', candidate)"
      >
        {{ t('tool.workbench.merged.directWrite') }}
      </MdButton>
    </div>
  </div>
</template>
