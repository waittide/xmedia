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
  isSafeHttpUrl,
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
const runningActionKey = ref<string | null>(null)
const showVarEditor = ref(false)
const rawVariableDraft = ref('')
const loadingVar = ref(false)
const varLoaded = ref(false)
const varLoadError = ref<string | null>(null)
const savingVar = ref(false)

let actionSeq = 0
let varLoadSeq = 0

function resetEditorState(): void {
  actionSeq += 1
  varLoadSeq += 1
  runningActionKey.value = null
  showVarEditor.value = false
  rawVariableDraft.value = ''
  loadingVar.value = false
  varLoaded.value = false
  varLoadError.value = null
  savingVar.value = false
}

async function loadFullVariable(sourceId: string): Promise<void> {
  const seq = ++varLoadSeq
  loadingVar.value = true
  varLoadError.value = null
  try {
    const res = await onlineBookApi.getSourceVariable(sourceId)
    if (seq !== varLoadSeq || !props.open || props.source?.id !== sourceId) return
    rawVariableDraft.value = res.variable ?? ''
    varLoaded.value = true
  } catch (err) {
    if (seq !== varLoadSeq || !props.open || props.source?.id !== sourceId) return
    varLoadError.value = err instanceof Error ? err.message : String(err)
    varLoaded.value = false
  } finally {
    if (seq === varLoadSeq) {
      loadingVar.value = false
    }
  }
}

