<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuGlobe, LuLoader2, LuLock } from 'vue-icons-plus/lu'
import OnlineSourceManagerPanel from '@/domains/book/components/OnlineSourceManagerPanel.vue'
import { onlineBookApi, type BookSourceSummaryDto } from '@/domains/book/onlineApi'
import { useAuthStore } from '@/stores/auth'
import type { MediaKind } from '@/types/api'

const props = defineProps<{
  domain: MediaKind
}>()

const emit = defineEmits<{
  error: [message: string]
}>()

const { t } = useI18n()
const authStore = useAuthStore()

const bookSources = ref<BookSourceSummaryDto[]>([])
const loading = ref(false)
const loaded = ref(false)

async function refreshBookSources(): Promise<void> {
  if (props.domain !== 'book' || !authStore.isAdmin) return
  emit('error', '')
  loading.value = true
  try {
    bookSources.value = await onlineBookApi.listSources()
    loaded.value = true
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  if (props.domain === 'book' && authStore.isAdmin) {
    void refreshBookSources()
  }
})

watch(
  () => props.domain,
  (next) => {
    if (next === 'book' && authStore.isAdmin && !loaded.value) {
      void refreshBookSources()
    }
  },
)
</script>

<template>
  <!-- Book Domain: Legado Online Book Sources -->
  <template v-if="domain === 'book'">
    <section v-if="!authStore.isAdmin" class="md-card-outlined px-6 py-14 text-center">
      <div
        class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuLock class="h-5 w-5" />
      </div>
      <p class="md-title-small text-[var(--md-sys-color-on-surface)]">
        {{ t('settings.dataSourcesAdminOnly') }}
      </p>
    </section>

    <div
      v-else-if="loading && !loaded"
      class="flex items-center justify-center py-16 text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
      <span>{{ t('common.loading') }}</span>
    </div>

    <OnlineSourceManagerPanel
      v-else
      :sources="bookSources"
      @refresh="refreshBookSources"
    />
  </template>

  <!-- Future extensible online data sources for Comic / Music / Video -->
  <section v-else class="md-card-outlined px-6 py-14 text-center">
    <div
      class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuGlobe class="h-5 w-5" />
    </div>
    <p class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t('settings.domainDataSourceEmptyTitle') }}
    </p>
    <p class="md-body-small mx-auto mt-1 max-w-sm text-[var(--md-sys-color-on-surface-variant)]">
      {{ t('settings.domainDataSourceEmptyHint') }}
    </p>
  </section>
</template>
