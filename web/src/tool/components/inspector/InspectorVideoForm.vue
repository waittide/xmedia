<script setup lang="ts">
import { computed } from 'vue'
import { LuLock, LuUnlock } from 'vue-icons-plus/lu'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import {
  isVideoFieldLockedIn,
  toggleVideoFieldLockIn,
  type VideoInspectorForm,
} from './types'

const props = defineProps<{
  form: VideoInspectorForm
}>()

const nfoKindModel = computed<'movie' | 'episode'>({
  get: () => (props.form.isEpisode ? 'episode' : 'movie'),
  set: (v) => {
    props.form.isEpisode = v === 'episode'
  },
})

const nfoKindOptions = [
  { value: 'movie' as const, label: '电影 NFO (<movie>)' },
  { value: 'episode' as const, label: '剧集单集 NFO (<episodedetails>)' },
]

function isVideoFieldLocked(field: string): boolean {
  return isVideoFieldLockedIn(props.form.lockedFields, field)
}

function toggleVideoFieldLock(field: string): void {
  props.form.lockedFields = toggleVideoFieldLockIn(props.form.lockedFields, field)
}

const inputCls =
  'w-full rounded-xl border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] px-3 py-2 text-xs text-[var(--md-sys-color-on-surface)] outline-none transition focus:border-[var(--md-sys-color-primary)]'

function lockBtnCls(field: string): string {
  return isVideoFieldLocked(field)
    ? 'bg-[var(--md-sys-color-warning-container)] font-semibold text-[var(--md-sys-color-on-warning-container)]'
    : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
}
</script>

<template>
  <div class="space-y-3">
    <MdSegmentedButton v-model="nfoKindModel" :options="nfoKindOptions" size="sm" />

    <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
      <div class="block sm:col-span-2">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
            {{ form.isEpisode ? '单集标题 (Episode Title)' : '电影标题 (Title)' }}
          </span>
          <button
            type="button"
            class="inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[10px] transition"
            :class="lockBtnCls('title')"
            title="锁定/解锁标题（锁定后刮削跳过覆盖，写入 NFO lockedfields）"
            @click="toggleVideoFieldLock('title')"
          >
            <LuLock v-if="isVideoFieldLocked('title')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
            <span v-if="isVideoFieldLocked('title')">已锁定</span>
          </button>
        </div>
        <input v-model="form.title" type="text" :class="inputCls" />
      </div>

      <div v-if="form.isEpisode" class="block sm:col-span-2">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">所属剧集名 (Show Title)</span>
          <button
            type="button"
            class="inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[10px] transition"
            :class="lockBtnCls('show_title')"
            title="锁定/解锁剧集名"
            @click="toggleVideoFieldLock('show_title')"
          >
            <LuLock v-if="isVideoFieldLocked('show_title')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <input v-model="form.showTitle" type="text" :class="inputCls" />
      </div>

      <div class="block sm:col-span-2">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">原名 (Original Title)</span>
          <button
            type="button"
            class="inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[10px] transition"
            :class="lockBtnCls('original_title')"
            title="锁定/解锁原名"
            @click="toggleVideoFieldLock('original_title')"
          >
            <LuLock v-if="isVideoFieldLocked('original_title')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <input v-model="form.originalTitle" type="text" :class="inputCls" />
      </div>

      <div v-if="form.isEpisode" class="grid grid-cols-2 gap-2 sm:col-span-2">
        <label class="block">
          <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">季号 (Season)</span>
          <input v-model="form.season" type="number" min="0" :class="inputCls" />
        </label>
        <label class="block">
          <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">集号 (Episode)</span>
          <input v-model="form.episode" type="number" min="0" :class="inputCls" />
        </label>
      </div>

      <div class="block">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">年份 (Year)</span>
          <button
            type="button"
            class="inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[10px] transition"
            :class="lockBtnCls('year')"
            title="锁定/解锁年份"
            @click="toggleVideoFieldLock('year')"
          >
            <LuLock v-if="isVideoFieldLocked('year')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <input v-model="form.year" type="number" :class="inputCls" />
      </div>

      <div class="block">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">评分 (Rating)</span>
          <button
            type="button"
            class="inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[10px] transition"
            :class="lockBtnCls('rating')"
            title="锁定/解锁评分"
            @click="toggleVideoFieldLock('rating')"
          >
            <LuLock v-if="isVideoFieldLocked('rating')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <input v-model="form.rating" type="number" step="0.1" :class="inputCls" />
      </div>

      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">TMDB ID</span>
        <input v-model="form.tmdbId" type="text" :class="inputCls" />
      </label>

      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">Bangumi ID</span>
        <input v-model="form.bangumiId" type="text" :class="inputCls" />
      </label>

      <div class="block">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">导演（`/` 分隔）</span>
          <button
            type="button"
            class="inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[10px] transition"
            :class="lockBtnCls('directors')"
            title="锁定/解锁导演"
            @click="toggleVideoFieldLock('directors')"
          >
            <LuLock v-if="isVideoFieldLocked('directors')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <input v-model="form.directors" type="text" :class="inputCls" />
      </div>

      <div class="block">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">制片厂 / 动画制作</span>
          <button
            type="button"
            class="inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[10px] transition"
            :class="lockBtnCls('studio')"
            title="锁定/解锁制片厂"
            @click="toggleVideoFieldLock('studio')"
          >
            <LuLock v-if="isVideoFieldLocked('studio')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <input v-model="form.studio" type="text" :class="inputCls" />
      </div>

      <div class="block sm:col-span-2">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">演员 / 声优（`/` 分隔）</span>
          <button
            type="button"
            class="inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[10px] transition"
            :class="lockBtnCls('actors')"
            title="锁定/解锁演员"
            @click="toggleVideoFieldLock('actors')"
          >
            <LuLock v-if="isVideoFieldLocked('actors')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <input v-model="form.actors" type="text" :class="inputCls" />
      </div>

      <div class="block">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">类型 (Genres)</span>
          <button
            type="button"
            class="inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[10px] transition"
            :class="lockBtnCls('genres')"
            title="锁定/解锁类型"
            @click="toggleVideoFieldLock('genres')"
          >
            <LuLock v-if="isVideoFieldLocked('genres')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <input v-model="form.genres" type="text" :class="inputCls" />
      </div>

      <div class="block">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">标签 (Tags)</span>
          <button
            type="button"
            class="inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[10px] transition"
            :class="lockBtnCls('tags')"
            title="锁定/解锁标签"
            @click="toggleVideoFieldLock('tags')"
          >
            <LuLock v-if="isVideoFieldLocked('tags')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <input v-model="form.tags" type="text" :class="inputCls" />
      </div>

      <div class="block sm:col-span-2">
        <div class="mb-1 flex items-center justify-between">
          <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">剧情简介 (Plot)</span>
          <button
            type="button"
            class="inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-[10px] transition"
            :class="lockBtnCls('summary')"
            title="锁定/解锁简介"
            @click="toggleVideoFieldLock('summary')"
          >
            <LuLock v-if="isVideoFieldLocked('summary')" class="h-3 w-3" />
            <LuUnlock v-else class="h-3 w-3" />
          </button>
        </div>
        <textarea
          v-model="form.summary"
          rows="5"
          class="w-full resize-y rounded-xl border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] p-2.5 text-xs leading-relaxed text-[var(--md-sys-color-on-surface)] outline-none transition focus:border-[var(--md-sys-color-primary)]"
        />
      </div>
    </div>
  </div>
</template>
