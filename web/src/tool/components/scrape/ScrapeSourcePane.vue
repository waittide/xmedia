<script setup lang="ts">
import {
  LuArrowDown,
  LuArrowUp,
  LuBan,
  LuCheck,
  LuImage,
  LuRefreshCw,
} from 'vue-icons-plus/lu'
import { resolveScraperCoverUrl } from '../../api'
import type { ProviderBriefDto, ScrapeCandidate } from '../../types'

const props = defineProps<{
  sourceTabs: Array<{ id: string; label: string; count: number }>
  activeSourceTab: string
  searching: boolean
  orderedActiveBriefs: ProviderBriefDto[]
  ignoredProviderIds: Set<string>
  selectedByProvider: Record<string, ScrapeCandidate>
  loadingProviderId: string | null
  candidatesForActiveSource: ScrapeCandidate[]
  brokenCoverUrls: Set<string>
}>()

const emit = defineEmits<{
  switchSourceTab: [tabId: string]
  toggleProviderIgnored: [providerId: string]
  moveProviderOrder: [idx: number, delta: -1 | 1]
  selectCandidate: [cand: ScrapeCandidate]
  markCoverBroken: [url: string | null | undefined]
}>()

function isCoverUsable(url: string | null | undefined): url is string {
  return Boolean(url && !props.brokenCoverUrls.has(url))
}

function candidatesCountFor(providerId: string): number {
  return props.sourceTabs.find((tab) => tab.id === providerId)?.count ?? 0
}
</script>

