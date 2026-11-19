<script setup lang="ts">
import type { Component } from 'vue'
import {
  LuChevronDown,
  LuChevronRight,
  LuFolderPlus,
  LuRefreshCw,
  LuSlidersHorizontal,
  LuTrash2,
} from 'vue-icons-plus/lu'
import type { MediaKind } from '@/types/api'
import { useWorkspaceSidebar } from './useWorkspaceSidebar'

defineProps<{
  mobileMenuOpen: boolean
  collapsed: boolean
  isCompact: boolean
  showMusicBar: boolean
  domainTabs: Array<{ kind: MediaKind; label: string; icon: Component }>
}>()

const emit = defineEmits<{
  closeMobile: []
  switchDomain: [kind: MediaKind]
  openCustomizer: []
}>()

const {
  t,
  domainStore,
  primaryNavItems,
  activeSidebarGroups,
  creatingGroupId,
  newCollectionName,
  SIDEBAR_PREVIEW_LIMIT,
  isGroupFolded,
  toggleGroupFold,
  isGroupExpanded,
  toggleGroupExpanded,
  visibleGroupItems,
  toggleCreateForGroup,
  handleCreateCollection,
} = useWorkspaceSidebar(() => emit('closeMobile'))
</script>

<template>
<div
  v-if="mobileMenuOpen"
  class="fixed inset-0 z-30 bg-black/45 md:hidden"
  @click="emit('closeMobile')"
/>

<aside
  class="fixed top-16 left-0 z-40 flex shrink-0 flex-col border-r border-[var(--ctp-surface0)]/65 bg-[var(--ctp-shell)] transition-all duration-200 select-none md:static md:top-0 md:bottom-0 md:border-r-0"
  :class="[
    showMusicBar ? 'bottom-20' : 'bottom-0',
    mobileMenuOpen ? 'w-64 translate-x-0' : '-translate-x-full md:translate-x-0',
    collapsed ? 'md:w-16' : 'md:w-60',
  ]"
