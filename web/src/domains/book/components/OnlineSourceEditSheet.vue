<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { LuCheckCircle2, LuCode2, LuLoader2, LuSave, LuXCircle } from 'vue-icons-plus/lu'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import { onlineBookApi, type BookSourceSummaryDto } from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: BookSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  saved: [source: BookSourceSummaryDto]
}>()

const snackbar = useSnackbarStore()

const loading = ref(false)
const saving = ref(false)
const loadError = ref<string | null>(null)

const groupName = ref('')
const weight = ref<number>(0)
const variableText = ref('')
const rawJsonText = ref('')
const initialRawJson = ref('')

const jsonValidation = computed<{ valid: boolean; error: string | null }>(() => {
  const txt = rawJsonText.value.trim()
  if (!txt) return { valid: false, error: '书源 JSON 不能为空' }
  try {
    const parsed = JSON.parse(txt)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return { valid: false, error: '书源 JSON 必须是单个对象 {...}' }
    }
    return { valid: true, error: null }
  } catch (err) {
    return { valid: false, error: err instanceof Error ? err.message : 'JSON 语法错误' }
  }
})

async function loadSourceRaw(src: BookSourceSummaryDto): Promise<void> {
  loading.value = true
  loadError.value = null
  groupName.value = src.group_name
  weight.value = src.weight
  variableText.value = src.variable_preview ?? ''
  rawJsonText.value = ''
  initialRawJson.value = ''
  try {
    const obj = await onlineBookApi.fetchSourceJson(src.id)
    const pretty = JSON.stringify(obj, null, 2)
    rawJsonText.value = pretty
    initialRawJson.value = pretty
  } catch (err) {
    loadError.value = err instanceof Error ? err.message : String(err)
  } finally {
    loading.value = false
  }
}

watch(
  () => [props.open, props.source?.id] as const,
  ([isOpen]) => {
    if (isOpen && props.source) {
      void loadSourceRaw(props.source)
    }
  },
  { immediate: true },
)

function formatJson(): void {
  try {
    const parsed = JSON.parse(rawJsonText.value)
    rawJsonText.value = JSON.stringify(parsed, null, 2)
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? `格式化失败：${err.message}` : 'JSON 语法无效',
      tone: 'error',
    })
  }
}

async function handleSave(): Promise<void> {
  if (!props.source || saving.value || !jsonValidation.value.valid) return
  saving.value = true
  try {
    const patch: {
      group_name?: string
      weight?: number
      variable?: string
      raw_json?: string
    } = {
      group_name: groupName.value.trim(),
      weight: Number.isFinite(weight.value) ? Math.trunc(weight.value) : 0,
    }
    if (rawJsonText.value !== initialRawJson.value) {
      patch.raw_json = rawJsonText.value
    }
    if (variableText.value !== (props.source.variable_preview ?? '')) {
      patch.variable = variableText.value
    }
    const updated = await onlineBookApi.updateSource(props.source.id, patch)
    snackbar.show({ message: `已保存书源「${updated.name}」` })
    emit('saved', updated)
    emit('close')
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <MdSideSheet
    :open="open"
    :title="`编辑书源 · ${source?.name ?? ''}`"
    :subtitle="source?.source_url"
    width-class="w-[min(48rem,100vw)]"
    @close="emit('close')"
  >
    <div v-if="loading" class="flex items-center justify-center gap-2 py-20 text-sm text-[var(--md-sys-color-on-surface-variant)]">
      <LuLoader2 class="h-4 w-4 animate-spin" />
      <span>正在加载书源配置…</span>
    </div>

    <div
      v-else-if="loadError"
      class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs text-[var(--md-sys-color-on-error-container)]"
    >
      {{ loadError }}
    </div>

    <div v-else class="space-y-4">
      <!-- Basic Properties -->
      <div class="grid grid-cols-1 gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4 sm:grid-cols-2">
        <label class="block space-y-1.5">
          <span class="text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">书源分组</span>
          <input
            v-model="groupName"
            type="text"
            placeholder="例如：精品、轻小说（可留空）"
            class="md-input w-full py-2 text-sm"
          />
        </label>

        <label class="block space-y-1.5">
          <span class="text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">排序权重（越大越靠前）</span>
          <input
            v-model.number="weight"
            type="number"
            class="md-input w-full py-2 text-sm"
          />
        </label>

        <label class="block space-y-1.5 sm:col-span-2">
          <span class="text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">书源变量 (source.variable)</span>
          <input
            v-model="variableText"
            type="text"
            placeholder="可选运行时变量字符串"
            class="md-input w-full py-2 font-mono text-xs"
          />
        </label>
      </div>

      <!-- Raw Legado JSON Editor -->
      <div class="space-y-2 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex items-center gap-2">
            <span class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
              Legado 书源 JSON
            </span>
            <span
              v-if="jsonValidation.valid"
              class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-success-container)]"
            >
              <LuCheckCircle2 class="h-3 w-3" />
              <span>JSON 合法</span>
            </span>
            <span
              v-else
              class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-error-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-error-container)]"
            >
              <LuXCircle class="h-3 w-3" />
              <span>语法有误</span>
            </span>
          </div>

          <button
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="formatJson"
          >
            <LuCode2 class="h-3.5 w-3.5" />
            <span>格式化 JSON</span>
          </button>
        </div>

        <p
          v-if="!jsonValidation.valid && jsonValidation.error"
          class="text-[11px] text-[var(--md-sys-color-error)]"
        >
          {{ jsonValidation.error }}
        </p>

        <textarea
          v-model="rawJsonText"
          rows="18"
          spellcheck="false"
          class="md-input w-full py-2.5 font-mono text-xs leading-5"
        />
      </div>
    </div>

    <template #footer>
      <button
        type="button"
        class="md-btn md-btn-text md-state"
        @click="emit('close')"
      >
        取消
      </button>
      <button
        type="button"
        :disabled="loading || saving || !jsonValidation.valid"
        class="md-btn md-btn-filled md-state"
        @click="handleSave"
      >
        <LuLoader2 v-if="saving" class="h-4 w-4 animate-spin" />
        <LuSave v-else class="h-4 w-4" />
        <span>{{ saving ? '保存中…' : '保存修改' }}</span>
      </button>
    </template>
  </MdSideSheet>
</template>
