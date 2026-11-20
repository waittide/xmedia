<script setup lang="ts">
import { computed, defineComponent, h, ref, watch } from 'vue'
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
import MdDialog from '@/components/ui/MdDialog.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
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

const PanelShell = defineComponent({
  name: 'LayoutCustomizerShell',
  props: {
    inline: { type: Boolean, required: true },
    open: { type: Boolean, required: true },
    title: { type: String, required: true },
  },
  emits: ['close'],
  setup(shellProps, { slots, emit: shellEmit }) {
    return () =>
      shellProps.inline
        ? h('div', { class: 'space-y-4' }, slots.default?.())
        : h(
            MdDialog,
            {
              open: shellProps.open,
              title: shellProps.title,
              maxWidth: 'max-w-lg',
              onClose: () => shellEmit('close'),
            },
            {
              icon: slots.icon,
              default: () => h('div', { class: 'space-y-4 py-1' }, slots.default?.()),
              footer: slots.footer,
            },
          )
  },
})

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
  { immediate: true },
)

const domainOptions = computed(() => {
  const all: Array<{ value: MediaKind; label: string; icon: typeof LuBookOpen }> = [
    { value: 'book', label: t('app.domains.book'), icon: LuBookOpen },
    { value: 'comic', label: t('app.domains.comic'), icon: LuLayers },
    { value: 'music', label: t('app.domains.music'), icon: LuDisc3 },
    { value: 'video', label: t('app.domains.video'), icon: LuClapperboard },
  ]
  return all.filter((d) => authStore.hasModule(d.value))
})

const tabOptions = computed<Array<{ value: 'cards' | 'sidebar'; label: string }>>(() => [
  { value: 'cards', label: t('layoutCustomizer.tabCards') },
  { value: 'sidebar', label: t('layoutCustomizer.tabSidebar') },
])

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
  <PanelShell
    :inline="inline"
    :open="open"
    :title="t('layoutCustomizer.title')"
    @close="emit('close')"
  >
    <template #icon>
      <LuSlidersHorizontal class="h-5 w-5" />
    </template>

    <!-- Domain Switcher & Cards/Sidebar Tabs -->
    <div class="flex flex-wrap items-center justify-between gap-2.5">
      <div
        v-if="domainOptions.length > 1"
        class="inline-flex flex-wrap gap-1 rounded-full bg-[var(--md-sys-color-surface-container)] p-1"
      >
        <button
          v-for="dom in domainOptions"
          :key="dom.value"
          type="button"
          class="md-state inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-medium transition"
          :class="
            activeDomain === dom.value
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
          "
          @click="activeDomain = dom.value"
        >
          <component :is="dom.icon" class="h-3.5 w-3.5" />
          <span>{{ dom.label }}</span>
        </button>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <MdSegmentedButton
          v-model="activeTab"
          :options="tabOptions"
          size="sm"
        />

        <button
          v-if="inline"
          type="button"
          class="md-btn md-btn-sm md-btn-outlined md-state"
          @click="themeStore.resetDomainLayout(activeDomain)"
        >
          <LuRotateCcw class="h-3.5 w-3.5" />
          <span>{{ t('layoutCustomizer.resetDomain') }}</span>
        </button>
      </div>
    </div>

    <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
      {{
        activeTab === 'cards'
          ? t('layoutCustomizer.cardsHint')
          : t('layoutCustomizer.sidebarHint')
      }}
    </p>

    <!-- Selected Active Items -->
    <div class="space-y-2">
      <div
        class="flex items-center justify-between text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
      >
        <span>{{ t('layoutCustomizer.selectedTitle') }}</span>
        <span class="tabular-nums">{{ selectedItems.length }} / {{ MAX_LAYOUT_ITEMS }}</span>
      </div>

      <div
        v-if="selectedItems.length === 0"
        class="rounded-2xl border border-dashed border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] px-4 py-6 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('layoutCustomizer.emptySidebarSelected') }}
      </div>

      <div
        v-else
        :class="inline ? 'grid grid-cols-1 gap-2 sm:grid-cols-2' : 'space-y-2'"
      >
        <div
          v-for="(key, idx) in selectedItems"
          :key="key"
          draggable="true"
          class="flex items-center justify-between gap-2 rounded-xl border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface-container)] px-3 py-2 text-xs transition hover:border-[var(--md-sys-color-primary)]/60"
          :class="dragIndex === idx ? 'opacity-60' : ''"
          @dragstart="onDragStart(idx)"
          @dragover.prevent
          @dragend="dragIndex = null"
          @drop="onDropAt(idx)"
        >
          <div class="flex min-w-0 items-center gap-2">
            <LuGripVertical
              class="h-3.5 w-3.5 shrink-0 cursor-grab text-[var(--md-sys-color-on-surface-variant)]"
            />
            <span
              class="flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-[var(--md-sys-color-primary-container)] text-[11px] font-bold text-[var(--md-sys-color-on-primary-container)]"
            >
              {{ idx + 1 }}
            </span>
            <span class="truncate font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ itemLabel(key) }}
            </span>
          </div>

          <div class="flex shrink-0 items-center gap-0.5">
            <button
              type="button"
              :disabled="idx === 0"
              :title="t('layoutCustomizer.moveUp')"
              :aria-label="t('layoutCustomizer.moveUp')"
              class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7 disabled:opacity-30"
              @click="moveItem(idx, -1)"
            >
              <LuArrowUp class="h-3.5 w-3.5" />
            </button>
            <button
              type="button"
              :disabled="idx === selectedItems.length - 1"
              :title="t('layoutCustomizer.moveDown')"
              :aria-label="t('layoutCustomizer.moveDown')"
              class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7 disabled:opacity-30"
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
              :aria-label="t('layoutCustomizer.remove')"
              class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7 hover:text-[var(--md-sys-color-error)] disabled:opacity-30"
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
      <div
        class="flex items-center justify-between text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
      >
        <span>{{ t('layoutCustomizer.poolTitle') }}</span>
        <span v-if="!canAddMore" class="text-[var(--md-sys-color-primary)]">
          {{ t('layoutCustomizer.limitReached') }}
        </span>
      </div>
      <div class="flex flex-wrap gap-2">
        <button
          v-for="key in availableItems"
          :key="key"
          type="button"
          :disabled="!canAddMore"
          class="md-state inline-flex items-center gap-1.5 rounded-full border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] px-3 py-1.5 text-xs font-medium text-[var(--md-sys-color-on-surface-variant)] transition hover:border-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-primary)] disabled:cursor-not-allowed disabled:opacity-45"
          @click="addItem(key)"
        >
          <LuPlus class="h-3.5 w-3.5" />
          <span>{{ itemLabel(key) }}</span>
        </button>
      </div>
    </div>

    <template #footer>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-outlined md-state"
        @click="themeStore.resetDomainLayout(activeDomain)"
      >
        <LuRotateCcw class="h-3.5 w-3.5" />
        <span>{{ t('layoutCustomizer.resetDomain') }}</span>
      </button>

      <button
        type="button"
        class="md-btn md-btn-sm md-btn-filled md-state"
        @click="emit('close')"
      >
        {{ t('common.confirm') }}
      </button>
    </template>
  </PanelShell>
</template>