>
  <!-- Mobile Domain Switcher Grid -->
  <div
    v-if="domainTabs.length > 1"
    class="grid grid-cols-2 gap-1.5 border-b border-[var(--ctp-surface0)]/60 px-3 py-2.5 md:hidden"
  >
    <button
      v-for="tab in domainTabs"
      :key="`mob-${tab.kind}`"
      type="button"
      class="flex items-center justify-center gap-1.5 rounded-xl px-2.5 py-2 text-xs font-semibold transition"
      :class="
        domainStore.activeDomain === tab.kind
          ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
          : 'bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)]'
      "
      @click="emit('switchDomain', tab.kind)"
    >
      <component :is="tab.icon" class="h-3.5 w-3.5 shrink-0" />
      <span class="truncate">{{ tab.label }}</span>
    </button>
  </div>

  <nav class="flex-1 space-y-0.5 overflow-y-auto px-3 pt-1 pb-3">
    <!-- Fixed Top Primary Navigation (Home / All / Continue / Favorites) -->
    <button
      v-for="item in primaryNavItems"
      :key="item.key"
      type="button"
      class="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-sm font-medium transition"
      :class="[
        isCompact ? 'md:justify-center md:px-0' : '',
        item.active
          ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
          : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]',
      ]"
      :title="isCompact ? item.label : undefined"
      @click="item.onClick()"
    >
      <component :is="item.icon" class="h-4 w-4 shrink-0" />
      <span v-if="!isCompact" class="truncate">{{ item.label }}</span>
    </button>

    <!-- Compact Rail Icons for Configured Sidebar Groups -->
    <div
      v-if="isCompact"
      class="mt-3 hidden space-y-0.5 border-t border-[var(--ctp-surface0)]/55 pt-3 md:block"
    >
      <button
        v-for="group in activeSidebarGroups"
        :key="group.id"
        type="button"
        class="flex w-full items-center justify-center rounded-xl py-2 transition"
        :class="
          group.overviewActive || group.items.some((it) => it.active)
            ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
        "
        :title="group.label"
        @click="group.onOpenOverview()"
      >
        <component :is="group.icon" class="h-4 w-4 shrink-0" />
      </button>

      <button
        type="button"
        class="flex w-full items-center justify-center rounded-xl py-2 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
        :title="t('layoutCustomizer.customizeSidebar')"
        @click="emit('openCustomizer')"
      >
        <LuSlidersHorizontal class="h-4 w-4 shrink-0" />
      </button>
    </div>

    <!-- Expanded Customizable Foldable Groups (0..=4) -->
    <template v-if="!isCompact">
      <div class="mt-4 flex items-center justify-between border-t border-[var(--ctp-surface0)]/50 px-2 pt-3">
        <span class="text-[10px] font-semibold tracking-wider text-[var(--ctp-subtext0)]/80 uppercase">
          {{ t(`app.domains.${domainStore.activeDomain}`) }}
        </span>
        <button
          type="button"
          class="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
          :title="t('layoutCustomizer.customizeSidebar')"
          @click="emit('openCustomizer')"
        >
          <LuSlidersHorizontal class="h-3 w-3" />
          <span>{{ t('layoutCustomizer.customizeSidebar') }}</span>
        </button>
      </div>

      <div
        v-for="group in activeSidebarGroups"
        :key="group.id"
        class="pt-3.5"
      >
        <div class="mb-1 flex items-center justify-between px-2">
          <div class="flex min-w-0 flex-1 items-center gap-0.5">
            <button
              type="button"
              class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
              :title="t('nav.toggleGroup')"
              @click="toggleGroupFold(group.id)"
            >
              <LuChevronRight v-if="isGroupFolded(group.id)" class="h-3.5 w-3.5" />
              <LuChevronDown v-else class="h-3.5 w-3.5" />
            </button>

            <button
              type="button"
              class="truncate rounded-md px-1 py-0.5 text-left text-[11px] font-bold tracking-wider uppercase transition"
              :class="
                group.overviewActive
                  ? 'text-[var(--ctp-accent)]'
                  : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
              "
              @click="group.onOpenOverview()"
            >
              {{ group.label }}
            </button>
          </div>

          <button
            v-if="group.canCreate"
            type="button"
            class="rounded-md p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
            :title="group.createTitle"
            @click="toggleCreateForGroup(group)"
          >
            <LuFolderPlus class="h-3.5 w-3.5" />
          </button>
          <span
            v-else
            class="px-1.5 text-[11px] font-medium tabular-nums text-[var(--ctp-subtext0)]"
          >
            {{ group.items.length }}
          </span>
        </div>

        <div v-if="!isGroupFolded(group.id)" class="space-y-0.5">
          <form
            v-if="group.canCreate && creatingGroupId === group.id"
            class="mb-2 px-2"
            @submit.prevent="handleCreateCollection"
          >
            <input
              v-model="newCollectionName"
              type="text"
              :placeholder="group.createPlaceholder"
              class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1.5 text-xs outline-none focus:border-[var(--ctp-accent)]"
            />
          </form>

          <div
            v-for="sub in visibleGroupItems(group)"
            :key="sub.id"
            class="group flex items-center justify-between rounded-xl px-3 py-1.5 text-sm transition"
            :class="
              sub.active
                ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]'
            "
            :title="sub.label"
          >
            <button
              type="button"
              class="flex min-w-0 flex-1 items-center gap-2.5 text-left"
              @click="sub.onClick()"
            >
              <component :is="sub.icon" class="h-3.5 w-3.5 shrink-0" />
              <span class="truncate">{{ sub.label }}</span>
            </button>

            <div class="ml-2 flex shrink-0 items-center gap-1">
              <LuRefreshCw
                v-if="sub.scanning"
                class="h-3.5 w-3.5 animate-spin text-[var(--ctp-accent)]"
              />
              <span
                v-else-if="sub.count !== undefined"
                class="text-xs tabular-nums text-[var(--ctp-subtext0)]"
              >
                {{ sub.count }}
              </span>

              <button
                v-if="sub.onDelete"
                type="button"
                class="hidden rounded p-0.5 text-[var(--ctp-subtext0)] group-hover:inline-flex hover:text-[var(--ctp-red)]"
                :title="t('common.delete')"
                @click.stop="sub.onDelete()"
              >
                <LuTrash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>

          <button
            v-if="group.items.length > SIDEBAR_PREVIEW_LIMIT"
            type="button"
            class="flex w-full items-center gap-1.5 rounded-xl px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/50 hover:text-[var(--ctp-text)]"
            @click="toggleGroupExpanded(group.id)"
          >
            <span>
              {{
                isGroupExpanded(group.id)
                  ? t('nav.showLess')
                  : t('nav.showMore', {
                      count: group.items.length - SIDEBAR_PREVIEW_LIMIT,
                    })
              }}
            </span>
          </button>
        </div>
      </div>
    </template>
  </nav>
</aside>
</template>

