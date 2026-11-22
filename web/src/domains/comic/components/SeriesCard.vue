<script setup lang="ts">
import { computed } from 'vue'
import { LuBookOpen } from 'vue-icons-plus/lu'
import { MediaItemCard } from '@/components/domain'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import type { SeriesSummaryDto } from '@/domains/comic/types'
import { seriesToMediaItemVM } from '@/domains/comic/utils/mappers'

const props = withDefaults(
  defineProps<{
    series: SeriesSummaryDto
    subtitleOverride?: string
    showFavorite?: boolean
  }>(),
  {
    subtitleOverride: undefined,
    showFavorite: false,
  },
)

const emit = defineEmits<{
  'toggle-favorite': [series: SeriesSummaryDto]
}>()

const { t } = useI18n()

const statusLabel = computed(() => {
  if (props.series.status === 'completed') return t('series.statusCompleted')
  if (props.series.status === 'hiatus') return t('series.statusHiatus')
  return t('series.statusOngoing')
})

const itemVm = computed(() =>
  seriesToMediaItemVM(props.series, {
    subtitleOverride: props.subtitleOverride,
    chaptersCountLabel: t('home.chaptersCount', { count: props.series.chapter_count }),
    statusLabel: statusLabel.value,
  }),
)
</script>

<template>
  <MediaItemCard
    :item="itemVm"
    aspect="poster"
    cover-style="book"
    :fallback-icon="LuBookOpen"
    :show-favorite="showFavorite || series.is_favorite"
    :favorite="showFavorite || series.is_favorite ? series.is_favorite : undefined"
    @toggle-favorite="emit('toggle-favorite', series)"
  />
</template>
