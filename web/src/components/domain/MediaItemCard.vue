<script setup lang="ts">
import { computed, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import { RouterLink, type RouteLocationRaw } from 'vue-router'
import {
  LuAlertTriangle,
  LuBookOpen,
  LuFilm,
  LuHeart,
  LuImage,
  LuMoreVertical,
  LuMusic,
} from 'vue-icons-plus/lu'
import { MdIconButton, MdLinearProgress, MdMenu, MediaCoverImage } from '@/components/ui'
import { isAvailabilityBlocked, type MediaItemVM } from '@/sources'

export type MediaItemCardAspect = 'poster' | 'square' | 'video'
export type MediaItemCardLayout = 'vertical' | 'horizontal'
export type MediaItemCoverStyle = 'plain' | 'book' | 'album'

const props = withDefaults(
  defineProps<{
    item: MediaItemVM
    aspect?: MediaItemCardAspect
    layout?: MediaItemCardLayout
    coverStyle?: MediaItemCoverStyle
    fallbackIcon?: Component
    favorite?: boolean
    showFavorite?: boolean
    favoriteLoading?: boolean
    /** Show source origin chip below subtitle. Defaults to auto-showing for non-local items. */
    showSourceBadge?: boolean
    /** Single optional duration/length pill placed at cover bottom-right (e.g. video duration). */
    durationLabel?: string
    /** Prefer `item.playTo` when available and not availability-blocked (e.g. Continue rail). */
    preferPlayRoute?: boolean
    /** Force button rendering instead of RouterLink (e.g. custom click behavior). */
    asButton?: boolean
    disabled?: boolean
  }>(),
  {
    aspect: 'poster',
    layout: 'vertical',
    coverStyle: 'plain',
    fallbackIcon: undefined,
    favorite: undefined,
    showFavorite: false,
    favoriteLoading: false,
    showSourceBadge: undefined,
    durationLabel: '',
    preferPlayRoute: false,
    asButton: false,
    disabled: false,
  },
)

const emit = defineEmits<{
  select: [item: MediaItemVM]
  'toggle-favorite': [item: MediaItemVM]
}>()

const { locale } = useI18n()
const isZh = computed(() => locale.value.startsWith('zh'))

const isBlocked = computed(
  () => Boolean(props.item.badge?.blocked) || isAvailabilityBlocked(props.item.badge?.availability),
)

const blockedLabel = computed(() => {
  if (!isBlocked.value) return ''
  if (props.item.badge?.availability === 'source_missing') {
    return isZh.value ? '来源已删除' : 'Source missing'
  }
  return isZh.value ? '来源已停用' : 'Source disabled'
})

const resolvedTo = computed<RouteLocationRaw | undefined>(() => {
  if (props.asButton || props.disabled) return undefined
  if (props.preferPlayRoute && props.item.playTo && !isBlocked.value) {
    return props.item.playTo
  }
  return props.item.to
})

const resolvedFallbackIcon = computed<Component>(() => {
  if (props.fallbackIcon) return props.fallbackIcon
  switch (props.item.ref.domain) {
    case 'comic':
      return LuImage
    case 'music':
      return LuMusic
    case 'video':
      return LuFilm
    case 'book':
    default:
      return LuBookOpen
  }
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

const horizontalCoverClass = computed(() => {
  switch (props.aspect) {
    case 'square':
      return 'h-16 w-16'
    case 'video':
      return 'h-16 w-28'
    case 'poster':
    default:
      return 'h-20 w-14'
  }
})

const coverEffectClass = computed(() => {
  if (props.layout === 'horizontal') return ''
  if (props.coverStyle === 'book') return 'book-shadow'
  if (props.coverStyle === 'album') return 'album-shadow'
  return ''
})

const progressPercent = computed<number | null>(() => {
  const raw = props.item.badge?.progressPercent
  if (raw === undefined || raw === null || !Number.isFinite(raw) || raw <= 0) return null
  return Math.min(100, Math.max(1, Math.round(raw)))
})

const sourceBadgeText = computed(() => {
  const explicit = props.showSourceBadge
  const label = props.item.badge?.source?.trim() || ''
  if (!label) return ''
  if (explicit === false) return ''
  if (explicit === true) return label
  return props.item.ref.scope !== 'local' ? label : ''
})

const shouldShowFavorite = computed(() => props.showFavorite || props.favorite !== undefined)

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

function onPrimaryClick(): void {
  if (props.disabled) return
  emit('select', props.item)
}

function onFavoriteClick(): void {
  if (props.disabled || props.favoriteLoading) return
  emit('toggle-favorite', props.item)
}
</script>

<template>
  <!-- Horizontal Continue / Compact Row Card -->
  <article
    v-if="layout === 'horizontal'"
    class="md-state group relative flex items-center gap-3 rounded-corner-md bg-surface-container-highest p-2.5 text-on-surface"
    :class="[isBlocked ? 'opacity-60' : '', disabled ? 'pointer-events-none opacity-38' : '']"
  >
    <div class="relative shrink-0 overflow-hidden rounded-corner-sm" :class="horizontalCoverClass">
      <MediaCoverImage
        :src="item.cover"
        :alt="item.title"
        :aspect-class="aspectClass"
        rounded="rounded-corner-sm"
        bg-class="bg-surface-container-high"
        :fallback-icon="resolvedFallbackIcon"
        fallback-icon-class="h-5 w-5 text-on-surface-variant/60"
        class="h-full w-full"
      />
    </div>

    <div class="min-w-0 flex-1">
      <component
        :is="resolvedTo ? RouterLink : 'button'"
        :to="resolvedTo"
        :type="resolvedTo ? undefined : 'button'"
        :disabled="!resolvedTo && disabled ? true : undefined"
        class="block w-full text-left outline-none after:absolute after:inset-0 after:rounded-corner-md focus-visible:after:outline-3 focus-visible:after:outline-offset-2 focus-visible:after:outline-secondary"
        @click="onPrimaryClick"
      >
        <h3 class="text-title-small truncate text-on-surface">
          {{ item.title }}
        </h3>
        <p v-if="item.subtitle" class="text-body-small mt-0.5 truncate text-on-surface-variant">
          {{ item.subtitle }}
        </p>
      </component>

      <div
        v-if="isBlocked || sourceBadgeText || item.badge?.status"
        class="mt-1 flex flex-wrap items-center gap-1.5"
      >
        <span
          v-if="isBlocked"
          class="text-label-small inline-flex items-center gap-1 rounded-corner-xs bg-error-container px-1.5 py-0.5 text-on-error-container"
        >
          <LuAlertTriangle class="h-3 w-3 shrink-0" aria-hidden="true" />
          <span>{{ blockedLabel }}</span>
        </span>
        <span
          v-else-if="sourceBadgeText"
          class="text-label-small max-w-full truncate rounded-corner-xs bg-surface-container-low px-1.5 py-0.5 text-on-surface-variant"
        >
          {{ sourceBadgeText }}
        </span>
        <span
          v-if="!isBlocked && item.badge?.status"
          class="text-label-small truncate text-on-surface-variant"
        >
          {{ item.badge.status }}
        </span>
      </div>

      <div v-if="progressPercent !== null" class="mt-2">
        <MdLinearProgress
          :value="progressPercent"
          :aria-label="`${item.title} ${progressPercent}%`"
        />
      </div>
    </div>

    <div v-if="shouldShowFavorite || $slots.menu || $slots.actions" class="relative z-10 flex shrink-0 items-center gap-0.5">
      <slot name="actions" />
      <MdIconButton
        v-if="shouldShowFavorite"
        size="sm"
        variant="standard"
        :selected="Boolean(favorite)"
        :loading="favoriteLoading"
        :aria-label="favoriteAriaLabel"
        :class="favorite ? 'text-tertiary' : ''"
        @click.stop="onFavoriteClick"
      >
        <LuHeart class="h-4 w-4" :class="favorite ? 'fill-current text-tertiary' : ''" />
      </MdIconButton>
      <MdMenu v-if="$slots.menu" placement="bottom-end" :aria-label="moreAriaLabel">
        <template #trigger="{ toggle, triggerAttrs }">
          <MdIconButton
            v-bind="triggerAttrs"
            size="sm"
            variant="standard"
            :icon="LuMoreVertical"
            :aria-label="moreAriaLabel"
            @click.stop="toggle"
          />
        </template>
        <template #default="{ close }">
          <slot name="menu" :close="close" />
        </template>
      </MdMenu>
    </div>
  </article>

  <!-- Vertical Borderless MD3 Media Poster/Square/Landscape Card -->
  <article
    v-else
    class="group relative flex flex-col text-on-surface"
    :class="[isBlocked ? 'opacity-60' : '', disabled ? 'pointer-events-none opacity-38' : '']"
  >
    <div
      class="relative overflow-hidden rounded-corner-md bg-surface-container-high transition-shadow duration-200 ease-standard group-hover:shadow-elevation-1"
      :class="coverEffectClass"
    >
      <MediaCoverImage
        :src="item.cover"
        :alt="item.title"
        :aspect-class="aspectClass"
        rounded="rounded-corner-md"
        bg-class="bg-surface-container-high"
        :fallback-icon="resolvedFallbackIcon"
        fallback-icon-class="h-10 w-10 text-on-surface-variant/50"
      >
        <!-- Single subtle overlay slot: blocked status OR duration pill -->
        <slot name="cover-badge">
          <span
            v-if="isBlocked"
            class="text-label-small pointer-events-none absolute top-2 left-2 inline-flex items-center gap-1 rounded-corner-xs bg-error-container/95 px-2 py-0.5 text-on-error-container"
          >
            <LuAlertTriangle class="h-3 w-3 shrink-0" aria-hidden="true" />
            <span>{{ blockedLabel }}</span>
          </span>
          <span
            v-else-if="durationLabel"
            class="text-label-small pointer-events-none absolute right-2 bottom-2 rounded-corner-xs bg-scrim/75 px-1.5 py-0.5 tabular-nums text-white"
          >
            {{ durationLabel }}
          </span>
        </slot>

        <!-- Bottom-edge reading/watching progress bar -->
        <div
          v-if="progressPercent !== null"
          class="pointer-events-none absolute right-0 bottom-0 left-0"
        >
          <MdLinearProgress
            :value="progressPercent"
            :aria-label="`${item.title} ${progressPercent}%`"
          />
        </div>
      </MediaCoverImage>
    </div>

    <div class="mt-2 flex items-start gap-1">
      <div class="min-w-0 flex-1">
        <component
          :is="resolvedTo ? RouterLink : 'button'"
          :to="resolvedTo"
          :type="resolvedTo ? undefined : 'button'"
          :disabled="!resolvedTo && disabled ? true : undefined"
          class="block w-full text-left outline-none after:absolute after:inset-0 after:rounded-corner-md focus-visible:after:outline-3 focus-visible:after:outline-offset-2 focus-visible:after:outline-secondary"
          @click="onPrimaryClick"
        >
          <h3 class="text-title-small line-clamp-2 text-on-surface">
            {{ item.title }}
          </h3>
          <p v-if="item.subtitle" class="text-body-small mt-0.5 truncate text-on-surface-variant">
            {{ item.subtitle }}
          </p>
        </component>

        <div
          v-if="sourceBadgeText || (!isBlocked && item.badge?.status) || $slots.meta"
          class="mt-1 flex flex-wrap items-center gap-1.5"
        >
          <span
            v-if="sourceBadgeText"
            class="text-label-small max-w-full truncate rounded-corner-xs bg-surface-container-high px-1.5 py-0.5 text-on-surface-variant"
          >
            {{ sourceBadgeText }}
          </span>
          <span
            v-if="!isBlocked && item.badge?.status"
            class="text-label-small truncate text-on-surface-variant"
          >
            {{ item.badge.status }}
          </span>
          <slot name="meta" />
        </div>
      </div>

      <div v-if="shouldShowFavorite || $slots.menu || $slots.actions" class="relative z-10 -mr-1.5 flex shrink-0 items-center">
        <slot name="actions" />
        <MdIconButton
          v-if="shouldShowFavorite"
          size="sm"
          variant="standard"
          :selected="Boolean(favorite)"
          :loading="favoriteLoading"
          :aria-label="favoriteAriaLabel"
          :class="favorite ? 'text-tertiary' : ''"
          @click.stop="onFavoriteClick"
        >
          <LuHeart class="h-4 w-4" :class="favorite ? 'fill-current text-tertiary' : ''" />
        </MdIconButton>
        <MdMenu v-if="$slots.menu" placement="bottom-end" :aria-label="moreAriaLabel">
          <template #trigger="{ toggle, triggerAttrs }">
            <MdIconButton
              v-bind="triggerAttrs"
              size="sm"
              variant="standard"
              :icon="LuMoreVertical"
              :aria-label="moreAriaLabel"
              @click.stop="toggle"
            />
          </template>
          <template #default="{ close }">
            <slot name="menu" :close="close" />
          </template>
        </MdMenu>
      </div>
    </div>
  </article>
</template>
