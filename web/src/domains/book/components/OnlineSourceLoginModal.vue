<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
import {
  LuCheckCircle2,
  LuKeyRound,
  LuLoader2,
} from 'vue-icons-plus/lu'
import MdDialog from '@/components/ui/MdDialog.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  onlineBookApi,
  type BookSourceSummaryDto,
  type LoginUiItem,
} from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: BookSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  updated: [source: BookSourceSummaryDto]
}>()

const snackbar = useSnackbarStore()

const formValues = reactive<Record<string, string>>({})
const runningAction = ref<string | null>(null)
const showVarEditor = ref(false)
const rawVariableDraft = ref('')
const savingVar = ref(false)

watch(
  () => [props.open, props.source?.id] as const,
  ([isOpen, srcId], prev) => {
    const src = props.source
    if (!isOpen || !src || !srcId) return
    const wasOpen = prev?.[0] ?? false
    const prevId = prev?.[1]
    if (wasOpen && prevId === srcId) return

    showVarEditor.value = false
    rawVariableDraft.value = src.variable_preview || ''

    for (const k of Object.keys(formValues)) {
      delete formValues[k]
    }
    for (const item of src.login_ui) {
      if (item.type === 'text' || item.type === 'password') {
        formValues[item.name] = src.login_info[item.name] ?? ''
      }
    }
  },
  { immediate: true },
)

function buttonSpanClass(item: LoginUiItem): string {
  const basis = item.style?.layout_flex_basis_percent ?? 0.5
  if (basis >= 0.85) return 'col-span-4'
  if (basis >= 0.4) return 'col-span-2'
  return 'col-span-1'
}

function isDividerButton(item: LoginUiItem): boolean {
  return item.type === 'button' && (!item.action || !item.action.trim())
}

async function triggerAction(item: LoginUiItem): Promise<void> {
  if (!props.source || runningAction.value) return
  const act = item.action?.trim()
  if (!act) return
  runningAction.value = item.name
  try {
    const resp = await onlineBookApi.runLoginAction(
      props.source.id,
      { ...formValues },
      act,
    )
    rawVariableDraft.value = resp.source.variable_preview || ''
    emit('updated', resp.source)

    const toasts = resp.diagnostics.toasts
      .map((t) => t.trim())
      .filter(Boolean)
    const firstLink = resp.diagnostics.browser_urls[0]

    if (toasts.length > 0) {
      const finalMsg = toasts[toasts.length - 1]!
      snackbar.show({
        message: finalMsg,
        actionLabel: firstLink ? firstLink.title || '打开链接' : undefined,
        onAction: firstLink
          ? () => {
              window.open(firstLink.url, '_blank', 'noopener,noreferrer')
            }
          : undefined,
      })
    } else if (firstLink) {
      snackbar.show({
        message: `${item.name}：点击打开外部页面`,
        actionLabel: firstLink.title || '打开链接',
        onAction: () => {
          window.open(firstLink.url, '_blank', 'noopener,noreferrer')
        },
      })
    } else if (resp.result) {
      snackbar.show({ message: `${item.name}：${resp.result}` })
    } else {
      snackbar.show({ message: `已执行「${item.name}」` })
    }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    snackbar.show({ message: msg, tone: 'error' })
  } finally {
    runningAction.value = null
  }
}

async function saveRawVariable(): Promise<void> {
  if (!props.source || savingVar.value) return
  savingVar.value = true
  try {
    const updated = await onlineBookApi.updateSource(props.source.id, {
      variable: rawVariableDraft.value,
    })
    emit('updated', updated)
    showVarEditor.value = false
    snackbar.show({ message: '已保存源变量' })
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    savingVar.value = false
  }
}
</script>

<template>
  <MdDialog
    :open="open && source !== null"
    max-width="max-w-2xl"
    @close="emit('close')"
  >
    <template #header>
      <div v-if="source" class="flex min-w-0 items-start gap-3.5">
        <div
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
        >
          <LuKeyRound class="h-5 w-5" />
        </div>
        <div class="min-w-0">
          <div class="flex flex-wrap items-center gap-2">
            <h3 class="truncate text-base leading-snug font-semibold text-[var(--md-sys-color-on-surface)]">
              书源登录与扩展设置 · {{ source.name }}
            </h3>
            <span
              v-if="source.cookie_hosts.length > 0"
              class="inline-flex shrink-0 items-center gap-1 rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-success-container)]"
            >
              <LuCheckCircle2 class="h-3 w-3" />
              <span>已存 Cookie ({{ source.cookie_hosts.length }})</span>
            </span>
          </div>
          <p class="mt-0.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
            {{
              source.cookie_hosts.length > 0
                ? `Cookie 域：${source.cookie_hosts.slice(0, 3).join(', ')}${source.cookie_hosts.length > 3 ? ' 等' : ''}`
                : '执行书源自带 loginUi / loginUrl 脚本并持久化源变量与 Cookie'
            }}
          </p>
        </div>
      </div>
    </template>

    <div v-if="source" class="space-y-4 py-1">
      <!-- Dynamic Legado loginUi Controls -->
      <div
        v-if="source.login_ui.length > 0"
        class="grid grid-cols-4 gap-2.5"
      >
        <template v-for="(item, idx) in source.login_ui" :key="`ui-${idx}-${item.name}`">
          <!-- Text / Password Input -->
          <div
            v-if="item.type === 'text' || item.type === 'password'"
            class="col-span-4"
          >
            <label class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
              {{ item.name }}
            </label>
            <input
              v-model="formValues[item.name]"
              :type="item.type === 'password' ? 'password' : 'text'"
              class="md-input h-9 w-full text-xs"
              :placeholder="`输入 ${item.name}`"
            />
          </div>

          <!-- Section Divider Label -->
          <div
            v-else-if="isDividerButton(item)"
            class="col-span-4 pt-2 text-center text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ item.name }}
          </div>

          <!-- Action Button -->
          <button
            v-else-if="item.type === 'button'"
            type="button"
            :disabled="runningAction !== null"
            class="md-btn md-btn-sm md-btn-tonal md-state w-full justify-center"
            :class="buttonSpanClass(item)"
            @click="triggerAction(item)"
          >
            <LuLoader2
              v-if="runningAction === item.name"
              class="h-3.5 w-3.5 animate-spin"
            />
            <span class="truncate">{{ item.name }}</span>
          </button>
        </template>
      </div>

      <div
        v-else
        class="rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        该书源未声明 loginUi 面板，可直接修改下方源变量或关闭窗口。
      </div>

      <!-- Advanced Raw Source Variable Editor -->
      <div class="border-t border-[var(--md-sys-color-outline-variant)]/40 pt-3">
        <button
          type="button"
          class="text-[11px] font-medium text-[var(--md-sys-color-primary)] hover:underline"
          @click="showVarEditor = !showVarEditor"
        >
          {{ showVarEditor ? '收起源变量 (source.variable)' : '高级：查看 / 手动编辑源变量 (source.variable)' }}
        </button>

        <div v-if="showVarEditor" class="mt-2 space-y-2">
          <textarea
            v-model="rawVariableDraft"
            rows="4"
            class="md-input w-full py-2 font-mono text-[11px]"
            placeholder="JSON 或文本格式的源变量…"
          />
          <div class="flex justify-end">
            <button
              type="button"
              :disabled="savingVar"
              class="md-btn md-btn-sm md-btn-filled md-state"
              @click="saveRawVariable"
            >
              {{ savingVar ? '保存中…' : '保存源变量' }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex w-full items-center justify-end gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="emit('close')"
        >
          完成
        </button>
      </div>
    </template>
  </MdDialog>
</template>
