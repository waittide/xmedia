<script setup lang="ts">
import { ref, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuPanelLeftClose } from 'vue-icons-plus/lu'
import { MdDomainIcons, MdIconButton } from '@/components/ui'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import { useThemeStore } from '@/stores/theme'
import type { MediaKind } from '@/types/api'
import { stripAriaModalOnLeave } from '@/utils/dom'
import type { DomainNavTab } from './NavigationRail.vue'

const props = defineProps<{
  open: boolean
  domainTabs: DomainNavTab[]
  activeDomain: MediaKind
  isSettingsRoute: boolean
}>()

const emit = defineEmits<{
  close: []
  switchDomain: [kind: MediaKind]
}>()

const { t } = useI18n()
const themeStore = useThemeStore()
const escapeStack = useEscapeStack()
const drawerRef = ref<HTMLElement | null>(null)
const titleId = useId()

useFocusTrap(drawerRef, () => props.open)

escapeStack.register(
  () => props.open,
  () => emit('close'),
)

function selectDomain(kind: MediaKind): void {
  emit('close')
  emit('switchDomain', kind)
}
</script>

<template>
  <Teleport to="body">
    <Transition name="md-fade" @before-leave="stripAriaModalOnLeave">
      <div
        v-if="open"
        data-mobile-domain-drawer-backdrop
        class="fixed inset-0 z-[var(--z-sheet)] flex items-stretch justify-start bg-scrim/32 sm:hidden"
        @mousedown.self="emit('close')"
        @click.self="emit('close')"
      >
        <Transition name="md-sheet-left" appear @before-leave="stripAriaModalOnLeave">
          <aside
            ref="drawerRef"
            data-mobile-domain-drawer
            role="dialog"
            :aria-modal="open ? 'true' : undefined"
            :aria-labelledby="titleId"
            tabindex="-1"
            class="flex h-full flex-col overflow-hidden rounded-r-corner-lg bg-surface-container-low pt-[calc(var(--safe-area-top)+0.75rem)] pb-[calc(var(--safe-area-bottom)+0.75rem)] pl-[var(--safe-area-left)] text-on-surface shadow-elevation-1 outline-none select-none"
            :class="
              themeStore.showSidebarLabels
                ? 'w-[min(18rem,calc(100vw-3.5rem))]'
                : 'w-20 items-center'
            "
          >
            <!-- MD3 Modal Navigation Drawer Header -->
            <div
              class="flex items-center pb-2"
              :class="
                themeStore.showSidebarLabels
                  ? 'justify-between gap-3 px-4'
                  : 'w-full flex-col gap-2 px-2'
              "
            >
              <div class="flex min-w-0 items-center gap-3">
                <img
                  :src="themeStore.appIconSrc"
                  alt=""
                  aria-hidden="true"
                  class="h-9 w-9 shrink-0 select-none"
                />
                <span
                  :id="titleId"
                  class="text-title-medium truncate text-on-surface"
                  :class="themeStore.showSidebarLabels ? '' : 'sr-only'"
                >
                  {{ t('app.name') }}
                </span>
              </div>

              <MdIconButton
                size="sm"
                :icon="LuPanelLeftClose"
                :aria-label="t('nav.collapseSidebar')"
                @click="emit('close')"
              />
            </div>

            <!-- Section Label (visible when sidebar labels enabled) -->
            <p
              v-if="themeStore.showSidebarLabels"
              class="text-title-small px-7 py-2 text-on-surface-variant"
            >
              {{ t('nav.currentDomain') }}
            </p>

            <!-- MD3 Navigation Drawer Destination Pills (56dp standard height) -->
            <nav
              :aria-label="t('nav.currentDomain')"
              class="flex-1 overflow-y-auto px-3 py-1"
              :class="themeStore.showSidebarLabels ? 'space-y-1' : 'flex w-full flex-col items-center gap-2'"
            >
              <button
                v-for="tab in domainTabs"
                :key="`drawer-${tab.kind}`"
                type="button"
                :data-drawer-domain-switch="tab.kind"
                :aria-label="tab.label"
                :title="!themeStore.showSidebarLabels ? tab.label : undefined"
                :aria-current="!isSettingsRoute && activeDomain === tab.kind ? 'page' : undefined"
                class="md-state cursor-pointer rounded-corner-full transition-colors duration-150 ease-standard outline-none focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-secondary"
                :class="[
                  themeStore.showSidebarLabels
                    ? 'text-label-large flex h-14 w-full items-center gap-3 px-4 text-left'
                    : 'flex h-12 w-12 items-center justify-center',
                  !isSettingsRoute && activeDomain === tab.kind
                    ? 'bg-secondary-container font-semibold text-on-secondary-container'
                    : 'text-on-surface-variant hover:text-on-surface',
                ]"
                @click="selectDomain(tab.kind)"
              >
                <MdDomainIcons
                  :kind="tab.kind"
                  :active="!isSettingsRoute && activeDomain === tab.kind"
                  class="h-6 w-6 shrink-0"
                />
                <span v-if="themeStore.showSidebarLabels" class="min-w-0 flex-1 truncate">
                  {{ tab.label }}
                </span>
              </button>
            </nav>
          </aside>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
