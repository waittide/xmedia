<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowDown,
  LuArrowUp,
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuGripVertical,
  LuLayers,
  LuPlus,
  LuRotateCcw,
  LuSlidersHorizontal,
  LuX,
} from 'vue-icons-plus/lu'
import {
  HOME_CARD_POOLS,
  MAX_LAYOUT_ITEMS,
  SIDEBAR_GROUP_POOLS,
} from '@/config/workspaceLayout'
import { useAuthStore } from '@/stores/auth'
import { useThemeStore } from '@/stores/theme'
import type { MediaKind } from '@/types/api'

const props = withDefaults(
  defineProps<{
    open?: boolean
    domain?: MediaKind
    initialTab?: 'cards' | 'sidebar'
    inline?: boolean
  }>(),
  {
    open: false,
    domain: 'book',
    initialTab: 'cards',
    inline: false,
  },
)

const emit = defineEmits<{
  close: []
}>()

const { t } = useI18n()
const authStore = useAuthStore()
const themeStore = useThemeStore()

const activeDomain = ref<MediaKind>(props.domain)
const activeTab = ref<'cards' | 'sidebar'>(props.initialTab)
const dragIndex = ref<number | null>(null)

watch(
  () => [props.open, props.domain, props.initialTab] as const,
  ([isOpen, dom, tab]) => {
    if (isOpen || props.inline) {
      activeDomain.value = dom
      activeTab.value = tab
    }
  },
)

const domainOptions = computed(() => {
  const all: Array<{ kind: MediaKind; label: string; icon: typeof LuBookOpen }> = [
    { kind: 'book', label: t('app.domains.book'), icon: LuBookOpen },
    { kind: 'comic', label: t('app.domains.comic'), icon: LuLayers },
    { kind: 'music', label: t('app.domains.music'), icon: LuDisc3 },
    { kind: 'video', label: t('app.domains.video'), icon: LuClapperboard },
  ]
  return all.filter((d) => authStore.hasModule(d.kind))
})

const selectedItems = computed<string[]>(() => {
  const cfg = themeStore.workspaceLayout[activeDomain.value]
  return activeTab.value === 'cards' ? cfg.home_cards : cfg.sidebar_groups
})

const candidatePool = computed<readonly string[]>(() =>
  activeTab.value === 'cards'
    ? HOME_CARD_POOLS[activeDomain.value]
    : SIDEBAR_GROUP_POOLS[activeDomain.value],
)

const availableItems = computed<string[]>(() => {
  const used = new Set(selectedItems.value)
  return candidatePool.value.filter((item) => !used.has(item))
})

const canAddMore = computed(() => selectedItems.value.length < MAX_LAYOUT_ITEMS)
const canRemoveSelected = computed(() =>
  activeTab.value === 'cards' ? selectedItems.value.length > 1 : selectedItems.value.length > 0,
)

function itemLabel(key: string): string {
  const scope = activeTab.value === 'cards' ? 'cards' : 'sidebar'
  return t(`layoutCustomizer.${scope}.${activeDomain.value}.${key}`)
}

function commitList(next: string[]): void {
  if (activeTab.value === 'cards') {
    themeStore.updateDomainLayout(activeDomain.value, { home_cards: next })
  } else {
    themeStore.updateDomainLayout(activeDomain.value, { sidebar_groups: next })
  }
}

function addItem(key: string): void {
  if (!canAddMore.value) return
  if (selectedItems.value.includes(key)) return
  commitList([...selectedItems.value, key])
}

function removeItem(key: string): void {
  if (!canRemoveSelected.value) return
  commitList(selectedItems.value.filter((k) => k !== key))
}

function moveItem(index: number, delta: -1 | 1): void {
  const target = index + delta
  const list = [...selectedItems.value]
  if (target < 0 || target >= list.length) return
  const [moved] = list.splice(index, 1)
  if (!moved) return
  list.splice(target, 0, moved)
  commitList(list)
}

function onDragStart(idx: number): void {
  dragIndex.value = idx
}

function onDropAt(targetIdx: number): void {
  const from = dragIndex.value
  dragIndex.value = null
  if (from === null || from === targetIdx) return
  const list = [...selectedItems.value]
  const [moved] = list.splice(from, 1)
  if (!moved) return
  list.splice(targetIdx, 0, moved)
  commitList(list)
}
</script>

