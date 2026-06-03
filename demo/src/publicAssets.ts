export function publicAssetUrl(path: string): string {
  return `${import.meta.env.BASE_URL}${path}`.replace(/\/{2,}/g, "/");
}
