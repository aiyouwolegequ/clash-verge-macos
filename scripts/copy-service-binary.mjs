import { copyFile } from 'node:fs/promises'
import { resolve } from 'node:path'

/**
 * Copy the main binary as clash-verge-service-install for TUN service installation.
 * Runs before Tauri bundles the app, so the binary is included in the .app package.
 */

const BINARY_NAME = 'clash-verge'
const SERVICE_BINARY_NAME = 'clash-verge-service-install'

async function main() {
  const profile = process.env.CARGO_BUILD_PROFILE || 'release'
  const target = process.env.CARGO_BUILD_TARGET || ''
  const targetDir = process.env.CARGO_TARGET_DIR || 'target'

  const binaryDir = resolve(targetDir, target, profile)
  const src = resolve(binaryDir, BINARY_NAME)
  const dest = resolve(binaryDir, SERVICE_BINARY_NAME)

  try {
    await copyFile(src, dest)
    console.log(`Copied ${src} -> ${dest}`)
  } catch (err) {
    if (err.code === 'ENOENT') {
      console.error(`Binary not found at ${src}, skipping copy.`)
    } else {
      console.error(`Failed to copy service binary: ${err.message}`)
      process.exit(1)
    }
  }
}

main()