<template>
  <!-- Inline Panel Mode (for SettingsView) -->
  <div v-if="inline" class="space-y-4">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="inline-flex flex-wrap gap-1 rounded-xl bg-[var(--ctp-canvas)] p-1">
        <button
          v-for="dom in domainOptions"
          :key="dom.kind"
          type="button"
          class="inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold transition"
          :class="
            activeDomain === dom.kind
              ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-xs'
              : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
          "
          @click="activeDomain = dom.kind"
        >
          <component :is="dom.icon" class="h-3.5 w-3.5" />
          <span>{{ dom.label }}</span>
        </button>
      </div>

      <div class="flex items-center gap-2">
        <div class="inline-flex gap-1 rounded-xl bg-[var(--ctp-canvas)] p-1">
          <button
            type="button"
            class="rounded-lg px-3 py-1.5 text-xs font-semibold transition"
            :class="
              activeTab === 'cards'
                ? 'bg-[var(--ctp-surface0)] text-[var(--ctp-text)]'
                : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
            "
            @click="activeTab = 'cards'"
          >
            {{ t('layoutCustomizer.tabCards') }}
          </button>
          <button
            type="button"
            class="rounded-lg px-3 py-1.5 text-xs font-semibold transition"
            :class="
              activeTab === 'sidebar'
                ? 'bg-[var(--ctp-surface0)] text-[var(--ctp-text)]'
                : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
            "
            @click="activeTab = 'sidebar'"
          >
            {{ t('layoutCustomizer.tabSidebar') }}
          </button>
        </div>

        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
          @click="themeStore.resetDomainLayout(activeDomain)"
        >
          <LuRotateCcw class="h-3.5 w-3.5" />
          <span>{{ t('layoutCustomizer.resetDomain') }}</span>
        </button>
      </div>
    </div>

    <p class="text-xs text-[var(--ctp-subtext0)]">
      {{
        activeTab === 'cards'
          ? t('layoutCustomizer.cardsHint')
          : t('layoutCustomizer.sidebarHint')
      }}
    </p>

    <!-- Selected Active Items -->
    <div class="space-y-2">
      <div class="flex items-center justify-between text-[11px] font-semibold text-[var(--ctp-subtext0)]">
        <span>{{ t('layoutCustomizer.selectedTitle') }}</span>
        <span>{{ selectedItems.length }} / {{ MAX_LAYOUT_ITEMS }}</span>
      </div>

      <div
        v-if="selectedItems.length === 0"
        class="rounded-xl border border-dashed border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/60 px-4 py-5 text-center text-xs text-[var(--ctp-subtext0)]"
      >
        {{ t('layoutCustomizer.emptySidebarSelected') }}
      </div>

      <div v-else class="grid grid-cols-1 gap-2 sm:grid-cols-2">
        <div
          v-for="(key, idx) in selectedItems"
          :key="key"
          draggable="true"
          class="flex items-center justify-between gap-2 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs transition hover:border-[var(--ctp-accent)]/60"
          @dragstart="onDragStart(idx)"
          @dragover.prevent
          @drop="onDropAt(idx)"
        >
          <div class="flex min-w-0 items-center gap-2">
            <LuGripVertical class="h-3.5 w-3.5 shrink-0 cursor-grab text-[var(--ctp-subtext0)]" />
            <span class="flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-[var(--ctp-accent-soft)] text-[11px] font-bold text-[var(--ctp-accent)]">
              {{ idx + 1 }}
            </span>
            <span class="truncate font-semibold text-[var(--ctp-text)]">{{ itemLabel(key) }}</span>
          </div>

          <div class="flex shrink-0 items-center gap-0.5">
            <button
              type="button"
              :disabled="idx === 0"
              :title="t('layoutCustomizer.moveUp')"
              class="rounded-lg p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)] disabled:opacity-30"
              @click="moveItem(idx, -1)"
            >
              <LuArrowUp class="h-3.5 w-3.5" />
            </button>
            <button
              type="button"
              :disabled="idx === selectedItems.length - 1"
              :title="t('layoutCustomizer.moveDown')"
              class="rounded-lg p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)] disabled:opacity-30"
              @click="moveItem(idx, 1)"
            >
              <LuArrowDown class="h-3.5 w-3.5" />
            </button>
            <button
              type="button"
              :disabled="!canRemoveSelected"
              :title="
                canRemoveSelected
                  ? t('layoutCustomizer.remove')
                  : t('layoutCustomizer.minOneCard')
              "
              class="rounded-lg p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-red)]/15 hover:text-[var(--ctp-red)] disabled:opacity-30"
              @click="removeItem(key)"
            >
              <LuX class="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Candidate Pool -->
    <div v-if="availableItems.length > 0" class="space-y-2 pt-1">
      <div class="flex items-center justify-between text-[11px] font-semibold text-[var(--ctp-subtext0)]">
        <span>{{ t('layoutCustomizer.poolTitle') }}</span>
        <span v-if="!canAddMore" class="text-[var(--ctp-accent)]">
          {{ t('layoutCustomizer.limitReached') }}
        </span>
      </div>
      <div class="flex flex-wrap gap-2">
        <button
          v-for="key in availableItems"
          :key="key"
          type="button"
          :disabled="!canAddMore"
          class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)] disabled:cursor-not-allowed disabled:opacity-45"
          @click="addItem(key)"
        >
          <LuPlus class="h-3.5 w-3.5" />
          <span>{{ itemLabel(key) }}</span>
        </button>
      </div>
    </div>
  </div>

  <!-- Modal Dialog Mode -->
  <Teleport v-else-if="open" to="body">
    <div
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4 backdrop-blur-xs"
      @click.self="emit('close')"
    >
      <div
        class="w-full max-w-lg rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] p-6 shadow-2xl"
      >
        <div class="flex items-start justify-between gap-3">
          <div class="flex items-center gap-2.5">
            <div
              class="flex h-9 w-9 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
            >
              <LuSlidersHorizontal class="h-4.5 w-4.5" />
            </div>
            <div>
              <h3 class="text-base font-bold text-[var(--ctp-text)]">
                {{ t('layoutCustomizer.title') }}
              </h3>
              <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">
                {{ t('layoutCustomizer.subtitle') }}
              </p>
            </div>
          </div>

          <button
            type="button"
            class="rounded-xl p-1.5 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
            @click="emit('close')"
          >
            <LuX class="h-4.5 w-4.5" />
          </button>
        </div>

        <!-- Domain Switcher + Card/Sidebar Tabs -->
        <div class="mt-5 flex flex-wrap items-center justify-between gap-2">
          <div class="inline-flex flex-wrap gap-1 rounded-xl bg-[var(--ctp-canvas)] p-1">
            <button
              v-for="dom in domainOptions"
              :key="dom.kind"
              type="button"
              class="inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs font-semibold transition"
              :class="
                activeDomain === dom.kind
                  ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-xs'
                  : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
              "
              @click="activeDomain = dom.kind"
            >
              <component :is="dom.icon" class="h-3.5 w-3.5" />
              <span>{{ dom.label }}</span>
            </button>
          </div>

          <div class="inline-flex gap-1 rounded-xl bg-[var(--ctp-canvas)] p-1">
            <button
              type="button"
              class="rounded-lg px-3 py-1.5 text-xs font-semibold transition"
              :class="
                activeTab === 'cards'
                  ? 'bg-[var(--ctp-surface0)] text-[var(--ctp-text)]'
                  : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
              "
              @click="activeTab = 'cards'"
            >
              {{ t('layoutCustomizer.tabCards') }}
            </button>
            <button
              type="button"
              class="rounded-lg px-3 py-1.5 text-xs font-semibold transition"
              :class="
                activeTab === 'sidebar'
                  ? 'bg-[var(--ctp-surface0)] text-[var(--ctp-text)]'
                  : 'text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]'
              "
              @click="activeTab = 'sidebar'"
            >
              {{ t('layoutCustomizer.tabSidebar') }}
            </button>
          </div>
        </div>

        <p class="mt-3 text-xs text-[var(--ctp-subtext0)]">
          {{
            activeTab === 'cards'
              ? t('layoutCustomizer.cardsHint')
              : t('layoutCustomizer.sidebarHint')
          }}
        </p>

        <!-- Selected Items List -->
        <div class="mt-4 space-y-2">
          <div class="flex items-center justify-between text-[11px] font-semibold text-[var(--ctp-subtext0)]">
            <span>{{ t('layoutCustomizer.selectedTitle') }}</span>
            <span>{{ selectedItems.length }} / {{ MAX_LAYOUT_ITEMS }}</span>
          </div>

          <div
            v-if="selectedItems.length === 0"
            class="rounded-xl border border-dashed border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/60 px-4 py-6 text-center text-xs text-[var(--ctp-subtext0)]"
          >
            {{ t('layoutCustomizer.emptySidebarSelected') }}
          </div>

          <div v-else class="space-y-2">
            <div
              v-for="(key, idx) in selectedItems"
              :key="key"
              draggable="true"
              class="flex items-center justify-between gap-2 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2.5 text-xs transition hover:border-[var(--ctp-accent)]/60"
              @dragstart="onDragStart(idx)"
              @dragover.prevent
              @drop="onDropAt(idx)"
            >
              <div class="flex min-w-0 items-center gap-2.5">
                <LuGripVertical class="h-4 w-4 shrink-0 cursor-grab text-[var(--ctp-subtext0)]" />
                <span class="flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-[var(--ctp-accent-soft)] text-[11px] font-bold text-[var(--ctp-accent)]">
                  {{ idx + 1 }}
                </span>
                <span class="truncate font-semibold text-[var(--ctp-text)]">{{ itemLabel(key) }}</span>
              </div>

              <div class="flex shrink-0 items-center gap-1">
                <button
                  type="button"
                  :disabled="idx === 0"
                  :title="t('layoutCustomizer.moveUp')"
                  class="rounded-lg p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)] disabled:opacity-30"
                  @click="moveItem(idx, -1)"
                >
                  <LuArrowUp class="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  :disabled="idx === selectedItems.length - 1"
                  :title="t('layoutCustomizer.moveDown')"
                  class="rounded-lg p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)] disabled:opacity-30"
                  @click="moveItem(idx, 1)"
                >
                  <LuArrowDown class="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  :disabled="!canRemoveSelected"
                  :title="
                    canRemoveSelected
                      ? t('layoutCustomizer.remove')
                      : t('layoutCustomizer.minOneCard')
                  "
                  class="rounded-lg p-1 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-red)]/15 hover:text-[var(--ctp-red)] disabled:opacity-30"
                  @click="removeItem(key)"
                >
                  <LuX class="h-3.5 w-3.5" />
                </button>
              </div>
            </div>
          </div>
        </div>

        <!-- Candidate Pool -->
        <div v-if="availableItems.length > 0" class="mt-4 space-y-2">
          <div class="flex items-center justify-between text-[11px] font-semibold text-[var(--ctp-subtext0)]">
            <span>{{ t('layoutCustomizer.poolTitle') }}</span>
            <span v-if="!canAddMore" class="text-[var(--ctp-accent)]">
              {{ t('layoutCustomizer.limitReached') }}
            </span>
          </div>
          <div class="flex flex-wrap gap-2">
            <button
              v-for="key in availableItems"
              :key="key"
              type="button"
              :disabled="!canAddMore"
              class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)] disabled:cursor-not-allowed disabled:opacity-45"
              @click="addItem(key)"
            >
              <LuPlus class="h-3.5 w-3.5" />
              <span>{{ itemLabel(key) }}</span>
            </button>
          </div>
        </div>

        <div class="mt-6 flex items-center justify-between border-t border-[var(--ctp-surface0)] pt-4">
          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 py-2 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
            @click="themeStore.resetDomainLayout(activeDomain)"
          >
            <LuRotateCcw class="h-3.5 w-3.5" />
            <span>{{ t('layoutCustomizer.resetDomain') }}</span>
          </button>

          <button
            type="button"
            class="rounded-xl bg-[var(--ctp-accent)] px-5 py-2 text-xs font-bold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95"
            @click="emit('close')"
          >
            {{ t('common.confirm') }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
