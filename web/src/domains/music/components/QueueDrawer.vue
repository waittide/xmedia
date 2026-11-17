<script setup lang="ts">
import { LuX, LuTrash2, LuPlay, LuVolume2, LuRefreshCw } from 'vue-icons-plus/lu'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import CoverImage from './CoverImage.vue'

const { t } = useI18n()
const playerStore = usePlayerStore()
</script>

<template>
  <Teleport to="body">
    <div
      v-if="playerStore.queueOpen"
      class="fixed inset-0 z-[95] flex justify-end bg-black/35 backdrop-blur-[1px]"
      @mousedown.self="playerStore.queueOpen = false"
    >
      <aside
        class="flex h-full w-full max-w-md flex-col border-l border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] shadow-2xl"
      >
        <header class="flex items-center justify-between border-b border-[var(--ctp-surface0)] px-5 py-4">
          <div>
            <h2 class="text-sm font-bold">{{ t('player.queueTitle') }}</h2>
            <p class="text-[11px] text-[var(--ctp-subtext0)]">
              {{ t('player.queueSubtitle') }} · {{ t('common.tracksCount', { count: playerStore.queue.length }) }}
            </p>
          </div>
          <div class="flex items-center gap-1">
            <button
              type="button"
              class="rounded-xl p-2 text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
              :title="t('player.syncFromCloud')"
              @click="playerStore.restoreRemoteQueue(true)"
            >
              <LuRefreshCw class="h-4 w-4" />
            </button>
            <button
              v-if="playerStore.queue.length > 0"
              type="button"
              class="rounded-xl p-2 text-[var(--ctp-red)] transition hover:bg-[var(--ctp-red)]/12"
              :title="t('player.clearQueue')"
              @click="playerStore.clearQueue()"
            >
              <LuTrash2 class="h-4 w-4" />
            </button>
            <button
              type="button"
              class="rounded-xl p-2 text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
              @click="playerStore.queueOpen = false"
            >
              <LuX class="h-4 w-4" />
            </button>
          </div>
        </header>

        <div class="flex-1 divide-y divide-[var(--ctp-surface0)]/50 overflow-y-auto p-2">
          <div
            v-if="playerStore.queue.length === 0"
            class="flex h-48 items-center justify-center text-xs text-[var(--ctp-subtext0)]"
          >
            {{ t('player.noTrack') }}
          </div>

          <div
            v-for="(tr, idx) in playerStore.queue"
            :key="`${tr.id}-${idx}`"
            class="group flex items-center gap-3 rounded-xl px-3 py-2 text-xs transition hover:bg-[var(--ctp-row-hover)]"
            :class="idx === playerStore.currentIndex ? 'bg-[var(--ctp-accent-soft)]' : ''"
          >
            <CoverImage
              :cover-id="tr.id"
              :has-cover="Boolean(tr.cover_key)"
              :size="80"
              rounded="rounded-lg"
              class="h-10 w-10 shrink-0 cursor-pointer"
              @click="playerStore.playQueue(playerStore.queue, idx)"
            />

            <div
              class="min-w-0 flex-1 cursor-pointer"
              @click="playerStore.playQueue(playerStore.queue, idx)"
            >
              <p
                class="truncate font-semibold"
                :class="
                  idx === playerStore.currentIndex
                    ? 'text-[var(--ctp-accent)]'
                    : 'text-[var(--ctp-text)]'
                "
              >
                {{ tr.title }}
              </p>
              <p class="truncate text-[11px] text-[var(--ctp-subtext0)]">
                {{ tr.artist_name }} · {{ tr.album_title }}
              </p>
            </div>

            <LuVolume2
              v-if="idx === playerStore.currentIndex"
              class="h-4 w-4 shrink-0 text-[var(--ctp-accent)]"
            />
            <button
              v-else
              type="button"
              class="hidden rounded-lg p-1.5 text-[var(--ctp-subtext0)] group-hover:inline-flex hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
              @click="playerStore.playQueue(playerStore.queue, idx)"
            >
              <LuPlay class="h-3.5 w-3.5 fill-current" />
            </button>

            <button
              type="button"
              class="rounded-lg p-1.5 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-red)]/10 hover:text-[var(--ctp-red)]"
              @click="playerStore.removeQueueAt(idx)"
            >
              <LuTrash2 class="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </aside>
    </div>
  </Teleport>
</template>
