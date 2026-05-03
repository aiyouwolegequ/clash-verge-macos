import { copyFile } from 'node:fs/promises'
import { resolve } from 'node:path'

/**
 * Copy the main binary as clash-verge-service-install for TUN service installation.
 * Runs after tauri build, placing the binary into the bundle.
 */
async function main() {
  const profile = process.env.CARGO_BUILD_PROFILE || 'release'
  const targetTriple = process.env.CARGO_BUILD_TARGET || ''
  const targetDir = process.env.CARGO_TARGET_DIR || 'target'

  const binaryDir = resolve(targetDir, targetTriple, profile)
  const src = resolve(binaryDir, 'clash-verge')
  const dest = resolve(binaryDir, 'clash-verge-service-install')

  // Copy to target dir (for next build's bundling)
  try {
    await copyFile(src, dest)
    console.log(`Copied ${src} -> ${dest}`)
  } catch (err) {
    if (err.code === 'ENOENT') {
      console.error('Binary not found, skipping service binary copy.')
      return
    }
    throw err
  }

  // Also copy into app bundle for immediate testing
  const bundleDest = resolve(
    targetDir,
    `${targetTriple ? targetTriple + '/' : ''}${profile}/bundle/macos/Clash Verge.app/Contents/MacOS/clash-verge-service-install`,
  )
  try {
    await copyFile(src, bundleDest)
    console.log(`Copied ${src} -> ${bundleDest}`)
  } catch {
    // Bundle dir may not exist if build was partial
  }
}

main()
