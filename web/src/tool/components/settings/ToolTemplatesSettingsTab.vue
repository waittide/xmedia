<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuRotateCcw, LuSave } from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../../api'
import { DEFAULT_RENAME_TEMPLATES, DOMAIN_RENAME_TEMPLATE_SPECS } from '../../templates'
import type { PublicSettingsDto, RenameTemplatesDto } from '../../types'

const props = defineProps<{
  settings: PublicSettingsDto | null
  domainSubTab: MediaKind
}>()

const emit = defineEmits<{
  updated: [settings: PublicSettingsDto]
  error: [message: string | null]
  saved: []
}>()

const { t } = useI18n()

const domainLabel = computed(() => t(`tool.domains.${props.domainSubTab}`))
const domainTokenExamples = DOMAIN_RENAME_TEMPLATE_SPECS

const templates = ref<RenameTemplatesDto>({ ...DEFAULT_RENAME_TEMPLATES })
const saving = ref(false)

watch(
  () => props.settings,
  (s) => {
    if (!s) return
    templates.value = { ...s.rename_templates }
  },
  { immediate: true },
)

function resetDomainTemplates(dom: MediaKind): void {
  if (dom === 'book') templates.value.book = DEFAULT_RENAME_TEMPLATES.book
  else if (dom === 'comic') templates.value.comic = DEFAULT_RENAME_TEMPLATES.comic
  else if (dom === 'music') templates.value.music = DEFAULT_RENAME_TEMPLATES.music
  else {
    templates.value.video_movie = DEFAULT_RENAME_TEMPLATES.video_movie
    templates.value.video_episode = DEFAULT_RENAME_TEMPLATES.video_episode
  }
}

async function handleSaveSettings(): Promise<void> {
  saving.value = true
  emit('error', null)
  try {
    const next = await toolApi.updateSettings({
      rename_templates: { ...templates.value },
    })
    emit('updated', next)
    emit('saved')
  } catch (err) {
    emit('error', err instanceof Error ? err.message : String(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <section class="md-card-filled space-y-4 p-6">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h2 class="text-title-small text-on-surface">
          {{ t('tool.settings.templates.title', { domain: domainLabel }) }}
        </h2>
        <p class="text-body-small mt-1 text-on-surface-variant">
          {{
            t('tool.settings.templates.descPrefix', {
              spec: domainTokenExamples[domainSubTab].spec,
            })
          }}
          <code class="font-mono text-primary">:2</code>
          {{ t('tool.settings.templates.zeroPadHint') }}
          <code class="font-mono text-primary">&lt;...&gt;</code>
          {{ t('tool.settings.templates.conditionalHint') }}
        </p>
      </div>

      <MdButton
        variant="outlined"
        size="sm"
        :icon="LuRotateCcw"
        @click="resetDomainTemplates(domainSubTab)"
      >
        {{ t('tool.settings.templates.resetDefault') }}
      </MdButton>
    </div>

    <template v-if="domainSubTab === 'book'">
      <MdTextField
        v-model="templates.book"
        :label="t('tool.settings.templates.bookTemplateLabel')"
      />
    </template>

    <template v-else-if="domainSubTab === 'comic'">
      <MdTextField
        v-model="templates.comic"
        :label="t('tool.settings.templates.comicTemplateLabel')"
      />
    </template>

    <template v-else-if="domainSubTab === 'music'">
      <MdTextField
        v-model="templates.music"
        :label="t('tool.settings.templates.musicTemplateLabel')"
      />
    </template>

    <template v-else>
      <div class="space-y-4">
        <MdTextField
          v-model="templates.video_movie"
          :label="t('tool.settings.templates.movieTemplateLabel')"
        />
        <MdTextField
          v-model="templates.video_episode"
          :label="t('tool.settings.templates.episodeTemplateLabel')"
        />
      </div>
    </template>

    <div class="space-y-2 rounded-corner-lg bg-surface p-4">
      <div class="flex flex-wrap items-center gap-1.5">
        <span class="text-label-medium text-on-surface-variant">
          {{ t('tool.settings.templates.availableTokens') }}
        </span>
        <code
          v-for="tok in domainTokenExamples[domainSubTab].tokens"
          :key="tok"
          class="text-label-small rounded-corner-xs bg-surface-container-high px-2 py-0.5 font-mono text-primary"
        >
          {{ tok }}
        </code>
      </div>
      <p class="text-body-small font-mono text-on-surface-variant">
        {{
          t('tool.settings.templates.sampleOutput', {
            sample: domainTokenExamples[domainSubTab].sample,
          })
        }}
      </p>
    </div>

    <div class="flex justify-end pt-1">
      <MdButton variant="filled" :icon="LuSave" :loading="saving" @click="handleSaveSettings">
        {{ saving ? t('tool.common.saving') : t('tool.settings.templates.saveTemplates') }}
      </MdButton>
    </div>
  </section>
</template>
