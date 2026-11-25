import { LuCode2, LuInfo, LuListChecks, LuSlidersHorizontal } from 'vue-icons-plus/lu'
import type { RuleFieldDef } from '@/components/online-sources/RuleFieldsSection.vue'
import type { EditTabItem } from '@/components/online-sources/useOnlineSourceEdit'

export type SourceEditFourTab = 'basic' | 'rules' | 'advanced' | 'json'

export interface RuleStageItem {
  id: string
  title: string
  jsonKey: string
  fields: RuleFieldDef[]
  modelValue: Record<string, string>
  onUpdateRules: (next: Record<string, string>) => void
  urlValue?: string
  urlLabel?: string
  urlJsonKey?: string
  urlPlaceholder?: string
  urlRows?: number
  onUpdateUrl?: (next: string) => void
}

export function countStageConfigured(
  ruleMap: Record<string, string>,
  extraUrl?: string,
): number {
  let count = extraUrl?.trim() ? 1 : 0
  for (const val of Object.values(ruleMap)) {
    if (val && val.trim().length > 0) {
      count += 1
    }
  }
  return count
}

export function buildSourceEditTabs(params: {
  basicLabel: string
  rulesLabel: string
  advancedLabel: string
  jsonLabel: string
  showRules?: boolean
  hasRulesConfigured: boolean
  hasAdvancedConfigured: boolean
}): EditTabItem<SourceEditFourTab>[] {
  const items: EditTabItem<SourceEditFourTab>[] = [
    {
      id: 'basic',
      label: params.basicLabel,
      icon: LuInfo,
      activeDot: false,
    },
  ]
  if (params.showRules !== false) {
    items.push({
      id: 'rules',
      label: params.rulesLabel,
      icon: LuListChecks,
      activeDot: params.hasRulesConfigured,
    })
  }
  items.push(
    {
      id: 'advanced',
      label: params.advancedLabel,
      icon: LuSlidersHorizontal,
      activeDot: params.hasAdvancedConfigured,
    },
    {
      id: 'json',
      label: params.jsonLabel,
      icon: LuCode2,
      activeDot: false,
    },
  )
  return items
}
