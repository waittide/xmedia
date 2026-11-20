<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
import {
  LuCheckCircle2,
  LuExternalLink,
  LuKeyRound,
  LuLoader2,
  LuTerminal,
  LuX,
} from 'vue-icons-plus/lu'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  onlineBookApi,
  type BookSourceSummaryDto,
  type ExecutionDiagnostics,
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
const errorMsg = ref<string | null>(null)
const lastActionName = ref<string | null>(null)
const lastDiagnostics = ref<ExecutionDiagnostics | null>(null)
const lastResultText = ref('')
const showLogs = ref(false)
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

    errorMsg.value = null
    lastActionName.value = null
    lastDiagnostics.value = null
    lastResultText.value = ''
    showLogs.value = false
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
  lastActionName.value = item.name
  errorMsg.value = null
  try {
    const resp = await onlineBookApi.runLoginAction(
      props.source.id,
      { ...formValues },
      act,
    )
    lastDiagnostics.value = resp.diagnostics
    lastResultText.value = resp.result
    rawVariableDraft.value = resp.source.variable_preview || ''
    emit('updated', resp.source)

    const latestToast =
      resp.diagnostics.toasts.length > 0
        ? resp.diagnostics.toasts[resp.diagnostics.toasts.length - 1]
        : null
    if (latestToast) {
      const lines = latestToast
        .split('\n')
        .map((l) => l.trim())
        .filter(Boolean)
      if (lines.length === 1 && lines[0]!.length <= 60) {
        snackbar.show({ message: lines[0]! })
      } else {
        snackbar.show({ message: `「${item.name}」执行完成，详情已在上方反馈区完整展示` })
      }
    } else if (resp.result) {
      snackbar.show({ message: `${item.name}：${resp.result.slice(0, 60)}` })
    } else {
      snackbar.show({ message: `已执行「${item.name}」` })
    }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    errorMsg.value = msg
    snackbar.show({ message: msg, tone: 'error' })
  } finally {
    runningAction.value = null
  }
}

