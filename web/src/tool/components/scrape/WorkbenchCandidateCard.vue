<script setup lang="ts">
import { LuCheck, LuImage, LuLayers, LuZap } from 'vue-icons-plus/lu'
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
</script>

<template>
  <div
    role="button"
    tabindex="0"
    class="cursor-pointer rounded-2xl border p-2.5 transition outline-none focus-visible:outline-2 focus-visible:outline-secondary"
    :class="
      selected
        ? 'border-primary bg-primary-container/35 ring-1 ring-primary/40'
        : 'border-outline-variant/55 bg-surface hover:border-primary/50'
    "
    @click="emit('select-preview', candidate)"
    @keydown.enter.self.prevent="emit('select-preview', candidate)"
    @keydown.space.self.prevent="emit('select-preview', candidate)"
  >
    <div class="flex items-start gap-2.5">
      <button
        type="button"
        class="relative h-14 w-10 shrink-0 overflow-hidden rounded-lg border border-outline-variant/60 bg-surface-container-high"
        title="点击在右侧预览此候选详情"
        :aria-label="`预览 ${candidate.title}`"
        @click.stop="emit('select-preview', candidate)"
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
          <LuImage class="h-3.5 w-3.5" />
          <span class="text-[10px]">无图</span>
        </div>
      </button>

      <div class="min-w-0 flex-1">
        <div class="flex items-start justify-between gap-1.5">
          <button
            type="button"
            class="line-clamp-2 text-left text-xs font-bold text-[var(--md-sys-color-on-surface)] transition hover:text-[var(--md-sys-color-primary)]"
            title="点击在右侧预览此条目详情"
            @click.stop="emit('select-preview', candidate)"
          >
            {{ candidate.title }}
          </button>
          <span
            class="shrink-0 rounded-full px-2 py-0.5 text-[9px] font-semibold"
            :class="
              selected
                ? 'bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)]'
                : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
            "
          >
            {{ candidate.provider_name }}
          </span>
        </div>

        <div class="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
          <span v-if="candidate.subtitle" class="truncate">{{ candidate.subtitle }}</span>
          <span v-if="candidate.year" class="font-mono">{{ candidate.year }}</span>
        </div>

        <div class="mt-2 flex flex-wrap items-center gap-1.5">
          <button
            v-if="!isRepresentative"
            type="button"
            :disabled="loadingDetail"
            class="md-state inline-flex items-center gap-1 rounded-full border border-[var(--md-sys-color-primary)]/45 bg-[var(--md-sys-color-surface)] px-2.5 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-primary)] transition hover:bg-[var(--md-sys-color-primary-container)] disabled:opacity-50"
            title="将此候选设为该数据源的聚合代表，并实时重算多源瀑布聚合结果"
            @click.stop="emit('use-for-merge', candidate)"
          >
            <LuLayers class="h-3 w-3" />
            <span>用于聚合</span>
          </button>
          <span
            v-else-if="providerCandidateCount > 1"
            class="inline-flex items-center gap-0.5 rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[9px] font-semibold text-[var(--md-sys-color-on-primary-container)]"
            title="当前已作为该数据源的聚合代表"
          >
            <LuCheck class="h-2.5 w-2.5" />
            <span>聚合中</span>
          </span>

          <button
            type="button"
            :disabled="loadingDetail"
            class="md-state inline-flex items-center gap-1 rounded-full border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface-container-low)] px-2.5 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)] transition hover:border-[var(--md-sys-color-success)] hover:text-[var(--md-sys-color-success)] disabled:opacity-50"
            title="直接写入磁盘（含封面），无需再点保存"
            @click.stop="emit('direct-apply', candidate)"
          >
            <LuZap class="h-3 w-3" />
            <span>直写</span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