watch(
  () => [props.open, props.source?.id] as const,
  ([isOpen, srcId], prev) => {
    const src = props.source
    if (!isOpen || !src || !srcId) {
      resetEditorState()
      return
    }
    const wasOpen = prev?.[0] ?? false
    const prevId = prev?.[1]
    if (wasOpen && prevId === srcId) return

    resetEditorState()

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

function fieldInputId(idx: number): string {
  const sid = props.source?.id ?? 'src'
  return `online-source-login-${sid}-${idx}`
}

function actionKeyFor(item: LoginUiItem, idx: number): string {
  return `${idx}:${item.name}`
}

function toggleVarEditor(): void {
  showVarEditor.value = !showVarEditor.value
  if (showVarEditor.value && props.source && !varLoaded.value && !loadingVar.value) {
    void loadFullVariable(props.source.id)
  }
}

function buttonSpanClass(item: LoginUiItem): string {
  const basis = item.style?.layout_flex_basis_percent ?? 0.5
  if (basis >= 0.85) return 'col-span-4'
  if (basis >= 0.4) return 'col-span-2'
  return 'col-span-1'
}

function isDividerButton(item: LoginUiItem): boolean {
  return item.type === 'button' && (!item.action || !item.action.trim())
}

function openExternalUrlSafely(url: string): void {
  if (!isSafeHttpUrl(url)) {
    snackbar.show({ message: '已拦截不安全的外部链接协议（仅允许 http/https）', tone: 'error' })
    return
  }
  window.open(url.trim(), '_blank', 'noopener,noreferrer')
}

async function triggerAction(item: LoginUiItem, idx: number): Promise<void> {
  const src = props.source
  if (!src || runningActionKey.value !== null) return
  const act = item.action?.trim()
  if (!act) return
  const sourceId = src.id
  const seq = ++actionSeq
  runningActionKey.value = actionKeyFor(item, idx)
  try {
    const resp = await onlineBookApi.runLoginAction(
      sourceId,
      { ...formValues },
      act,
    )
    if (seq !== actionSeq || !props.open || props.source?.id !== sourceId) return

    varLoaded.value = false
    if (showVarEditor.value) {
      void loadFullVariable(sourceId)
    }
    emit('updated', resp.source)

    const toasts = resp.diagnostics.toasts
      .map((t) => t.trim())
      .filter(Boolean)
    const rawLinks = resp.diagnostics.browser_urls
    const safeLink = rawLinks.find((link) => isSafeHttpUrl(link.url))
    const hasBlockedLink = rawLinks.length > 0 && !safeLink

    if (toasts.length > 0) {
      const finalMsg = toasts[toasts.length - 1]!
      snackbar.show({
        message: finalMsg,
        actionLabel: safeLink ? safeLink.title || '打开链接' : undefined,
        onAction: safeLink
          ? () => {
              openExternalUrlSafely(safeLink.url)
            }
          : undefined,
      })
    } else if (safeLink) {
      snackbar.show({
        message: `${item.name}：点击打开外部页面`,
        actionLabel: safeLink.title || '打开链接',
        onAction: () => {
          openExternalUrlSafely(safeLink.url)
        },
      })
    } else if (hasBlockedLink) {
      snackbar.show({
        message: `${item.name}：已拦截非 http/https 协议的跳转链接`,
        tone: 'error',
      })
    } else if (resp.result) {
      snackbar.show({ message: `${item.name}：${resp.result}` })
    } else {
      snackbar.show({ message: `已执行「${item.name}」` })
    }
  } catch (err) {
    if (seq !== actionSeq || !props.open || props.source?.id !== sourceId) return
    const msg = err instanceof Error ? err.message : String(err)
    snackbar.show({ message: msg, tone: 'error' })
  } finally {
    if (seq === actionSeq) {
      runningActionKey.value = null
    }
  }
}

async function saveRawVariable(): Promise<void> {
  const src = props.source
  if (!src || savingVar.value || loadingVar.value || !varLoaded.value || varLoadError.value) return
  const sourceId = src.id
  const seq = varLoadSeq
  savingVar.value = true
  try {
    const updated = await onlineBookApi.updateSource(sourceId, {
      variable: rawVariableDraft.value,
    })
    if (seq !== varLoadSeq || !props.open || props.source?.id !== sourceId) return
    emit('updated', updated)
    showVarEditor.value = false
    snackbar.show({ message: '已保存源变量' })
  } catch (err) {
    if (seq !== varLoadSeq || !props.open || props.source?.id !== sourceId) return
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    if (seq === varLoadSeq) {
      savingVar.value = false
    }
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
            <label
              :for="fieldInputId(idx)"
              class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ item.name }}
            </label>
            <input
              :id="fieldInputId(idx)"
              v-model="formValues[item.name]"
              :type="item.type === 'password' ? 'password' : 'text'"
              :autocomplete="item.type === 'password' ? 'off' : undefined"
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
            :disabled="runningActionKey !== null"
            class="md-btn md-btn-sm md-btn-tonal md-state w-full justify-center"
            :class="buttonSpanClass(item)"
            @click="triggerAction(item, idx)"
          >
            <LuLoader2
              v-if="runningActionKey === actionKeyFor(item, idx)"
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
          @click="toggleVarEditor"
        >
          {{ showVarEditor ? '收起源变量 (source.variable)' : '高级：查看 / 手动编辑源变量 (source.variable)' }}
        </button>

        <div v-if="showVarEditor" class="mt-2 space-y-2">
          <div
            v-if="loadingVar"
            class="flex items-center gap-2 rounded-xl bg-[var(--md-sys-color-surface-container)] px-3 py-2 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
          >
            <LuLoader2 class="h-3.5 w-3.5 animate-spin" />
            <span>正在加载完整源变量…</span>
          </div>
          <div
            v-else-if="varLoadError"
            class="flex items-center justify-between gap-2 rounded-xl bg-[var(--md-sys-color-error-container)] px-3 py-2 text-[11px] text-[var(--md-sys-color-on-error-container)]"
          >
            <span class="min-w-0 truncate">加载源变量失败：{{ varLoadError }}</span>
            <button
              type="button"
              class="shrink-0 font-medium underline"
              @click="source && loadFullVariable(source.id)"
            >
              重试
            </button>
          </div>
          <textarea
            v-else
            v-model="rawVariableDraft"
            rows="4"
            class="md-input w-full py-2 font-mono text-[11px]"
            placeholder="JSON 或文本格式的源变量…"
          />
          <div class="flex justify-end">
            <button
              type="button"
              :disabled="savingVar || loadingVar || !varLoaded || Boolean(varLoadError)"
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
