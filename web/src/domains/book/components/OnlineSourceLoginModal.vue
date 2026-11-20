<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
import {
  LuExternalLink,
  LuKeyRound,
  LuLoader2,
  LuTerminal,
  LuX,
} from 'vue-icons-plus/lu'
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

const formValues = reactive<Record<string, string>>({})
const runningAction = ref<string | null>(null)
const errorMsg = ref<string | null>(null)
const lastDiagnostics = ref<ExecutionDiagnostics | null>(null)
const lastResultText = ref('')
const showVarEditor = ref(false)
const rawVariableDraft = ref('')
const savingVar = ref(false)

watch(
  () => [props.open, props.source] as const,
  ([isOpen, src]) => {
    if (!isOpen || !src) return
    errorMsg.value = null
    lastDiagnostics.value = null
    lastResultText.value = ''
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
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
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
              <h2 class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                书源登录与扩展设置 · {{ source.name }}
              </h2>
              <p class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                {{
                  source.cookie_hosts.length > 0
                    ? `已存 Cookie 域：${source.cookie_hosts.join(', ')}`
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

        <!-- Body -->
        <div class="flex-1 space-y-4 overflow-y-auto px-5 py-4">
          <div
            v-if="errorMsg"
            class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-3.5 py-2.5 text-xs text-[var(--md-sys-color-on-error-container)]"
          >
            {{ errorMsg }}
          </div>

          <!-- Toast / Diagnostics Output -->
          <div
            v-if="
              lastDiagnostics &&
              (lastDiagnostics.toasts.length > 0 ||
                lastDiagnostics.browser_urls.length > 0 ||
                lastResultText)
            "
            class="space-y-2 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-3.5 text-xs"
          >
            <div class="flex items-center gap-1.5 font-semibold text-[var(--md-sys-color-primary)]">
              <LuTerminal class="h-3.5 w-3.5" />
              <span>书源反馈</span>
            </div>
            <pre
              v-for="(toast, i) in lastDiagnostics.toasts"
              :key="`toast-${i}`"
              class="whitespace-pre-wrap break-words rounded-xl bg-[var(--md-sys-color-surface-container-low)] px-3 py-2 font-mono text-[11px] leading-relaxed text-[var(--md-sys-color-on-surface)]"
            >{{ toast }}</pre>
            <div
              v-if="lastDiagnostics.toasts.length === 0 && lastResultText"
              class="rounded-xl bg-[var(--md-sys-color-surface-container-low)] px-3 py-2 font-mono text-[11px] text-[var(--md-sys-color-on-surface)]"
            >
              返回结果：{{ lastResultText }}
            </div>
            <div
              v-if="lastDiagnostics.browser_urls.length > 0"
              class="flex flex-wrap gap-2 pt-1"
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
          </div>

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
