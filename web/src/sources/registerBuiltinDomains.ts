import { registerDomainPlugin } from './domainRegistry'
import { bookDomainPlugin } from './plugins/bookPlugin'
import { comicDomainPlugin } from './plugins/comicPlugin'
import { musicDomainPlugin } from './plugins/musicPlugin'
import { videoDomainPlugin } from './plugins/videoPlugin'

let registered = false

export function registerBuiltinDomains(): void {
  if (registered) return
  registered = true
  registerDomainPlugin(bookDomainPlugin)
  registerDomainPlugin(comicDomainPlugin)
  registerDomainPlugin(musicDomainPlugin)
  registerDomainPlugin(videoDomainPlugin)
}

registerBuiltinDomains()
