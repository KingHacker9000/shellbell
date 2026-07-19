import { readFile } from 'node:fs/promises'

const manifest = JSON.parse(await readFile(new URL('../public/manifest.webmanifest', import.meta.url), 'utf8'))
for (const field of ['name', 'short_name', 'start_url', 'display', 'icons']) {
  if (!manifest[field]) throw new Error(`manifest missing ${field}`)
}
if (manifest.display !== 'standalone') throw new Error('manifest must use standalone display')
if (!manifest.icons.some((icon) => icon.src && icon.sizes && icon.type)) throw new Error('manifest needs a typed icon')
console.log('manifest.webmanifest is valid')
