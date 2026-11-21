<script setup lang="ts">
import { nextTick, ref, watch, type Component } from 'vue'
import {
  LuCheck,
  LuChevronDown,
  LuChevronRight,
  LuFolderPlus,
  LuLoader2,
  LuRefreshCw,
  LuSlidersHorizontal,
  LuTrash2,
  LuX,
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
  creatingCollection,
  SIDEBAR_PREVIEW_LIMIT,
  isGroupFolded,
  toggleGroupFold,
  isGroupExpanded,
  toggleGroupExpanded,
  visibleGroupItems,
  toggleCreateForGroup,
  cancelCreateCollection,
  handleCreateCollection,
} = useWorkspaceSidebar(() => emit('closeMobile'))

const createInputRef = ref<HTMLInputElement[] | HTMLInputElement | null>(null)

watch(creatingGroupId, async (id) => {
  if (!id) return
  await nextTick()
  const el = Array.isArray(createInputRef.value) ? createInputRef.value[0] : createInputRef.value
  el?.focus()
})
</script>

<template>
  <Transition name="md-fade">
    <div
      v-if="mobileMenuOpen"
      class="fixed inset-0 z-[25] bg-[var(--md-sys-color-scrim)]/32 md:hidden"
      @click="emit('closeMobile')"
    />
  </Transition>

  <aside
    class="safe-pl fixed top-16 left-0 z-40 flex w-72 shrink-0 flex-col bg-[var(--md-sys-color-surface-container)] transition-[width,transform] duration-300 ease-[var(--md-sys-motion-easing-standard)] select-none md:static md:top-0 md:bottom-0"
    :class="[
      showMusicBar ? 'bottom-[var(--player-bar-h)]' : 'bottom-0',
      mobileMenuOpen
        ? 'translate-x-0 rounded-r-[28px] bg-[var(--md-sys-color-surface-container-low)] shadow-[var(--md-sys-elevation-3)] md:rounded-none md:bg-[var(--md-sys-color-surface-container)] md:shadow-none'
        : '-translate-x-full md:translate-x-0',
      collapsed ? 'md:w-20' : 'md:w-64',
    ]"
  >
    <!-- Mobile Domain Switcher Pills -->
    <div
      v-if="domainTabs.length > 1"
      class="grid grid-cols-2 gap-1.5 px-3 pt-3 pb-2 md:hidden"
    >
      <button
        v-for="tab in domainTabs"
        :key="`mob-${tab.kind}`"
        type="button"
        class="md-state flex items-center justify-center gap-1.5 rounded-full px-3 py-2 text-xs font-semibold"
        :class="
          domainStore.activeDomain === tab.kind
            ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
            : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
        "
        @click="emit('switchDomain', tab.kind)"
      >
        <component :is="tab.icon" class="h-3.5 w-3.5 shrink-0" />
        <span class="truncate">{{ tab.label }}</span>
      </button>
    </div>

    <!-- COMPACT MODE: Authentic MD3 Navigation Rail (80px, 56x32 Active Pill + Label) -->
    <nav
      v-if="isCompact"
      class="hidden flex-1 flex-col items-center gap-1.5 overflow-y-auto px-2 pt-2 pb-4 md:flex"
    >
      <button
        v-for="item in primaryNavItems"
        :key="item.key"
        type="button"
        class="group flex w-full flex-col items-center gap-1 py-1 text-[11px] font-medium outline-none"
        @click="item.onClick()"
      >
        <span
          class="md-state flex h-8 w-14 items-center justify-center rounded-full transition group-focus-visible:outline-2 group-focus-visible:outline-offset-2 group-focus-visible:outline-[var(--md-sys-color-primary)]"
          :class="
            item.active
              ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] group-hover:text-[var(--md-sys-color-on-surface)]'
          "
        >
          <component :is="item.icon" class="h-[18px] w-[18px] shrink-0" />
        </span>
        <span
          class="max-w-[68px] truncate"
          :class="
            item.active
              ? 'font-semibold text-[var(--md-sys-color-on-surface)]'
              : 'text-[var(--md-sys-color-on-surface-variant)]'
          "
        >
          {{ item.label }}
        </span>
      </button>

      <div
        v-if="activeSidebarGroups.length > 0"
        class="my-1.5 h-px w-8 bg-[var(--md-sys-color-outline-variant)]/60"
      />

      <button
        v-for="group in activeSidebarGroups"
        :key="group.id"
        type="button"
        class="md-icon-btn md-state"
        :class="
          group.overviewActive || group.items.some((it) => it.active)
            ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
            : ''
        "
        :title="group.label"
        @click="group.onOpenOverview()"
      >
        <component :is="group.icon" class="h-[18px] w-[18px] shrink-0" />
      </button>

      <button
        type="button"
        class="md-icon-btn md-state mt-1"
        :title="t('layoutCustomizer.customizeSidebar')"
        @click="emit('openCustomizer')"
      >
        <LuSlidersHorizontal class="h-[18px] w-[18px] shrink-0" />
      </button>
    </nav>

    <!-- EXPANDED MODE: Authentic MD3 Navigation Drawer (Full-Pill Active Indicator) -->
    <nav v-else class="flex-1 space-y-1 overflow-y-auto px-3 pt-2 pb-5">
      <button
        v-for="item in primaryNavItems"
        :key="item.key"
        type="button"
        class="md-state hidden h-11 w-full items-center gap-3 rounded-full px-4 text-[13px] font-medium md:flex"
        :class="
          item.active
            ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
            : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
        "
        @click="item.onClick()"
      >
        <component :is="item.icon" class="h-[18px] w-[18px] shrink-0" />
        <span class="truncate">{{ item.label }}</span>
      </button>

      <div class="mt-2 flex items-center justify-between px-3 pt-1 md:mt-4 md:pt-2">
        <span
          class="text-[11px] font-semibold tracking-wider text-[var(--md-sys-color-on-surface-variant)] uppercase"
        >
          {{ t(`app.domains.${domainStore.activeDomain}`) }}
        </span>
        <button
          type="button"
          class="md-state inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
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
        class="pt-3"
      >
        <div class="mb-1 flex items-center justify-between px-2.5">
          <div class="flex min-w-0 flex-1 items-center gap-0.5">
            <button
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state !h-6 !w-6"
              :title="t('nav.toggleGroup')"
              @click="toggleGroupFold(group.id)"
            >
              <LuChevronRight v-if="isGroupFolded(group.id)" class="h-3.5 w-3.5" />
              <LuChevronDown v-else class="h-3.5 w-3.5" />
            </button>

            <button
              type="button"
              class="truncate rounded-full px-1.5 py-0.5 text-left text-[11px] font-semibold tracking-wider uppercase transition focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--md-sys-color-primary)]"
              :class="
                group.overviewActive
                  ? 'text-[var(--md-sys-color-primary)]'
                  : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
              "
              @click="group.onOpenOverview()"
            >
              {{ group.label }}
            </button>
          </div>

          <button
            v-if="group.canCreate"
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
            :title="group.createTitle"
            @click="toggleCreateForGroup(group)"
          >
            <LuFolderPlus class="h-3.5 w-3.5" />
          </button>
          <span
            v-else
            class="px-2 text-[11px] font-medium tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ group.items.length }}
          </span>
        </div>

        <div v-if="!isGroupFolded(group.id)" class="space-y-0.5">
          <form
            v-if="group.canCreate && creatingGroupId === group.id"
            class="mb-2 flex items-center gap-1 px-1.5"
            @submit.prevent="handleCreateCollection"
          >
            <input
              ref="createInputRef"
              v-model="newCollectionName"
              type="text"
              :disabled="creatingCollection"
              :placeholder="group.createPlaceholder"
              class="md-input h-9 min-w-0 flex-1 px-3 text-xs"
              @keydown.escape.stop.prevent="cancelCreateCollection"
            />
            <button
              type="submit"
              :disabled="!newCollectionName.trim() || creatingCollection"
              :title="t('common.confirm')"
              :aria-label="t('common.confirm')"
              class="md-icon-btn md-icon-btn-sm md-state !h-8 !w-8 shrink-0 text-[var(--md-sys-color-primary)] disabled:opacity-35"
            >
              <LuLoader2 v-if="creatingCollection" class="h-3.5 w-3.5 animate-spin" />
              <LuCheck v-else class="h-4 w-4" />
            </button>
            <button
              type="button"
              :disabled="creatingCollection"
              :title="t('common.cancel')"
              :aria-label="t('common.cancel')"
              class="md-icon-btn md-icon-btn-sm md-state !h-8 !w-8 shrink-0 text-[var(--md-sys-color-on-surface-variant)]"
              @click="cancelCreateCollection"
            >
              <LuX class="h-4 w-4" />
            </button>
          </form>

          <div
            v-for="sub in visibleGroupItems(group)"
            :key="sub.id"
            class="md-state group flex h-9 items-center justify-between rounded-full px-3.5 text-[13px] has-[>button:focus-visible]:ring-2 has-[>button:focus-visible]:ring-inset has-[>button:focus-visible]:ring-[var(--md-sys-color-primary)]"
            :class="
              sub.active
                ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
                : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
            "
            :title="sub.label"
          >
            <button
              type="button"
              class="flex h-full min-w-0 flex-1 items-center gap-2.5 rounded-full text-left focus-visible:outline-none"
              @click="sub.onClick()"
            >
              <component :is="sub.icon" class="h-3.5 w-3.5 shrink-0" />
              <span class="truncate">{{ sub.label }}</span>
            </button>

            <div class="ml-2 flex shrink-0 items-center gap-1">
              <LuRefreshCw
                v-if="sub.scanning"
                class="h-3.5 w-3.5 animate-spin text-[var(--md-sys-color-primary)]"
              />
              <span
                v-else-if="sub.count !== undefined"
                class="text-[11px] tabular-nums opacity-75"
              >
                {{ sub.count }}
              </span>

              <button
                v-if="sub.onDelete"
                type="button"
                class="inline-flex rounded-full p-1 opacity-100 transition-opacity hover:text-[var(--md-sys-color-error)] focus:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--md-sys-color-primary)] sm:opacity-0 sm:group-hover:opacity-100 sm:group-focus-within:opacity-100"
                :title="t('common.delete')"
                :aria-label="t('common.delete')"
                @click.stop="sub.onDelete()"
              >
                <LuTrash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>

          <button
            v-if="group.items.length > SIDEBAR_PREVIEW_LIMIT"
            type="button"
            class="md-state flex h-8 w-full items-center gap-1.5 rounded-full px-4 text-xs font-medium text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
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
    </nav>
  </aside>
</template>
