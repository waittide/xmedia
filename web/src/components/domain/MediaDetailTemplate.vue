<script setup lang="ts">
import { computed, ref, useSlots, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink, type RouteLocationRaw } from 'vue-router'
import {
  LuAlertTriangle,
  LuArrowLeft,
  LuBookOpen,
  LuChevronDown,
  LuChevronUp,
  LuHeart,
  LuMoreVertical,
  LuPlay,
} from 'vue-icons-plus/lu'
import {
  MdButton,
  MdChip,
  MdIconButton,
  MdLinearProgress,
  MdMenu,
  MdSkeleton,
  MdTabs,
  MediaCoverImage,
  type MdTabItem,
} from '@/components/ui'
import MediaErrorState from './MediaErrorState.vue'
import type { MediaItemCardAspect, MediaItemCoverStyle } from './MediaItemCard.vue'

export interface MediaDetailTag {
  label: string
  to?: RouteLocationRaw
  value?: string
}

export interface MediaDetailSpec {
  label: string
  value: string
  mono?: boolean
}

const MAX_COLLAPSED_TAGS = 5

const props = withDefaults(
  defineProps<{
    title: string
    subtitle?: string
    cover?: string | null
    coverAlt?: string
    aspect?: MediaItemCardAspect
    coverStyle?: MediaItemCoverStyle
    fallbackIcon?: Component
    showBack?: boolean
    backLabel?: string
    backTo?: RouteLocationRaw
    /** Hidden when 0 or null so unread items keep mobile Hero compact. */
    progressPercent?: number | null
    progressLabel?: string
    /** The single high-emphasis filled primary CTA. */
    primaryActionLabel?: string
    primaryActionIcon?: Component
    primaryActionTo?: RouteLocationRaw
    primaryActionDisabled?: boolean
    primaryActionLoading?: boolean
    /** Optional single tonal secondary action (e.g. Shuffle / Add to shelf). */
    secondaryActionLabel?: string
    secondaryActionIcon?: Component
    secondaryActionTo?: RouteLocationRaw
    secondaryActionDisabled?: boolean
    favorite?: boolean
    showFavorite?: boolean
    favoriteLoading?: boolean
    description?: string | null
    tags?: readonly MediaDetailTag[]
    specs?: readonly MediaDetailSpec[]
    specsTitle?: string
    /** Rendered only when > 1 tab exists. */
    tabs?: MdTabItem<string>[]
    activeTab?: string
    loading?: boolean
    error?: string | null
    availabilityWarning?: string | null
    maxWidthClass?: string
  }>(),
  {
    subtitle: '',
    cover: null,
    coverAlt: '',
    aspect: 'poster',
    coverStyle: 'plain',
    fallbackIcon: undefined,
    showBack: true,
    backLabel: '',
    backTo: undefined,
    progressPercent: null,
    progressLabel: '',
    primaryActionLabel: '',
    primaryActionIcon: undefined,
    primaryActionTo: undefined,
    primaryActionDisabled: false,
    primaryActionLoading: false,
    secondaryActionLabel: '',
    secondaryActionIcon: undefined,
    secondaryActionTo: undefined,
    secondaryActionDisabled: false,
    favorite: false,
    showFavorite: false,
    favoriteLoading: false,
    description: null,
    tags: () => [],
    specs: () => [],
    specsTitle: '',
    tabs: () => [],
    activeTab: '',
    loading: false,
    error: null,
    availabilityWarning: null,
    maxWidthClass: 'max-w-5xl',
  },
)

const emit = defineEmits<{
  back: []
  'primary-action': []
  'secondary-action': []
  'toggle-favorite': []
  'tag-click': [tag: MediaDetailTag]
  'update:activeTab': [tab: string]
  retry: []
}>()

const slots = useSlots()
const { locale } = useI18n()
const isZh = computed(() => locale.value.startsWith('zh'))

const descriptionExpanded = ref(false)
const tagsExpanded = ref(false)
const specsExpanded = ref(false)

const resolvedBackLabel = computed(() => props.backLabel || (isZh.value ? '返回' : 'Back'))
const resolvedPrimaryIcon = computed(() => props.primaryActionIcon ?? LuPlay)
const resolvedFallbackIcon = computed(() => props.fallbackIcon ?? LuBookOpen)
const resolvedSpecsTitle = computed(() => props.specsTitle || (isZh.value ? '详细信息' : 'Details'))

