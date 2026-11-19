<script setup lang="ts">
import { LuLock, LuUnlock } from 'vue-icons-plus/lu'
import {
  isVideoFieldLockedIn,
  toggleVideoFieldLockIn,
  type VideoInspectorForm,
} from './types'

const props = defineProps<{
  form: VideoInspectorForm
}>()

function isVideoFieldLocked(field: string): boolean {
  return isVideoFieldLockedIn(props.form.lockedFields, field)
}

function toggleVideoFieldLock(field: string): void {
  props.form.lockedFields = toggleVideoFieldLockIn(props.form.lockedFields, field)
}
</script>

<template>
  <div class="space-y-3">
  <div class="flex items-center gap-1 rounded-xl bg-[var(--ctp-canvas)] p-1 border border-[var(--ctp-surface0)]/75">
    <button
      type="button"
      class="flex-1 rounded-lg py-1.5 text-xs font-semibold transition"
      :class="
        !form.isEpisode
          ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
          : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
      "
      @click="form.isEpisode = false"
    >
      电影 NFO (&lt;movie&gt;)
    </button>
    <button
      type="button"
      class="flex-1 rounded-lg py-1.5 text-xs font-semibold transition"
      :class="
        form.isEpisode
          ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
          : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
      "
      @click="form.isEpisode = true"
    >
      剧集单集 NFO (&lt;episodedetails&gt;)
    </button>
  </div>

  <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
    <div class="block sm:col-span-2">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">
          {{ form.isEpisode ? '单集标题 (Episode Title)' : '电影标题 (Title)' }}
        </span>
        <button
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isVideoFieldLocked('title') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁标题（锁定后刮削跳过覆盖，写入 NFO lockedfields）"
          @click="toggleVideoFieldLock('title')"
        >
          <LuLock v-if="isVideoFieldLocked('title')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
          <span>{{ isVideoFieldLocked('title') ? '已锁定' : '' }}</span>
        </button>
      </div>
      <input
        v-model="form.title"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>
    <div v-if="form.isEpisode" class="block sm:col-span-2">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">所属剧集名 (Show Title)</span>
        <button
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isVideoFieldLocked('show_title') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁剧集名"
          @click="toggleVideoFieldLock('show_title')"
        >
          <LuLock v-if="isVideoFieldLocked('show_title')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.showTitle"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>
    <div class="block sm:col-span-2">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">原名 (Original Title)</span>
        <button
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isVideoFieldLocked('original_title') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁原名"
          @click="toggleVideoFieldLock('original_title')"
        >
          <LuLock v-if="isVideoFieldLocked('original_title')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.originalTitle"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>

    <div v-if="form.isEpisode" class="grid grid-cols-2 gap-2 sm:col-span-2">
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">季号 (Season)</span>
        <input
          v-model="form.season"
          type="number"
          min="0"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </label>
      <label class="block">
        <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">集号 (Episode)</span>
        <input
          v-model="form.episode"
          type="number"
          min="0"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </label>
    </div>

    <div class="block">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">年份 (Year)</span>
        <button
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isVideoFieldLocked('year') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁年份"
          @click="toggleVideoFieldLock('year')"
        >
          <LuLock v-if="isVideoFieldLocked('year')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.year"
        type="number"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>
    <div class="block">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">评分 (Rating)</span>
        <button
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isVideoFieldLocked('rating') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁评分"
          @click="toggleVideoFieldLock('rating')"
        >
          <LuLock v-if="isVideoFieldLocked('rating')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.rating"
        type="number"
        step="0.1"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>
    <label class="block">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">TMDB ID</span>
      <input
        v-model="form.tmdbId"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <label class="block">
      <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">Bangumi ID</span>
      <input
        v-model="form.bangumiId"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </label>
    <div class="block">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">导演（`/` 分隔）</span>
        <button
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isVideoFieldLocked('directors') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁导演"
          @click="toggleVideoFieldLock('directors')"
        >
          <LuLock v-if="isVideoFieldLocked('directors')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.directors"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>
    <div class="block">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">制片厂 / 动画制作</span>
        <button
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isVideoFieldLocked('studio') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁制片厂"
          @click="toggleVideoFieldLock('studio')"
        >
          <LuLock v-if="isVideoFieldLocked('studio')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.studio"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>
    <div class="block sm:col-span-2">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">演员 / 声优（`/` 分隔）</span>
        <button
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isVideoFieldLocked('actors') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁演员"
          @click="toggleVideoFieldLock('actors')"
        >
          <LuLock v-if="isVideoFieldLocked('actors')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.actors"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>
    <div class="block">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">类型 (Genres)</span>
        <button
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isVideoFieldLocked('genres') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁类型"
          @click="toggleVideoFieldLock('genres')"
        >
          <LuLock v-if="isVideoFieldLocked('genres')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.genres"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>
    <div class="block">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">标签 (Tags)</span>
        <button
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isVideoFieldLocked('tags') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
          title="锁定/解锁标签"
          @click="toggleVideoFieldLock('tags')"
        >
          <LuLock v-if="isVideoFieldLocked('tags')" class="h-3 w-3" />
          <LuUnlock v-else class="h-3 w-3" />
        </button>
      </div>
      <input
        v-model="form.tags"
        type="text"
        class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>
    <div class="block sm:col-span-2">
      <div class="mb-1 flex items-center justify-between">
        <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">剧情简介 (Plot)</span>
        <button
          type="button"
          class="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[10px] transition"
          :class="isVideoFieldLocked('summary') ? 'bg-amber-500/15 font-semibold text-amber-500' : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'"
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
        class="w-full resize-y rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-2.5 text-xs leading-relaxed text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
      />
    </div>
  </div>
  </div>
</template>
