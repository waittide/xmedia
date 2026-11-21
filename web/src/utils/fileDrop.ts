export type UploadInputFile = {
  file: File
  relativePath: string
}

export interface ExtractedUploadFiles {
  files: File[]
  fromDirectory: boolean
}

const IGNORED_PATH_SEGMENTS = new Set(['.ds_store', 'thumbs.db', 'desktop.ini', '__macosx'])

const fileRelativePathMap = new WeakMap<File, string>()

function normalizeRelativePath(raw: string): string {
  return raw.replace(/\\/g, '/').replace(/^\/+|\/+$/g, '').replace(/\/{2,}/g, '/')
}

function compareNames(a: string, b: string): number {
  return a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' })
}

function isDirectoryEntry(entry: FileSystemEntry): entry is FileSystemDirectoryEntry {
  return entry.isDirectory
}

function isFileEntry(entry: FileSystemEntry): entry is FileSystemFileEntry {
  return entry.isFile
}

export function isIgnoredSystemFile(name: string): boolean {
  const normalized = normalizeRelativePath(name)
  if (!normalized) return false
  for (const segment of normalized.split('/')) {
    if (!segment) continue
    if (segment.startsWith('.')) return true
    if (IGNORED_PATH_SEGMENTS.has(segment.toLowerCase())) return true
  }
  return false
}

export function setUploadRelativePath(file: File, relativePath: string): void {
  const normalized = normalizeRelativePath(relativePath)
  if (normalized) {
    fileRelativePathMap.set(file, normalized)
  }
}

export function getUploadRelativePath(file: File): string {
  const stored = fileRelativePathMap.get(file)
  if (stored) return stored
  if (file.webkitRelativePath) {
    const normalized = normalizeRelativePath(file.webkitRelativePath)
    if (normalized) return normalized
  }
  return normalizeRelativePath(file.name) || file.name
}

export function toUploadInputFile(file: File): UploadInputFile {
  return {
    file,
    relativePath: getUploadRelativePath(file),
  }
}

async function readAllDirectoryEntries(
  dirEntry: FileSystemDirectoryEntry,
): Promise<FileSystemEntry[]> {
  const reader = dirEntry.createReader()
  const entries: FileSystemEntry[] = []
  while (true) {
    const batch = await new Promise<FileSystemEntry[]>((resolve, reject) => {
      reader.readEntries(resolve, reject)
    })
    if (batch.length === 0) break
    entries.push(...batch)
  }
  return entries
}

function readFileFromEntry(fileEntry: FileSystemFileEntry): Promise<File> {
  return new Promise<File>((resolve, reject) => {
    fileEntry.file(resolve, reject)
  })
}

export function normalizePickedFiles(
  fileList: FileList | File[],
  isDirectory?: boolean,
): ExtractedUploadFiles {
  const rawFiles = Array.from(fileList)
  const files: File[] = []
  let fromDirectory = Boolean(isDirectory)

  for (const file of rawFiles) {
    const relativePath = getUploadRelativePath(file)
    if (relativePath.includes('/')) {
      fromDirectory = true
    }
    if (isIgnoredSystemFile(relativePath) || isIgnoredSystemFile(file.name)) {
      continue
    }
    setUploadRelativePath(file, relativePath)
    files.push(file)
  }

  if (fromDirectory && files.length > 1) {
    files.sort((a, b) => compareNames(getUploadRelativePath(a), getUploadRelativePath(b)))
  }

  return { files, fromDirectory }
}

export async function extractFilesFromDataTransfer(
  dataTransfer: DataTransfer,
): Promise<ExtractedUploadFiles> {
  // Snapshot synchronously during the drop event before any async work,
  // as browsers clear DataTransfer items/files after the event handler yields.
  const fallbackFiles = Array.from(dataTransfer.files ?? [])
  const items = Array.from(dataTransfer.items ?? [])
  const rootEntries: FileSystemEntry[] = []

  for (const item of items) {
    if (item.kind !== 'file') continue
    const getter = (
      item as DataTransferItem & { webkitGetAsEntry?: () => FileSystemEntry | null }
    ).webkitGetAsEntry
    if (typeof getter !== 'function') continue
    const entry = getter.call(item)
    if (entry) {
      rootEntries.push(entry)
    }
  }

  if (rootEntries.length === 0) {
    return normalizePickedFiles(fallbackFiles, false)
  }

  const files: File[] = []
  let fromDirectory = false

  async function walkEntry(entry: FileSystemEntry, parentPath: string): Promise<void> {
    const entryPath = normalizeRelativePath(parentPath ? `${parentPath}/${entry.name}` : entry.name)

    if (isDirectoryEntry(entry)) {
      fromDirectory = true
      if (isIgnoredSystemFile(entryPath)) return

      let children: FileSystemEntry[]
      try {
        children = await readAllDirectoryEntries(entry)
      } catch {
        return
      }
      children.sort((a, b) => compareNames(a.name, b.name))
      for (const child of children) {
        await walkEntry(child, entryPath)
      }
      return
    }

    if (isFileEntry(entry)) {
      if (isIgnoredSystemFile(entryPath)) return

      let file: File
      try {
        file = await readFileFromEntry(entry)
      } catch {
        return
      }

      const relativePath = normalizeRelativePath(entryPath || file.webkitRelativePath || file.name)
      if (relativePath.includes('/')) {
        fromDirectory = true
      }
      if (isIgnoredSystemFile(relativePath) || isIgnoredSystemFile(file.name)) {
        return
      }
      setUploadRelativePath(file, relativePath)
      files.push(file)
    }
  }

  for (const entry of rootEntries) {
    await walkEntry(entry, '')
  }

  return { files, fromDirectory }
}