const hasOverflowMenu = computed(
  () => Boolean(slots['overflow-menu']) || Boolean(slots['overflow-danger']),
)

const normalizedProgress = computed<number | null>(() => {
  const raw = props.progressPercent
  if (raw === null || raw === undefined || !Number.isFinite(raw) || raw <= 0) return null
  return Math.min(100, Math.max(1, Math.round(raw)))
})

const aspectClass = computed(() => {
  switch (props.aspect) {
    case 'square':
      return 'aspect-square'
    case 'video':
      return 'aspect-video'
    case 'poster':
    default:
      return 'aspect-[2/3]'
  }
})

const coverSizeClass = computed(() => {
  switch (props.aspect) {
    case 'video':
      return 'w-full sm:w-72 md:w-80'
    case 'square':
      return 'w-28 sm:w-44 md:w-52'
    case 'poster':
    default:
      return 'w-28 sm:w-44 md:w-52'
  }
})

const coverEffectClass = computed(() => {
  if (props.aspect === 'video') return ''
  if (props.coverStyle === 'book') return 'book-shadow'
  if (props.coverStyle === 'album') return 'album-shadow'
  return ''
})

const trimmedDescription = computed(() => props.description?.trim() ?? '')
const canCollapseDescription = computed(() => {
  const text = trimmedDescription.value
  if (!text) return false
  return text.length > 150 || text.split(/\r?\n/).length > 3
})

const visibleTags = computed(() => {
  if (tagsExpanded.value || props.tags.length <= MAX_COLLAPSED_TAGS) {
    return props.tags
  }
  return props.tags.slice(0, MAX_COLLAPSED_TAGS)
})

const hiddenTagsCount = computed(() => Math.max(0, props.tags.length - MAX_COLLAPSED_TAGS))

const favoriteAriaLabel = computed(() =>
  props.favorite
    ? isZh.value
      ? '取消收藏'
      : 'Remove from favorites'
    : isZh.value
      ? '收藏'
      : 'Add to favorites',
)

const moreAriaLabel = computed(() => (isZh.value ? '更多操作' : 'More actions'))
const expandTextLabel = computed(() => (isZh.value ? '展开全部' : 'Show more'))
const collapseTextLabel = computed(() => (isZh.value ? '收起' : 'Show less'))
</script>

