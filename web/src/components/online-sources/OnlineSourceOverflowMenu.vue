<script setup lang="ts" generic="S extends OnlineSourceSummaryDto">
import { useI18n } from 'vue-i18n'
import {
  LuArrowDownToLine,
  LuArrowUpToLine,
  LuDownload,
  LuFlaskConical,
  LuKeyRound,
  LuSearch,
  LuTrash2,
} from 'vue-icons-plus/lu'
import { MdMenu, MdMenuItem } from '@/components/ui'
import type { OnlineSourceSummaryDto } from './types'

defineProps<{
  source: S | null
  pos: { top: number; left: number }
  exportUrl: string
}>()

const emit = defineEmits<{
  close: []
  pin: [source: S, position: 'top' | 'bottom']
  login: [source: S]
  search: [source: S]
  debug: [source: S]
  delete: [source: S]
}>()

const { t } = useI18n()
</script>

<template>
  <MdMenu
    :open="Boolean(source)"
    :anchor-pos="pos"
    :min-width="180"
    :aria-label="source?.name"
    @close="emit('close')"
  >
    <template v-if="source">
      <MdMenuItem
        :icon="LuArrowUpToLine"
        :label="t('onlineSources.menu.pinTop')"
        @select="emit('pin', source, 'top')"
      />

      <MdMenuItem
        :icon="LuArrowDownToLine"
        :label="t('onlineSources.menu.pinBottom')"
        @select="emit('pin', source, 'bottom')"
      />

      <MdMenuItem
        v-if="source.has_login_url || source.login_ui.length > 0"
        :icon="LuKeyRound"
        :label="t('onlineSources.menu.login')"
        @select="emit('login', source)"
      />

      <MdMenuItem
        :icon="LuSearch"
        :label="t('onlineSources.menu.search')"
        @select="emit('search', source)"
      />

      <MdMenuItem
        :icon="LuFlaskConical"
        :label="t('onlineSources.menu.debug')"
        @select="emit('debug', source)"
      />

      <MdMenuItem
        :icon="LuDownload"
        :label="t('onlineSources.menu.export')"
        :href="exportUrl"
        :download="`${source.name}.json`"
      />

      <div class="my-1.5 border-t border-on-surface/8" />

      <MdMenuItem
        :icon="LuTrash2"
        :label="t('onlineSources.menu.delete')"
        destructive
        @select="emit('delete', source)"
      />
    </template>
  </MdMenu>
</template>
