import { getOctokit, context } from '@actions/github'
import fetch from 'node-fetch'

import { resolveUpdateLog, resolveUpdateLogDefault } from './updatelog.mjs'

const UPDATE_TAG_NAME = 'updater'
const UPDATE_JSON_FILE = 'update.json'
const ALPHA_TAG_NAME = 'updater-alpha'
const ALPHA_UPDATE_JSON_FILE = 'update.json'

async function resolveUpdater() {
  if (process.env.GITHUB_TOKEN === undefined) {
    throw new Error('GITHUB_TOKEN is required')
  }

  const options = { owner: context.repo.owner, repo: context.repo.repo }
  const github = getOctokit(process.env.GITHUB_TOKEN)

  let allTags = []
  let page = 1
  const perPage = 100

  while (true) {
    const { data: pageTags } = await github.rest.repos.listTags({
      ...options,
      per_page: perPage,
      page: page,
    })

    allTags = allTags.concat(pageTags)

    if (pageTags.length < perPage) {
      break
    }

    page++
  }

  const stableTagRegex = /^v\d+\.\d+\.\d+$/
  const preReleaseRegex = /^(alpha|beta|rc|pre)$/i

  const stableTag = allTags.find((t) => stableTagRegex.test(t.name))
  const preReleaseTag = allTags.find((t) => preReleaseRegex.test(t.name))

  console.log('Stable tag:', stableTag ? stableTag.name : 'None found')
  console.log(
    'Pre-release tag:',
    preReleaseTag ? preReleaseTag.name : 'None found',
  )

  if (stableTag) {
    await processRelease(github, options, stableTag, false)
  }

  if (preReleaseTag) {
    await processRelease(github, options, preReleaseTag, true)
  }
}

async function processRelease(github, options, tag, isAlpha) {
  if (!tag) return

  try {
    const { data: release } = await github.rest.repos.getReleaseByTag({
      ...options,
      tag: tag.name,
    })

    const updateData = {
      name: tag.name,
      notes: await resolveUpdateLog(tag.name).catch(() =>
        resolveUpdateLogDefault().catch(() => 'No changelog available'),
      ),
      pub_date: new Date().toISOString(),
      platforms: {
        'darwin-aarch64': { signature: '', url: '' },
      },
    }

    await Promise.allSettled(
      release.assets.map(async (asset) => {
        const { name, browser_download_url } = asset

        // macOS Apple Silicon DMG: Clash.Verge_X.Y.Z_aarch64.dmg
        if (name.endsWith('aarch64.dmg')) {
          updateData.platforms['darwin-aarch64'].url = browser_download_url
        }

        // macOS Apple Silicon DMG signature: Clash.Verge_X.Y.Z_aarch64.dmg.sig
        if (name.endsWith('aarch64.dmg.sig')) {
          const sig = await getSignature(browser_download_url)
          updateData.platforms['darwin-aarch64'].signature = sig
        }
      }),
    )

    console.log(updateData)

    if (!updateData.platforms['darwin-aarch64'].url) {
      console.error('[Error]: No darwin-aarch64 DMG asset found in release')
      return
    }

    const releaseTag = isAlpha ? ALPHA_TAG_NAME : UPDATE_TAG_NAME
    console.log(
      `Processing ${isAlpha ? 'alpha' : 'stable'} release: ${releaseTag}`,
    )

    let updateRelease

    try {
      const response = await github.rest.repos.getReleaseByTag({
        ...options,
        tag: releaseTag,
      })
      updateRelease = response.data
      console.log(
        `Found existing ${releaseTag} release with ID: ${updateRelease.id}`,
      )
    } catch (error) {
      if (error.status === 404) {
        console.log(
          `Release with tag ${releaseTag} not found, creating new release...`,
        )
        const createResponse = await github.rest.repos.createRelease({
          ...options,
          tag_name: releaseTag,
          name: isAlpha
            ? 'Auto-update Alpha Channel'
            : 'Auto-update Stable Channel',
          body: `This release contains the update information for ${isAlpha ? 'alpha' : 'stable'} channel.`,
          prerelease: isAlpha,
        })
        updateRelease = createResponse.data
      } else {
        throw error
      }
    }

    const jsonFile = isAlpha ? ALPHA_UPDATE_JSON_FILE : UPDATE_JSON_FILE

    // Delete existing update.json asset
    for (const asset of updateRelease.assets) {
      if (asset.name === jsonFile) {
        await github.rest.repos.deleteReleaseAsset({
          ...options,
          asset_id: asset.id,
        })
      }
    }

    // Upload new update.json
    await github.rest.repos.uploadReleaseAsset({
      ...options,
      release_id: updateRelease.id,
      name: jsonFile,
      data: JSON.stringify(updateData, null, 2),
    })

    console.log(
      `Successfully uploaded ${isAlpha ? 'alpha' : 'stable'} update.json to ${releaseTag}`,
    )
  } catch (error) {
    if (error.status === 404) {
      console.log(`Release not found for tag: ${tag.name}, skipping...`)
    } else {
      console.error(`Failed to get release for tag: ${tag.name}`, error.message)
    }
  }
}

async function getSignature(url) {
  const response = await fetch(url, {
    method: 'GET',
    headers: { 'Content-Type': 'application/octet-stream' },
  })
  return response.text()
}

resolveUpdater().catch(console.error)
