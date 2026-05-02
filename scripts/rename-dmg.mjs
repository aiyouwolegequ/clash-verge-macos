import { readdir, rename } from 'node:fs/promises'
import path from 'node:path'

/**
 * 构建后将 DMG 文件名中的空格替换为下划线，
 * 使 app 名称保持 "Clash Verge.app" 的同时，
 * DMG 文件名格式为 "Clash_Verge_{version}_{target}.dmg"。
 */

const CANDIDATE_DIRS = [
  'target/release/bundle/dmg',
  'target/fast-release/bundle/dmg',
]

async function main() {
  let renamed = false

  for (const dmgDir of CANDIDATE_DIRS) {
    try {
      const files = await readdir(dmgDir)
      for (const file of files) {
        if (file.startsWith('Clash Verge_') && file.endsWith('.dmg')) {
          const newName = file.replace('Clash Verge_', 'Clash_Verge_')
          const oldPath = path.resolve(dmgDir, file)
          const newPath = path.resolve(dmgDir, newName)
          await rename(oldPath, newPath)
          console.log(`Renamed DMG:\n  ${oldPath}\n  -> ${newPath}`)
          renamed = true
        }
      }
    } catch (err) {
      if (err.code !== 'ENOENT') {
        console.error(`Error reading ${dmgDir}:`, err)
      }
    }
  }

  if (!renamed) {
    console.log("No DMG file starting with 'Clash Verge_' found to rename.")
  }
}

main()
