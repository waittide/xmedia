// Unified minimalist MD3 domain primitives & page templates
export { default as MediaErrorState } from './MediaErrorState.vue'
export { default as PaginationBar } from './PaginationBar.vue'
export type { PaginationMode } from './PaginationBar.vue'
export { default as SourceScopeBar } from './SourceScopeBar.vue'
export { default as MediaItemCard } from './MediaItemCard.vue'
export {
  aspectClassFor,
  mediaGridClassFor,
  resolveDomainAspect,
  resolveDomainCoverStyle,
  resolveDomainIcon,
  type MediaItemCardAspect,
  type MediaItemCardLayout,
  type MediaItemCoverStyle,
} from './mediaLayout'

export { default as DomainHomeTemplate } from './DomainHomeTemplate.vue'
export { default as DomainLibraryTemplate } from './DomainLibraryTemplate.vue'
export { default as MediaDetailTemplate } from './MediaDetailTemplate.vue'
export type { MediaDetailSpec, MediaDetailTag } from './MediaDetailTemplate.vue'
export { default as ImmersiveMediaTemplate } from './ImmersiveMediaTemplate.vue'
export type { ImmersiveTapMode } from './ImmersiveMediaTemplate.vue'
