<script setup lang="ts">
import {
  LuArrowDown,
  LuArrowUp,
  LuBan,
  LuImage,
  LuLayers,
  LuSlidersHorizontal,
  LuZap,
} from 'vue-icons-plus/lu'
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

function isCoverUsable(
  url: string | null | undefined,
  broken: Set<string>,
): boolean {
  const trimmed = url?.trim()
  return Boolean(trimmed && !broken.has(trimmed))
}
</script>

<template>
  <!-- All-Muted Fallback Banner -->
  <div
    v-if="!mergedMeta && ignoredProviderIds.size > 0"
    class="flex items-center justify-between gap-2 rounded-2xl bg-[var(--md-sys-color-warning-container)] px-3 py-2 text-[11px] text-[var(--md-sys-color-on-warning-container)]"
  >
    <span>已临时屏蔽全部命中源（{{ ignoredProviderIds.size }} 个）</span>
    <button
      type="button"
      class="md-state rounded-full border border-current/35 px-2 py-0.5 text-[10px] font-semibold"
      @click="emit('clear-ignored')"
    >
      恢复全部源
    </button>
  </div>

  <!-- Multi-source Merged Quick Card -->
  <div
    v-if="mergedMeta"
    class="cursor-pointer rounded-2xl border p-2.5 transition"
    :class="
      selected
        ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)]/50 ring-1 ring-[var(--md-sys-color-primary)]/45'
        : 'border-[var(--md-sys-color-primary)]/40 bg-[var(--md-sys-color-primary-container)]/25 hover:border-[var(--md-sys-color-primary)]/75'
    "
    @click="emit('select-merged')"
  >
    <div class="flex items-center justify-between gap-2">
      <div class="flex items-center gap-1.5 text-[11px] font-bold text-[var(--md-sys-color-primary)]">
        <LuLayers class="h-3.5 w-3.5" />
        <span>多源瀑布聚合结果</span>
        <span
          class="rounded-full bg-[var(--md-sys-color-surface)]/85 px-1.5 py-0.5 text-[9px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ countPopulatedFields(mergedMeta) }} 项字段
        </span>
      </div>
      <span
        v-if="selected"
        class="rounded-full bg-[var(--md-sys-color-primary)] px-1.5 py-0.5 text-[9px] font-semibold text-[var(--md-sys-color-on-primary)]"
      >
        预览中
      </span>
    </div>

    <div class="mt-2 flex items-start gap-2.5">
      <button
        type="button"
        class="relative h-14 w-10 shrink-0 overflow-hidden rounded-lg border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface)]"
        title="在右侧预览多源聚合结果"
        @click.stop="emit('select-merged')"
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
          class="flex h-full w-full flex-col items-center justify-center gap-0.5 text-[var(--md-sys-color-on-surface-variant)]/55"
        >
          <LuImage class="h-3.5 w-3.5" />
          <span class="text-[8px]">无图</span>
        </div>
      </button>

      <div class="min-w-0 flex-1">
        <div class="line-clamp-2 text-xs font-bold text-[var(--md-sys-color-on-surface)]">
          {{ mergedMeta.title || '—' }}
        </div>
        <div class="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
          <span v-if="mergedMeta.creators.length > 0" class="truncate">
            {{ mergedMeta.creators.join(' / ') }}
          </span>
          <span v-if="mergedMeta.publisher_or_studio" class="truncate opacity-80">
            {{ mergedMeta.publisher_or_studio }}
          </span>
          <span v-if="mergedMeta.published_date || mergedMeta.year" class="font-mono">
            {{ mergedMeta.published_date || mergedMeta.year }}
          </span>
        </div>

        <div class="mt-2 flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            class="md-state inline-flex items-center gap-1 rounded-full border px-2.5 py-0.5 text-[10px] font-medium transition"
            :class="
              waterfallExpanded || ignoredProviderIds.size > 0
                ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                : 'border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
            "
            title="调整各源优先级、临时屏蔽干扰源或切换单源聚合代表"
            @click.stop="emit('toggle-waterfall')"
          >
            <LuSlidersHorizontal class="h-3 w-3" />
            <span>
              调源{{ ignoredProviderIds.size > 0 ? ` (-${ignoredProviderIds.size})` : '' }}
            </span>
          </button>

          <button
            type="button"
            class="md-state inline-flex items-center gap-1 rounded-full border border-[var(--md-sys-color-primary)]/55 bg-[var(--md-sys-color-surface)] px-2.5 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-primary)] transition hover:bg-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-on-primary)]"
            title="直接将聚合结果写入文件并保存"
            @click.stop="emit('direct-apply', mergedMeta)"
          >
            <LuZap class="h-3 w-3" />
            <span>直写</span>
          </button>
        </div>
      </div>
    </div>

    <!-- Interactive Multi-Source Waterfall Priority & Mute Drawer -->
    <div
      v-if="waterfallExpanded"
      class="mt-2.5 space-y-1.5 border-t border-[var(--md-sys-color-primary)]/25 pt-2"
      @click.stop
    >
      <div class="flex items-center justify-between text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
        <span>按顺序互补填充，可上下调权、屏蔽干扰源或换选候选：</span>
        <button
          v-if="ignoredProviderIds.size > 0"
          type="button"
          class="shrink-0 font-semibold text-[var(--md-sys-color-primary)] hover:underline"
          @click="emit('clear-ignored')"
        >
          全部恢复
        </button>
      </div>
      <div class="space-y-1">
        <div
          v-for="(brief, idx) in orderedActiveBriefs"
          :key="brief.id"
          class="flex items-center justify-between gap-1.5 rounded-xl border px-2 py-1.5 text-[10px] transition"
          :class="
            ignoredProviderIds.has(brief.id)
              ? 'border-dashed border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] opacity-55'
              : 'border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface)]'
          "
        >
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-1">
              <span class="font-mono text-[9px] font-bold text-[var(--md-sys-color-primary)]">
                #{{ idx + 1 }}
              </span>
              <span
                class="font-semibold text-[var(--md-sys-color-on-surface)]"
                :class="ignoredProviderIds.has(brief.id) ? 'line-through' : ''"
              >
                {{ brief.name }}
              </span>
            </div>
            <div class="truncate text-[9px] text-[var(--md-sys-color-on-surface-variant)]">
              {{
                selectedByProvider[brief.id]?.title ||
                detailsByProvider[brief.id]?.title ||
                '未命中'
              }}
            </div>
          </div>

          <div class="flex shrink-0 items-center gap-0.5">
            <button
              v-if="(providerCandidateCounts[brief.id] ?? 0) > 1"
              type="button"
              class="rounded-full border border-[var(--md-sys-color-outline-variant)] px-1.5 py-0.5 text-[9px] font-medium text-[var(--md-sys-color-on-surface-variant)] hover:border-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-primary)]"
              title="筛选此源下的所有候选以便换选聚合条目"
              @click="emit('filter-source', activeSourceFilter === brief.id ? 'all' : brief.id)"
            >
              换选({{ providerCandidateCounts[brief.id] }})
            </button>
            <button
              type="button"
              class="rounded-full p-1 transition"
              :class="
                ignoredProviderIds.has(brief.id)
                  ? 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
                  : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]'
              "
              :title="ignoredProviderIds.has(brief.id) ? '恢复此源参与聚合' : '临时屏蔽此源'"
              @click="emit('toggle-ignored', brief.id)"
            >
              <LuBan class="h-3 w-3" />
            </button>
            <button
              type="button"
              :disabled="idx === 0"
              class="rounded-full p-1 text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)] disabled:opacity-30"
              title="提升优先级"
              @click="emit('move-order', idx, -1)"
            >
              <LuArrowUp class="h-3 w-3" />
            </button>
            <button
              type="button"
              :disabled="idx === orderedActiveBriefs.length - 1"
              class="rounded-full p-1 text-[var(--md-sys-color-on-surface-variant)] transition hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)] disabled:opacity-30"
              title="降低优先级"
              @click="emit('move-order', idx, 1)"
            >
              <LuArrowDown class="h-3 w-3" />
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