async function saveRawVariable(): Promise<void> {
  if (!props.source || savingVar.value) return
  savingVar.value = true
  errorMsg.value = null
  try {
    const updated = await onlineBookApi.updateSource(props.source.id, {
      variable: rawVariableDraft.value,
    })
    emit('updated', updated)
    showVarEditor.value = false
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    savingVar.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open && source"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4 backdrop-blur-[2px]"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[88vh] w-full max-w-2xl flex-col overflow-hidden rounded-[28px] bg-[var(--md-sys-color-surface-container-low)] shadow-[var(--md-sys-elevation-3)]"
      >
        <!-- Header -->
        <div
          class="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--md-sys-color-outline-variant)]/50 px-5 py-4"
        >
          <div class="flex min-w-0 items-center gap-2.5">
            <div
              class="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
            >
              <LuKeyRound class="h-5 w-5" />
            </div>
            <div class="min-w-0">
              <div class="flex flex-wrap items-center gap-2">
                <h2 class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                  书源登录与扩展设置 · {{ source.name }}
                </h2>
                <span
                  v-if="source.cookie_hosts.length > 0"
                  class="inline-flex shrink-0 items-center gap-1 rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-success-container)]"
                >
                  <LuCheckCircle2 class="h-3 w-3" />
                  <span>已存 Cookie ({{ source.cookie_hosts.length }})</span>
                </span>
              </div>
              <p class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                {{
                  source.cookie_hosts.length > 0
                    ? `Cookie 域：${source.cookie_hosts.slice(0, 3).join(', ')}${source.cookie_hosts.length > 3 ? ' 等' : ''}`
                    : '执行书源自带 loginUi / loginUrl 脚本并持久化源变量与 Cookie'
                }}
              </p>
            </div>
          </div>

          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <!-- Pinned Feedback Banner (Always Visible Above Scrollable Form) -->
        <div
          v-if="runningAction || errorMsg || lastDiagnostics"
          class="shrink-0 space-y-2 border-b border-[var(--md-sys-color-outline-variant)]/40 bg-[var(--md-sys-color-surface-container)]/85 px-5 py-3 text-xs"
        >
          <div
            v-if="runningAction"
            class="flex items-center gap-2 text-[var(--md-sys-color-primary)]"
          >
            <LuLoader2 class="h-4 w-4 animate-spin shrink-0" />
            <span>正在执行「{{ runningAction }}」并等待远端接口响应…</span>
          </div>

          <div
            v-else-if="errorMsg"
            class="rounded-xl bg-[var(--md-sys-color-error-container)] px-3 py-2 text-[var(--md-sys-color-on-error-container)]"
          >
            执行「{{ lastActionName }}」失败：{{ errorMsg }}
          </div>

          <template v-else-if="lastDiagnostics">
            <div class="flex items-center justify-between gap-2">
              <div class="flex items-center gap-1.5 font-semibold text-[var(--md-sys-color-primary)]">
                <LuTerminal class="h-3.5 w-3.5" />
                <span>书源反馈 · {{ lastActionName }}</span>
              </div>
              <div class="flex items-center gap-3">
                <button
                  v-if="lastDiagnostics.logs.length > 0"
                  type="button"
                  class="text-[11px] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-primary)]"
                  @click="showLogs = !showLogs"
                >
                  {{ showLogs ? '隐藏调试日志' : `查看调试日志 (${lastDiagnostics.logs.length})` }}
                </button>
                <button
                  type="button"
                  class="text-[11px] text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
                  @click="lastDiagnostics = null"
                >
                  收起
                </button>
              </div>
            </div>

            <div class="max-h-[45vh] space-y-1.5 overflow-y-auto">
              <pre
                v-for="(toast, i) in lastDiagnostics.toasts"
                :key="`toast-${i}`"
                class="whitespace-pre-wrap break-words rounded-xl bg-[var(--md-sys-color-surface-container-low)] px-3.5 py-2.5 font-mono text-[11px] leading-relaxed text-[var(--md-sys-color-on-surface)] [tab-size:2]"
              >{{ toast }}</pre>
              <div
                v-if="lastDiagnostics.toasts.length === 0"
                class="rounded-xl bg-[var(--md-sys-color-surface-container-low)] px-3 py-2 font-mono text-[11px] text-[var(--md-sys-color-on-surface)]"
              >
                {{ lastResultText ? `返回结果：${lastResultText}` : `已执行「${lastActionName}」（脚本未输出提示文本）` }}
              </div>
              <pre
                v-if="showLogs && lastDiagnostics.logs.length > 0"
                class="whitespace-pre-wrap break-words rounded-xl bg-[var(--md-sys-color-surface-container-highest)]/70 px-3 py-2 font-mono text-[10px] leading-relaxed text-[var(--md-sys-color-on-surface-variant)]"
              >{{ lastDiagnostics.logs.join('\n') }}</pre>
            </div>

            <div
              v-if="lastDiagnostics.browser_urls.length > 0"
              class="flex flex-wrap gap-2 pt-0.5"
            >
              <a
                v-for="(link, idx) in lastDiagnostics.browser_urls"
                :key="`url-${idx}`"
                :href="link.url"
                target="_blank"
                rel="noopener noreferrer"
                class="md-btn md-btn-sm md-btn-tonal md-state"
              >
                <LuExternalLink class="h-3.5 w-3.5" />
                <span>{{ link.title || link.url }}</span>
              </a>
            </div>
          </template>
        </div>

        <!-- Body -->
        <div class="flex-1 space-y-4 overflow-y-auto px-5 py-4">

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

        <!-- Footer -->
        <div
          class="flex shrink-0 items-center justify-end gap-2 border-t border-[var(--md-sys-color-outline-variant)]/40 px-5 py-3"
        >
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="emit('close')"
          >
            完成
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
