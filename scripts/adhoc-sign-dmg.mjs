import { execFile } from 'node:child_process'
import {
  access,
  mkdtemp,
  readFile,
  readdir,
  rename,
  rm,
} from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { promisify } from 'node:util'

const execFileAsync = promisify(execFile)
const { version } = JSON.parse(await readFile('package.json', 'utf8'))
const dmgDirs = [
  'target/release/bundle/dmg',
  'target/fast-release/bundle/dmg',
  'target/aarch64-apple-darwin/release/bundle/dmg',
  'target/aarch64-apple-darwin/fast-release/bundle/dmg',
]

const run = async (command, args) => {
  const { stdout, stderr } = await execFileAsync(command, args, {
    maxBuffer: 10 * 1024 * 1024,
  })
  if (stdout) process.stdout.write(stdout)
  if (stderr) process.stderr.write(stderr)
}

const resolveDmg = async () => {
  const names = [
    `Clash Verge_${version}_aarch64.dmg`,
    `Clash_Verge_${version}_aarch64.dmg`,
  ]

  for (const dir of dmgDirs) {
    try {
      const files = await readdir(dir)
      const name = names.find((candidate) => files.includes(candidate))
      if (name) return path.resolve(dir, name)
    } catch (error) {
      if (error.code !== 'ENOENT') throw error
    }
  }

  throw new Error(`DMG for version ${version} was not found`)
}

const dmgPath = await resolveDmg()
const tempDir = await mkdtemp(path.join(os.tmpdir(), 'clash-verge-sign-'))
const mountDir = path.join(tempDir, 'mount')
const localAppPath = path.join(tempDir, 'Clash Verge.app')
const rwStem = path.join(tempDir, 'bundle-rw')
const signedStem = path.join(tempDir, 'bundle-signed')
const rwPath = `${rwStem}.dmg`
const signedPath = `${signedStem}.dmg`
let attached = false

const serviceExecutables = [
  'clash-verge-service',
  'clash-verge-service-install',
  'clash-verge-service-uninstall',
]

const signServiceExecutables = async (appPath) => {
  const resourcesDir = path.join(appPath, 'Contents', 'Resources', 'resources')

  for (const name of serviceExecutables) {
    const executable = path.join(resourcesDir, name)
    await access(executable)
    await run('codesign', ['--force', '--sign', '-', executable])
    await run('codesign', ['--verify', '--strict', executable])
  }
}

try {
  await run('hdiutil', ['convert', dmgPath, '-format', 'UDRW', '-o', rwStem])
  await run('hdiutil', [
    'attach',
    rwPath,
    '-nobrowse',
    '-mountpoint',
    mountDir,
    '-readwrite',
  ])
  attached = true

  const appPath = path.join(mountDir, 'Clash Verge.app')
  await access(appPath)
  await run('ditto', [appPath, localAppPath])
  await signServiceExecutables(localAppPath)
  await run('codesign', ['--force', '--deep', '--sign', '-', localAppPath])
  await run('codesign', ['--verify', '--deep', '--strict', localAppPath])
  await rm(appPath, { recursive: true, force: true })
  await run('ditto', [localAppPath, appPath])
  await run('codesign', ['--verify', '--deep', '--strict', appPath])

  await run('hdiutil', ['detach', mountDir])
  attached = false
  await run('hdiutil', ['convert', rwPath, '-format', 'UDZO', '-o', signedStem])
  await rename(signedPath, dmgPath)
  console.log(`Ad-hoc signed DMG: ${dmgPath}`)
} finally {
  if (attached) await run('hdiutil', ['detach', mountDir]).catch(() => {})
  await rm(tempDir, { recursive: true, force: true })
}
