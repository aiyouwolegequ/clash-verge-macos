import { createWriteStream } from 'node:fs'
import { mkdir, unlink } from 'node:fs/promises'
import { resolve } from 'node:path'
import { pipeline } from 'node:stream/promises'

import { extract as tarExtract } from 'tar'

const IPC_VERSION = '2.3.0'
const IPC_RELEASE_URL = `https://github.com/clash-verge-rev/clash-verge-service-ipc/releases/download/v${IPC_VERSION}/clash-verge-service-ipc-v${IPC_VERSION}-aarch64-apple-darwin.tar.gz`

/**
 * Download the genuine clash-verge-service binaries from GitHub releases
 * and place them in the app bundle for TUN service installation.
 */
async function main() {
  const profile = process.env.CARGO_BUILD_PROFILE || 'release'
  const targetTriple = process.env.CARGO_BUILD_TARGET || ''
  const targetDir = process.env.CARGO_TARGET_DIR || 'target'

  const bundleDir = resolve(
    targetDir,
    `${targetTriple ? targetTriple + '/' : ''}${profile}/bundle/macos/Clash Verge.app/Contents/MacOS`,
  )

  const tarPath = resolve('/tmp', 'clash-verge-service-ipc.tar.gz')
  const extractDir = resolve('/tmp', 'service-ipc-extract')

  try {
    console.log('Downloading clash-verge-service-ipc binaries...')
    const response = await fetch(IPC_RELEASE_URL)
    if (!response.ok) {
      console.error(`Failed to download service IPC: ${response.status}`)
      return
    }

    const fileStream = createWriteStream(tarPath)
    await pipeline(response.body, fileStream)

    await mkdir(extractDir, { recursive: true })

    await tarExtract({ file: tarPath, cwd: extractDir })

    // Copy service binaries to the app bundle
    for (const name of [
      'clash-verge-service',
      'clash-verge-service-install',
      'clash-verge-service-uninstall',
    ]) {
      const src = resolve(extractDir, name)
      const dest = resolve(bundleDir, name)
      const { copyFile } = await import('node:fs/promises')
      await copyFile(src, dest)
      console.log(`Copied ${name} -> ${dest}`)
    }

    // Also copy to target dir for next build
    const binaryDir = resolve(targetDir, targetTriple, profile)
    for (const name of [
      'clash-verge-service',
      'clash-verge-service-install',
      'clash-verge-service-uninstall',
    ]) {
      const src = resolve(extractDir, name)
      const dest = resolve(binaryDir, name)
      const { copyFile } = await import('node:fs/promises')
      await copyFile(src, dest)
    }

    console.log('Service IPC binaries installed successfully')
  } catch (err) {
    console.error(`Failed to download service IPC: ${err.message}`)
  } finally {
    try {
      await unlink(tarPath)
    } catch {}
  }
}

main()
