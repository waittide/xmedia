<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuLock, LuUnlock } from 'vue-icons-plus/lu'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import {
  isVideoFieldLockedIn,
  toggleVideoFieldLockIn,
  type VideoInspectorForm,
} from './types'

const props = withDefaults(
  defineProps<{
    form: VideoInspectorForm
    hideLocks?: boolean
  }>(),
  {
    hideLocks: false,
  },
)

const { t } = useI18n()

const nfoKindModel = computed<'movie' | 'episode'>({
  get: () => (props.form.isEpisode ? 'episode' : 'movie'),
  set: (v) => {
    props.form.isEpisode = v === 'episode'
  },
})

const nfoKindOptions = computed(() => [
  { value: 'movie' as const, label: t('tool.forms.video.movieNfo') },
  { value: 'episode' as const, label: t('tool.forms.video.episodeNfo') },
])

function isVideoFieldLocked(field: string): boolean {
  return isVideoFieldLockedIn(props.form.lockedFields, field)
}

function toggleVideoFieldLock(field: string): void {
  props.form.lockedFields = toggleVideoFieldLockIn(props.form.lockedFields, field)
}
</script>

<template>
  <div class="space-y-3">
    <MdSegmentedButton v-model="nfoKindModel" :options="nfoKindOptions" size="sm" />

    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.title"
          size="sm"
          :label="form.isEpisode ? t('tool.forms.video.episodeTitle') : t('tool.forms.video.movieTitle')"
        >
          <template v-if="!hideLocks" #trailing>
            <MdIconButton
              size="sm"
              class="-mr-2"
              :selected="isVideoFieldLocked('title')"
              :icon="isVideoFieldLocked('title') ? LuLock : LuUnlock"
              :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.video.movieTitle') })"
              @click="toggleVideoFieldLock('title')"
            />
          </template>
        </MdTextField>
      </div>

      <div v-if="form.isEpisode" class="sm:col-span-2">
        <MdTextField
          v-model="form.showTitle"
          size="sm"
          :label="t('tool.forms.video.showTitle')"
        >
          <template v-if="!hideLocks" #trailing>
            <MdIconButton
              size="sm"
              class="-mr-2"
              :selected="isVideoFieldLocked('show_title')"
              :icon="isVideoFieldLocked('show_title') ? LuLock : LuUnlock"
              :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.video.showTitle') })"
              @click="toggleVideoFieldLock('show_title')"
            />
          </template>
        </MdTextField>
      </div>

      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.originalTitle"
          size="sm"
          :label="t('tool.forms.video.originalTitle')"
        >
          <template v-if="!hideLocks" #trailing>
            <MdIconButton
              size="sm"
              class="-mr-2"
              :selected="isVideoFieldLocked('original_title')"
              :icon="isVideoFieldLocked('original_title') ? LuLock : LuUnlock"
              :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.video.originalTitle') })"
              @click="toggleVideoFieldLock('original_title')"
            />
          </template>
        </MdTextField>
      </div>

      <div v-if="form.isEpisode" class="grid grid-cols-2 gap-2.5 sm:col-span-2">
        <MdTextField
          v-model="form.season"
          size="sm"
          type="number"
          min="0"
          :label="t('tool.forms.video.season')"
        />
        <MdTextField
          v-model="form.episode"
          size="sm"
          type="number"
          min="0"
          :label="t('tool.forms.video.episode')"
        />
      </div>

      <MdTextField
        v-model="form.year"
        size="sm"
        type="number"
        :label="t('tool.forms.video.year')"
      >
        <template v-if="!hideLocks" #trailing>
          <MdIconButton
            size="sm"
            class="-mr-2"
            :selected="isVideoFieldLocked('year')"
            :icon="isVideoFieldLocked('year') ? LuLock : LuUnlock"
            :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.video.year') })"
            @click="toggleVideoFieldLock('year')"
          />
        </template>
      </MdTextField>

      <MdTextField
        v-model="form.rating"
        size="sm"
        type="number"
        step="0.1"
        :label="t('tool.forms.video.rating')"
      >
        <template v-if="!hideLocks" #trailing>
          <MdIconButton
            size="sm"
            class="-mr-2"
            :selected="isVideoFieldLocked('rating')"
            :icon="isVideoFieldLocked('rating') ? LuLock : LuUnlock"
            :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.video.rating') })"
            @click="toggleVideoFieldLock('rating')"
          />
        </template>
      </MdTextField>

      <MdTextField
        v-model="form.tmdbId"
        size="sm"
        :label="t('tool.forms.video.tmdbId')"
      />

      <MdTextField
        v-model="form.bangumiId"
        size="sm"
        :label="t('tool.forms.video.bangumiId')"
      />

      <MdTextField
        v-model="form.directors"
        size="sm"
        :label="t('tool.forms.video.directors')"
      >
        <template v-if="!hideLocks" #trailing>
          <MdIconButton
            size="sm"
            class="-mr-2"
            :selected="isVideoFieldLocked('directors')"
            :icon="isVideoFieldLocked('directors') ? LuLock : LuUnlock"
            :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.video.directors') })"
            @click="toggleVideoFieldLock('directors')"
          />
        </template>
      </MdTextField>

      <MdTextField
        v-model="form.studio"
        size="sm"
        :label="t('tool.forms.video.studio')"
      >
        <template v-if="!hideLocks" #trailing>
          <MdIconButton
            size="sm"
            class="-mr-2"
            :selected="isVideoFieldLocked('studio')"
            :icon="isVideoFieldLocked('studio') ? LuLock : LuUnlock"
            :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.video.studio') })"
            @click="toggleVideoFieldLock('studio')"
          />
        </template>
      </MdTextField>

      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.actors"
          size="sm"
          :label="t('tool.forms.video.actors')"
        >
          <template v-if="!hideLocks" #trailing>
            <MdIconButton
              size="sm"
              class="-mr-2"
              :selected="isVideoFieldLocked('actors')"
              :icon="isVideoFieldLocked('actors') ? LuLock : LuUnlock"
              :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.video.actors') })"
              @click="toggleVideoFieldLock('actors')"
            />
          </template>
        </MdTextField>
      </div>

      <MdTextField
        v-model="form.genres"
        size="sm"
        :label="t('tool.forms.video.genres')"
      >
        <template v-if="!hideLocks" #trailing>
          <MdIconButton
            size="sm"
            class="-mr-2"
            :selected="isVideoFieldLocked('genres')"
            :icon="isVideoFieldLocked('genres') ? LuLock : LuUnlock"
            :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.video.genres') })"
            @click="toggleVideoFieldLock('genres')"
          />
        </template>
      </MdTextField>

      <MdTextField
        v-model="form.tags"
        size="sm"
        :label="t('tool.forms.video.tags')"
      >
        <template v-if="!hideLocks" #trailing>
          <MdIconButton
            size="sm"
            class="-mr-2"
            :selected="isVideoFieldLocked('tags')"
            :icon="isVideoFieldLocked('tags') ? LuLock : LuUnlock"
            :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.video.tags') })"
            @click="toggleVideoFieldLock('tags')"
          />
        </template>
      </MdTextField>

      <div class="sm:col-span-2">
        <MdTextField
          v-model="form.summary"
          multiline
          :rows="5"
          :label="t('tool.forms.video.plot')"
        >
          <template v-if="!hideLocks" #trailing>
            <MdIconButton
              size="sm"
              class="-mr-2"
              :selected="isVideoFieldLocked('summary')"
              :icon="isVideoFieldLocked('summary') ? LuLock : LuUnlock"
              :aria-label="t('tool.forms.common.toggleLock', { field: t('tool.forms.video.plot') })"
              @click="toggleVideoFieldLock('summary')"
            />
          </template>
        </MdTextField>
      </div>
    </div>
  </div>
</template>
