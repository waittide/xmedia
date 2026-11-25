<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCopy } from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import type { MusicInspectorForm } from './types'

const props = defineProps<{
  form: MusicInspectorForm
  sidecarLrcFilename: string
  fileKey: string
}>()

const { t } = useI18n()
const lyricsTab = ref<'embedded' | 'sidecar'>('embedded')

const lyricsOptions = computed(() => [
  {
    value: 'embedded' as const,
    label: t('tool.forms.music.embeddedLyrics', {
      state: props.form.embeddedLyrics.trim()
        ? t('tool.forms.music.hasLyrics')
        : t('tool.forms.music.emptyLyrics'),
    }),
  },
  {
    value: 'sidecar' as const,
    label: t('tool.forms.music.sidecarLrc', {
      state: props.form.sidecarLrc.trim()
        ? t('tool.forms.music.hasLyrics')
        : t('tool.forms.music.emptyLyrics'),
    }),
  },
])

watch(
  () => props.fileKey,
  () => {
    if (!props.form.embeddedLyrics && props.form.sidecarLrc) {
      lyricsTab.value = 'sidecar'
    } else {
      lyricsTab.value = 'embedded'
    }
  },
  { immediate: true },
)

function syncLyricsBothWays(): void {
  if (lyricsTab.value === 'embedded') {
    props.form.sidecarLrc = props.form.embeddedLyrics
  } else {
    props.form.embeddedLyrics = props.form.sidecarLrc
  }
}
</script>

<template>
  <div class="space-y-3">
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.title"
          size="sm"
          :label="t('tool.forms.music.title')"
        />
      </div>
      <MdTextField
        v-model="form.artist"
        size="sm"
        :label="t('tool.forms.music.artist')"
      />
      <MdTextField
        v-model="form.albumArtist"
        size="sm"
        :label="t('tool.forms.music.albumArtist')"
      />
      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.album"
          size="sm"
          :label="t('tool.forms.music.album')"
        />
      </div>
      <div class="grid grid-cols-3 gap-2.5 sm:col-span-2">
        <MdTextField
          v-model="form.track"
          size="sm"
          type="number"
          min="0"
          :label="t('tool.forms.music.track')"
        />
        <MdTextField
          v-model="form.disc"
          size="sm"
          type="number"
          min="0"
          :label="t('tool.forms.music.disc')"
        />
        <MdTextField
          v-model="form.year"
          size="sm"
          type="number"
          min="0"
          :label="t('tool.forms.music.year')"
        />
      </div>
      <MdTextField
        v-model="form.genre"
        size="sm"
        :label="t('tool.forms.music.genre')"
      />
      <MdTextField
        v-model="form.comment"
        size="sm"
        :label="t('tool.forms.music.comment')"
      />
    </div>

    <!-- Dual Lyrics Editor (Tonal Surface, No Nested Harsh Border) -->
    <div class="space-y-2.5 rounded-corner-lg bg-surface p-3">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <MdSegmentedButton v-model="lyricsTab" :options="lyricsOptions" size="sm" />

        <MdButton
          variant="text"
          size="sm"
          :icon="LuCopy"
          :title="t('tool.forms.music.syncHint')"
          @click="syncLyricsBothWays"
        >
          {{
            lyricsTab === 'embedded'
              ? t('tool.forms.music.syncToLrc')
              : t('tool.forms.music.syncToEmbedded')
          }}
        </MdButton>
      </div>

      <MdTextField
        v-if="lyricsTab === 'embedded'"
        v-model="form.embeddedLyrics"
        multiline
        :rows="7"
        :aria-label="t('tool.workbench.local.embeddedLyrics')"
        :placeholder="t('tool.forms.music.embeddedPlaceholder')"
      />
      <MdTextField
        v-else
        v-model="form.sidecarLrc"
        multiline
        :rows="7"
        :aria-label="sidecarLrcFilename"
        :placeholder="t('tool.forms.music.sidecarPlaceholder', { filename: sidecarLrcFilename })"
      />
    </div>
  </div>
</template>