<template>
  <div class="flex min-h-0 flex-col overflow-hidden md:col-span-5">
  <!-- Tabs: ✦ 多源聚合 | 源1 (n) | 源2 (n) ... -->
  <div
    class="flex shrink-0 items-center gap-1 overflow-x-auto border-b border-[var(--ctp-surface0)]/65 bg-[var(--ctp-mantle)]/30 px-3 py-2"
  >
    <button
      v-for="stab in sourceTabs"
      :key="stab.id"
      type="button"
      class="inline-flex shrink-0 items-center gap-1 rounded-lg px-2.5 py-1 text-[11px] font-semibold transition"
      :class="
        activeSourceTab === stab.id
          ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-xs'
          : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/65 hover:text-[var(--ctp-text)]'
      "
      @click="emit('switchSourceTab', stab.id)"
    >
      <span>{{ stab.label }}</span>
      <span
        class="rounded-full px-1.5 py-0.2 font-mono text-[10px]"
        :class="
          activeSourceTab === stab.id
            ? 'bg-black/15 text-[var(--ctp-on-accent)]'
            : 'bg-[var(--ctp-mantle)] text-[var(--ctp-subtext0)]'
        "
      >
        {{ stab.count }}
      </span>
    </button>
  </div>

  <div class="flex-1 overflow-y-auto p-3">
    <div v-if="searching" class="py-12 text-center text-xs text-[var(--ctp-subtext0)]">
      正在并发检索各优先级刮削源并合成缺失字段…
    </div>

    <!-- 1. Default `✦ 多源聚合` Queue View -->
    <div v-else-if="activeSourceTab === 'merged'" class="space-y-2">
      <div class="px-1 text-[11px] text-[var(--ctp-subtext0)]">
        按下方优先级顺序互补填充，可上下调整优先级或点入单源更换匹配条目：
      </div>

      <div
        v-for="(brief, idx) in orderedActiveBriefs"
        :key="brief.id"
        class="rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 p-2.5 transition"
        :class="ignoredProviderIds.has(brief.id) ? 'opacity-50' : ''"
      >
        <div class="flex items-center justify-between gap-2">
          <div class="flex items-center gap-1.5">
            <span
              class="flex h-5 w-5 items-center justify-center rounded-md bg-[var(--ctp-accent-soft)] font-mono text-[10px] font-bold text-[var(--ctp-accent)]"
            >
              #{{ idx + 1 }}
            </span>
            <span class="text-xs font-bold text-[var(--ctp-text)]">{{ brief.name }}</span>
            <span
              v-if="ignoredProviderIds.has(brief.id)"
              class="rounded-md bg-[var(--ctp-canvas)] px-1.5 py-0.5 text-[10px] text-[var(--ctp-subtext0)]"
            >
              已排除
            </span>
          </div>

          <div class="flex items-center gap-1">
            <button
              type="button"
              class="flex h-6 w-6 items-center justify-center rounded-md transition"
              :class="
                ignoredProviderIds.has(brief.id)
                  ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)]/70 hover:text-amber-500'
              "
              :title="ignoredProviderIds.has(brief.id) ? '恢复此源参与聚合' : '在本次聚合中排除此源'"
              @click="emit('toggleProviderIgnored', brief.id)"
            >
              <LuBan class="h-3.5 w-3.5" />
            </button>
            <button
              type="button"
              :disabled="idx === 0"
              class="flex h-6 w-6 items-center justify-center rounded-md text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-accent)] disabled:opacity-30"
              title="上移优先级"
              @click="emit('moveProviderOrder', idx, -1)"
            >
              <LuArrowUp class="h-3.5 w-3.5" />
            </button>
            <button
              type="button"
              :disabled="idx === orderedActiveBriefs.length - 1"
              class="flex h-6 w-6 items-center justify-center rounded-md text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-accent)] disabled:opacity-30"
              title="下移优先级"
              @click="emit('moveProviderOrder', idx, 1)"
            >
              <LuArrowDown class="h-3.5 w-3.5" />
            </button>
            <button
              v-if="candidatesCountFor(brief.id) > 0"
              type="button"
              class="inline-flex items-center gap-0.5 rounded-lg bg-[var(--ctp-canvas)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent-soft)]"
              @click="emit('switchSourceTab', brief.id)"
            >
              <span>换选 ({{ candidatesCountFor(brief.id) }})</span>
              <LuChevronRight class="h-3 w-3" />
            </button>
          </div>
        </div>

        <div
          v-if="selectedByProvider[brief.id]"
          class="mt-2 flex items-center gap-2.5 rounded-xl bg-[var(--ctp-canvas)]/80 p-2"
        >
          <div
            class="flex h-11 w-9 shrink-0 items-center justify-center overflow-hidden rounded-md bg-[var(--ctp-mantle)]"
          >
            <img
              v-if="isCoverUsable(selectedByProvider[brief.id]?.cover_url)"
              :src="resolveScraperCoverUrl(selectedByProvider[brief.id]!.cover_url)"
              alt=""
              class="h-full w-full object-cover"
              loading="lazy"
              @error="emit('markCoverBroken', selectedByProvider[brief.id]?.cover_url)"
            />
            <LuImage v-else class="h-3.5 w-3.5 text-[var(--ctp-subtext0)]/55" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="truncate text-xs font-semibold text-[var(--ctp-text)]">
              {{ selectedByProvider[brief.id]!.title }}
            </div>
            <div class="truncate text-[11px] text-[var(--ctp-subtext0)]">
              {{ selectedByProvider[brief.id]!.subtitle || '已参与聚合' }}
            </div>
          </div>
          <LuRefreshCw
            v-if="loadingProviderId === brief.id"
            class="h-3.5 w-3.5 shrink-0 animate-spin text-[var(--ctp-accent)]"
          />
        </div>
        <div v-else class="mt-1.5 px-1 text-[11px] text-[var(--ctp-subtext0)]">
          未检索到匹配条目
        </div>
      </div>
    </div>

    <!-- 2. Individual Provider Candidate List -->
    <div
      v-else-if="candidatesForActiveSource.length === 0"
      class="py-12 text-center text-xs text-[var(--ctp-subtext0)]"
    >
      该刮削源下暂无匹配条目
    </div>
    <div v-else class="space-y-1.5">
      <button
        v-for="cand in candidatesForActiveSource"
        :key="`${cand.provider_id}-${cand.external_id}`"
        type="button"
        class="flex w-full items-start gap-3 rounded-2xl p-2.5 text-left transition"
        :class="
          selectedByProvider[cand.provider_id]?.external_id === cand.external_id
            ? 'bg-[var(--ctp-accent-soft)] ring-1 ring-[var(--ctp-accent)]'
            : 'hover:bg-[var(--ctp-surface0)]/55'
        "
        @click="emit('selectCandidate', cand)"
      >
        <div
          class="flex h-14 w-11 shrink-0 items-center justify-center overflow-hidden rounded-lg bg-[var(--ctp-mantle)]"
        >
          <img
            v-if="isCoverUsable(cand.cover_url)"
            :src="resolveScraperCoverUrl(cand.cover_url)"
            alt=""
            class="h-full w-full object-cover"
            loading="lazy"
            @error="emit('markCoverBroken', cand.cover_url)"
          />
          <LuImage v-else class="h-4 w-4 text-[var(--ctp-subtext0)]/55" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-1.5">
            <span
              v-if="selectedByProvider[cand.provider_id]?.external_id === cand.external_id"
              class="inline-flex items-center gap-0.5 rounded-md bg-[var(--ctp-accent)] px-1.5 py-0.2 text-[10px] font-semibold text-[var(--ctp-on-accent)]"
            >
              <LuCheck class="h-2.5 w-2.5" />
              当前采用
            </span>
            <span v-if="cand.year" class="text-[10px] text-[var(--ctp-subtext0)]">
              {{ cand.year }}
            </span>
          </div>
          <div class="mt-1 truncate text-xs font-semibold text-[var(--ctp-text)]">
            {{ cand.title }}
          </div>
          <div
            v-if="cand.subtitle"
            class="mt-0.5 truncate text-[11px] text-[var(--ctp-subtext0)]"
          >
            {{ cand.subtitle }}
          </div>
        </div>
      </button>
    </div>
  </div>
  </div>
</template>
