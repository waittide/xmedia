/// <reference types="vite/client" />

declare module 'pdfjs-dist/build/pdf.worker.min.mjs?url' {
  const src: string
  export default src
}

declare module 'epubjs' {
  export interface NavItem {
    id: string
    href: string
    label: string
    subitems?: NavItem[]
  }

  export interface LocationPoint {
    cfi: string
    percentage: number
    displayed?: {
      page: number
      total: number
    }
  }

  export interface RelocatedLocation {
    start: LocationPoint
    end: LocationPoint
  }

  export interface RenditionThemes {
    default(styles: Record<string, Record<string, string>>): void
    fontSize(size: string): void
    font(family: string): void
  }

  export interface Rendition {
    display(target?: string): Promise<void>
    next(): Promise<void>
    prev(): Promise<void>
    flow(flow: 'paginated' | 'scrolled' | 'scrolled-doc'): void
    on(event: 'relocated', handler: (location: RelocatedLocation) => void): void
    on(event: string, handler: (...args: unknown[]) => void): void
    themes: RenditionThemes
    resize(width?: number | string, height?: number | string): void
    destroy(): void
  }

  export interface Book {
    ready: Promise<void>
    loaded: {
      navigation: Promise<{ toc: NavItem[] }>
    }
    locations: {
      generate(chars?: number): Promise<string[]>
      percentageFromCfi(cfi: string): number
    }
    renderTo(
      element: HTMLElement | string,
      options?: {
        width?: string | number
        height?: string | number
        manager?: 'default' | 'continuous'
        flow?: 'paginated' | 'scrolled' | 'scrolled-doc'
        spread?: 'none' | 'auto' | 'always'
        allowScriptedContent?: boolean
      },
    ): Rendition
    destroy(): void
  }

  export default function ePub(input: ArrayBuffer | string, options?: Record<string, unknown>): Book
}
