export const DEFAULT_DELAY_TEST_URL = 'https://cp.cloudflare.com/generate_204'

const LEGACY_HTTP_DELAY_URLS = new Map([
  [
    'http://cp.cloudflare.com/generate_204',
    'https://cp.cloudflare.com/generate_204',
  ],
  [
    'http://www.gstatic.com/generate_204',
    'https://www.gstatic.com/generate_204',
  ],
])

export function normalizeDelayTestUrl(url?: string | null): string {
  const value = url?.trim()
  if (!value) return DEFAULT_DELAY_TEST_URL
  return LEGACY_HTTP_DELAY_URLS.get(value) || value
}
