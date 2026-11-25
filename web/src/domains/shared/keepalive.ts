import { postKeepaliveJson } from '@/api/client'

export function sendKeepalivePutJson(apiPath: string, payload: unknown): void {
  postKeepaliveJson(apiPath, payload, 'PUT')
}
