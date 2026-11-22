// Unified minimalist MD3 domain primitives & page templates
export { default as MediaErrorState } from './MediaErrorState.vue'
export { default as PaginationBar } from './PaginationBar.vue'
export type { PaginationMode } from './PaginationBar.vue'
export { default as SourceScopeBar } from './SourceScopeBar.vue'
export { default as MediaItemCard } from './MediaItemCard.vue'
export type {
  MediaItemCardAspect,
  MediaItemCardLayout,
  MediaItemCoverStyle,
} from './MediaItemCard.vue'

export { default as DomainHomeTemplate } from './DomainHomeTemplate.vue'
export { default as DomainLibraryTemplate } from './DomainLibraryTemplate.vue'
export { default as MediaDetailTemplate } from './MediaDetailTemplate.vue'
export type { MediaDetailSpec, MediaDetailTag } from './MediaDetailTemplate.vue'
export { default as ImmersiveMediaTemplate } from './ImmersiveMediaTemplate.vue'
export type { ImmersiveTapMode } from './ImmersiveMediaTemplate.vue'

// Existing legacy / shared domain shells (preserved for backwards compatibility)
export { default as DomainCollectionsView } from './DomainCollectionsView.vue'
export type {
  CollectionChipItem,
  CollectionGroupItem,
  CollectionLibraryItem,
  CollectionSortKey,
  CollectionViewMode,
} from './DomainCollectionsView.vue'
export { default as DomainBookmarksView } from './DomainBookmarksView.vue'
export { default as HomeStatCards } from './HomeStatCards.vue'
export type { StatCardItem } from './HomeStatCards.vue'