<template>
  <div class="mx-auto w-full space-y-6" :class="maxWidthClass">
    <!-- Minimal Back Bar -->
    <div v-if="showBack" class="flex items-center">
      <MdButton
        variant="text"
        size="sm"
        :icon="LuArrowLeft"
        :to="backTo"
        @click="!backTo && emit('back')"
      >
        {{ resolvedBackLabel }}
      </MdButton>
    </div>

    <!-- Error State -->
    <MediaErrorState
      v-if="error"
      :message="error"
      :retrying="loading"
      @retry="emit('retry')"
    />

    <!-- Loading Skeleton -->
    <div v-else-if="loading" class="space-y-6" aria-busy="true">
      <div class="flex flex-col gap-5 sm:flex-row">
        <MdSkeleton
          variant="rect"
          :aspect="aspect"
          class="shrink-0 rounded-corner-md"
          :class="coverSizeClass"
        />
        <div class="flex-1 space-y-3 pt-1">
          <MdSkeleton variant="text" width="60%" height="2rem" />
          <MdSkeleton variant="text" width="40%" height="1rem" />
          <div class="flex gap-2 pt-2">
            <MdSkeleton variant="rect" width="8rem" height="2.5rem" class="rounded-corner-full" />
            <MdSkeleton variant="circle" width="2.5rem" height="2.5rem" />
          </div>
          <MdSkeleton variant="text" :lines="3" class="pt-2" />
        </div>
      </div>
    </div>

    <!-- Main Detail Content -->
    <template v-else>
      <!-- Availability Warning (disabled/missing source for tracked online items) -->
      <div
        v-if="availabilityWarning"
        role="alert"
        class="flex flex-wrap items-center justify-between gap-3 rounded-corner-md bg-warning-container px-4 py-3 text-on-warning-container"
      >
        <div class="flex min-w-0 flex-1 items-center gap-2.5">
          <LuAlertTriangle class="h-4 w-4 shrink-0" aria-hidden="true" />
          <p class="text-body-medium min-w-0 flex-1">
            {{ availabilityWarning }}
          </p>
        </div>
        <div v-if="$slots['warning-actions']" class="flex shrink-0 items-center gap-2">
          <slot name="warning-actions" />
        </div>
      </div>

      <!-- Frameless Hero: compact row on 390px mobile so primary CTA stays above the fold -->
      <section class="flex flex-col gap-4 sm:flex-row sm:gap-6">
        <!-- Mobile Poster/Square compact header row + Desktop left cover -->
        <div
          class="flex gap-4 sm:contents"
          :class="aspect === 'video' ? 'flex-col' : 'flex-row items-start'"
        >
          <div
            class="shrink-0 overflow-hidden rounded-corner-md bg-surface-container-high"
            :class="[coverSizeClass, coverEffectClass]"
          >
            <MediaCoverImage
              :src="cover"
              :alt="coverAlt || title"
              :aspect-class="aspectClass"
              rounded="rounded-corner-md"
              bg-class="bg-surface-container-high"
              :fallback-icon="resolvedFallbackIcon"
              fallback-icon-class="h-10 w-10 text-on-surface-variant/50"
            />
          </div>

          <!-- Mobile-only title beside compact cover -->
          <div class="min-w-0 flex-1 space-y-1 sm:hidden">
            <h1 class="text-title-large line-clamp-3 text-on-surface">
              {{ title }}
            </h1>
            <p v-if="subtitle" class="text-body-small line-clamp-2 text-on-surface-variant">
              {{ subtitle }}
            </p>
            <slot name="hero-meta" />
          </div>
        </div>

        <!-- Hero Right Info & Action Bar -->
        <div class="flex min-w-0 flex-1 flex-col justify-between gap-4">
          <div class="space-y-3">
            <!-- Desktop Title + 1-line Subtitle -->
            <div class="hidden space-y-1 sm:block">
              <h1 class="text-headline-medium text-on-surface">
                {{ title }}
              </h1>
              <p v-if="subtitle" class="text-body-medium text-on-surface-variant">
                {{ subtitle }}
              </p>
              <slot name="hero-meta" />
            </div>

            <!-- Compact Progress Bar (ONLY shown when progress > 0) -->
            <div v-if="normalizedProgress !== null" class="max-w-md space-y-1">
              <div class="flex items-center justify-between gap-2">
                <span class="text-label-medium truncate text-on-surface-variant">
                  {{ progressLabel || `${normalizedProgress}%` }}
                </span>
                <span
                  v-if="progressLabel"
                  class="text-label-small shrink-0 tabular-nums text-on-surface-variant"
                >
                  {{ normalizedProgress }}%
                </span>
              </div>
              <MdLinearProgress
                :value="normalizedProgress"
                :aria-label="progressLabel || `${normalizedProgress}%`"
              />
            </div>

            <!-- Action Row: 1 Primary CTA + optional 1 secondary + favorite + overflow menu -->
            <div class="flex flex-wrap items-center gap-2 pt-0.5">
              <MdButton
                v-if="primaryActionLabel"
                variant="filled"
                :icon="resolvedPrimaryIcon"
                :to="primaryActionTo"
                :disabled="primaryActionDisabled"
                :loading="primaryActionLoading"
                class="flex-1 sm:flex-none"
                @click="!primaryActionTo && emit('primary-action')"
              >
                {{ primaryActionLabel }}
              </MdButton>

              <MdButton
                v-if="secondaryActionLabel"
                variant="tonal"
                :icon="secondaryActionIcon"
                :to="secondaryActionTo"
                :disabled="secondaryActionDisabled"
                @click="!secondaryActionTo && emit('secondary-action')"
              >
                {{ secondaryActionLabel }}
              </MdButton>

              <MdIconButton
                v-if="showFavorite"
                variant="tonal"
                :selected="favorite"
                :loading="favoriteLoading"
                :aria-label="favoriteAriaLabel"
                :tooltip="favoriteAriaLabel"
                @click="emit('toggle-favorite')"
              >
                <LuHeart class="h-5 w-5" :class="favorite ? 'fill-current text-tertiary' : ''" />
              </MdIconButton>

              <slot name="secondary-actions" />

              <MdMenu v-if="hasOverflowMenu" placement="bottom-end" :aria-label="moreAriaLabel">
                <template #trigger="{ toggle, triggerAttrs }">
                  <MdIconButton
                    v-bind="triggerAttrs"
                    variant="standard"
                    :icon="LuMoreVertical"
                    :aria-label="moreAriaLabel"
                    :tooltip="moreAriaLabel"
                    @click="toggle"
                  />
                </template>
                <template #default="{ close }">
                  <slot name="overflow-menu" :close="close" />
                  <div
                    v-if="$slots['overflow-danger']"
                    class="my-1 border-t border-outline-variant pt-1"
                    role="separator"
                  >
                    <slot name="overflow-danger" :close="close" />
                  </div>
                </template>
              </MdMenu>
            </div>
          </div>

          <!-- Collapsible Synopsis -->
          <div v-if="trimmedDescription" class="space-y-1">
            <p
              class="text-body-medium whitespace-pre-line text-on-surface-variant"
              :class="descriptionExpanded ? '' : 'line-clamp-3'"
            >
              {{ trimmedDescription }}
            </p>
            <MdButton
              v-if="canCollapseDescription"
              variant="text"
              size="sm"
              class="-ml-2.5"
              @click="descriptionExpanded = !descriptionExpanded"
            >
              {{ descriptionExpanded ? collapseTextLabel : expandTextLabel }}
            </MdButton>
          </div>

          <!-- Restrained Tag Chips (max 5 visible by default) -->
          <div v-if="tags.length > 0" class="flex flex-wrap items-center gap-1.5">
            <template v-for="tag in visibleTags" :key="tag.value ?? tag.label">
              <RouterLink v-if="tag.to" :to="tag.to" class="inline-flex">
                <MdChip variant="assist" :label="tag.label" />
              </RouterLink>
              <MdChip
                v-else
                variant="assist"
                :label="tag.label"
                @click="emit('tag-click', tag)"
              />
            </template>
            <MdChip
              v-if="hiddenTagsCount > 0"
              variant="suggestion"
              :label="tagsExpanded ? collapseTextLabel : `+${hiddenTagsCount}`"
              @click="tagsExpanded = !tagsExpanded"
            />
          </div>
        </div>
      </section>

      <slot name="after-hero" />

      <!-- Optional Content Tabs (only shown when > 1 tab exists) -->
      <MdTabs
        v-if="tabs.length > 1"
        :model-value="activeTab"
        :tabs="tabs"
        variant="primary"
        @update:model-value="emit('update:activeTab', $event)"
      />

      <!-- Main Detail Body (Chapters / Episodes / Tracks / Tab Panels) -->
      <div v-if="$slots.default" class="space-y-4">
        <slot />
      </div>

      <!-- Collapsible Technical Specs (collapsed by default) -->
      <section v-if="specs.length > 0" class="border-t border-outline-variant pt-4">
        <button
          type="button"
          :aria-expanded="specsExpanded"
          class="md-state text-title-small inline-flex cursor-pointer items-center gap-2 rounded-corner-full px-3 py-2 text-on-surface-variant hover:text-on-surface"
          @click="specsExpanded = !specsExpanded"
        >
          <span>{{ resolvedSpecsTitle }}</span>
          <LuChevronUp v-if="specsExpanded" class="h-4 w-4" aria-hidden="true" />
          <LuChevronDown v-else class="h-4 w-4" aria-hidden="true" />
        </button>

        <dl
          v-if="specsExpanded"
          class="mt-3 grid grid-cols-1 gap-x-6 gap-y-3 rounded-corner-md bg-surface-container-low p-4 sm:grid-cols-2"
        >
          <div v-for="spec in specs" :key="spec.label" class="min-w-0">
            <dt class="text-label-small text-on-surface-variant">
              {{ spec.label }}
            </dt>
            <dd
              class="text-body-medium mt-0.5 break-words text-on-surface"
              :class="spec.mono ? 'font-mono' : ''"
            >
              {{ spec.value }}
            </dd>
          </div>
        </dl>
      </section>
    </template>
  </div>
</template>
