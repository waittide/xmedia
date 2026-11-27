<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuUpload, LuX } from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdSelect, { type SelectOption } from '@/components/ui/MdSelect.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  canManageCustomFonts,
  deleteCustomFont,
  ensureCustomFontsLoaded,
  ensureLocalSystemFontsLoaded,
  localSystemFonts,
  sanitizeFontFamilyName,
  uploadedFonts,
  uploadCustomFontFiles,
  uploadingFonts,
} from '@/theme/customFonts'
import type { CustomFontDto } from '@/types/api'

const props = withDefaults(
  defineProps<{
    modelValue: string | null
    allowInherit?: boolean
    compact?: boolean
    ariaLabel?: string
  }>(),
  {
    allowInherit: false,
    compact: false,
    ariaLabel: undefined,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()

const { t } = useI18n()
const snackbarStore = useSnackbarStore()

const fileInputRef = ref<HTMLInputElement | null>(null)
const deletingFontId = ref<string | null>(null)

const effectiveValue = computed<string>(() => {
  const raw = props.modelValue?.trim() ?? ''
  if (!raw) return 'system-ui'
  return raw
})

const selectOptions = computed<SelectOption[]>(() => {
  const opts: SelectOption[] = []

  if (props.allowInherit) {
    opts.push({
      value: 'inherit',
      label: t('theme.fonts.presetInherit'),
    })
  }

  opts.push(
    { value: 'system-ui', label: t('theme.fonts.presetSans') },
    { value: 'serif', label: t('theme.fonts.presetSerif') },
    { value: 'mono', label: t('theme.fonts.presetMono') },
  )

  for (const font of uploadedFonts.value) {
    opts.push({
      value: `font:${font.id}`,
      label: font.name,
      badge: font.format.toUpperCase(),
    })
  }

  const current = effectiveValue.value
  const isBuiltin =
    current === 'inherit' ||
    current === 'system-ui' ||
    current === 'serif' ||
    current === 'mono'

  if (!isBuiltin) {
    if (current.startsWith('font:')) {
      const existsInUploaded = uploadedFonts.value.some((f) => `font:${f.id}` === current)
      if (!existsInUploaded) {
        opts.push({
          value: current,
          label: t('theme.fonts.customBadge'),
        })
      }
    } else if (!localSystemFonts.value.includes(current)) {
      opts.push({
        value: current,
        label: current,
        badge: t('theme.fonts.localBadge'),
      })
    }
  }

  for (const fam of localSystemFonts.value) {
    opts.push({
      value: fam,
      label: fam,
      badge: t('theme.fonts.localBadge'),
    })
  }

  return opts
})

function handleSelectChange(next: string): void {
  emit('update:modelValue', next)
}

function triggerFileUpload(): void {
  if (uploadingFonts.value) return
  fileInputRef.value?.click()
}

async function onFileInputChange(ev: Event): Promise<void> {
  const input = ev.target
  if (!(input instanceof HTMLInputElement)) return
  const files = input.files ? Array.from(input.files) : []
  input.value = ''
  if (files.length === 0) return

  try {
    const created = await uploadCustomFontFiles(files)
    const first = created[0]
    if (first) {
      emit('update:modelValue', `font:${first.id}`)
      snackbarStore.show(t('theme.fonts.uploadedToast', { name: first.name }))
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    snackbarStore.show({ message, tone: 'error' })
  }
}

function formatCustomOptionLabel(rawQuery: string): string | null {
  const cleaned = sanitizeFontFamilyName(rawQuery)
  if (!cleaned) return null
  return t('theme.fonts.useCustomFontOption', { name: cleaned })
}

function handleDropdownOpen(): void {
  void ensureLocalSystemFontsLoaded({ fromUserGesture: true })
}

async function handleDeleteFont(font: CustomFontDto): Promise<void> {
  if (deletingFontId.value) return
  deletingFontId.value = font.id
  try {
    await deleteCustomFont(font.id)
    snackbarStore.show(t('theme.fonts.deletedToast', { name: font.name }))
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    snackbarStore.show({ message, tone: 'error' })
  } finally {
    deletingFontId.value = null
  }
}

onMounted(() => {
  void ensureCustomFontsLoaded()
  void ensureLocalSystemFontsLoaded()
})
</script>

<template>
  <div data-font-picker class="flex flex-col" :class="compact ? 'gap-2.5' : 'gap-3'">
    <!-- Unified Select (Presets + Uploaded + Local System Fonts) + Upload Action -->
    <div class="flex flex-wrap items-center gap-2">
      <div class="min-w-44 flex-1">
        <MdSelect
          :model-value="effectiveValue"
          :options="selectOptions"
          :size="compact ? 'sm' : 'md'"
          searchable
          :search-placeholder="t('theme.fonts.searchPlaceholder')"
          :custom-option-label="formatCustomOptionLabel"
          :normalize-custom-value="sanitizeFontFamilyName"
          :aria-label="ariaLabel || t('theme.uiFontTitle')"
          @open="handleDropdownOpen"
          @update:model-value="handleSelectChange"
        />
      </div>

      <MdButton
        v-if="canManageCustomFonts"
        variant="tonal"
        :size="compact ? 'sm' : 'md'"
        :icon="LuUpload"
        :loading="uploadingFonts"
        data-font-upload-btn
        @click="triggerFileUpload"
      >
        {{ t('theme.fonts.uploadBtn') }}
      </MdButton>

      <input
        v-if="canManageCustomFonts"
        ref="fileInputRef"
        type="file"
        multiple
        accept=".ttf,.otf,.ttc,.woff,.woff2"
        class="hidden"
        data-font-file-input
        @change="onFileInputChange"
      />
    </div>

    <!-- Uploaded Font Chips (Selectable + Deletable) -->
    <div
      v-if="uploadedFonts.length > 0"
      class="flex flex-wrap items-center gap-1.5"
      role="group"
      :aria-label="t('theme.fonts.uploadedLabel')"
    >
      <span
        v-for="font in uploadedFonts"
        :key="font.id"
        :data-uploaded-font-id="font.id"
        class="inline-flex h-8 max-w-full items-center rounded-corner-sm border transition-colors duration-150 ease-standard select-none"
        :class="
          effectiveValue === `font:${font.id}`
            ? 'border-transparent bg-secondary-container text-on-secondary-container'
            : 'border-outline-variant bg-surface-container text-on-surface hover:border-outline'
        "
      >
        <button
          type="button"
          :aria-pressed="effectiveValue === `font:${font.id}`"
          class="md-state md-touch-target text-label-medium inline-flex h-full min-w-0 cursor-pointer items-center gap-1.5 pl-2.5"
          :class="canManageCustomFonts ? 'rounded-l-corner-sm pr-1.5' : 'rounded-corner-sm pr-2.5'"
          @click="handleSelectChange(`font:${font.id}`)"
        >
          <LuCheck
            v-if="effectiveValue === `font:${font.id}`"
            class="h-3.5 w-3.5 shrink-0"
            aria-hidden="true"
          />
          <span class="max-w-40 truncate">{{ font.name }}</span>
          <span
            class="text-label-small shrink-0 rounded-corner-xs bg-on-surface/8 px-1 py-0.2 text-on-surface-variant"
          >
            {{ font.format.toUpperCase() }}
          </span>
        </button>

        <button
          v-if="canManageCustomFonts"
          type="button"
          :disabled="deletingFontId === font.id"
          :data-delete-font-id="font.id"
          :aria-label="t('theme.fonts.deleteAria', { name: font.name })"
          class="md-state md-touch-target mr-0.5 inline-flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-corner-full text-on-surface-variant hover:text-error disabled:cursor-not-allowed disabled:opacity-38"
          @click.stop="handleDeleteFont(font)"
        >
          <LuX class="h-3.5 w-3.5" aria-hidden="true" />
        </button>
      </span>
    </div>
  </div>
</template>
