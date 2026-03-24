import { createRequire } from 'module'
import { readFileSync } from 'fs'
import { resolve, dirname } from 'path'
import { fileURLToPath } from 'url'

const __dirname = dirname(fileURLToPath(import.meta.url))

const meta = JSON.parse(
  readFileSync(resolve(__dirname, 'meta.json'), 'utf-8')
)

const { host, owner, name, release_tag, branch } = meta.repo

const repoBase = `https://${host}/${owner}/${name}`

/**
 * Build a URL to a file at the pinned release tag.
 * Codeberg: /src/tag/v0.1.0/path
 * GitHub:   /blob/v0.1.0/path
 */
function srcUrl(path) {
  if (host.includes('codeberg')) {
    return `${repoBase}/src/tag/${release_tag}/${path}`
  }
  return `${repoBase}/blob/${release_tag}/${path}`
}

/**
 * Build a URL to a file on the default branch.
 * Use for content that changes frequently (RFCs, chores).
 * Codeberg: /src/branch/main/path
 * GitHub:   /blob/main/path
 */
function branchUrl(path) {
  if (host.includes('codeberg')) {
    return `${repoBase}/src/branch/${branch}/${path}`
  }
  return `${repoBase}/blob/${branch}/${path}`
}

/**
 * Build a URL to a directory tree at the pinned release tag.
 * Codeberg: /src/tag/v0.1.0/path
 * GitHub:   /tree/v0.1.0/path
 */
function treeUrl(path) {
  if (host.includes('codeberg')) {
    return `${repoBase}/src/tag/${release_tag}/${path}`
  }
  return `${repoBase}/tree/${release_tag}/${path}`
}

/**
 * Build a raw file URL.
 * Codeberg: /raw/tag/v0.1.0/path
 * GitHub:   /raw/v0.1.0/path (via raw.githubusercontent.com)
 */
function rawUrl(path) {
  if (host.includes('codeberg')) {
    return `${repoBase}/raw/tag/${release_tag}/${path}`
  }
  return `https://raw.githubusercontent.com/${owner}/${name}/${release_tag}/${path}`
}

export {
  meta,
  repoBase,
  srcUrl,
  branchUrl,
  treeUrl,
  rawUrl,
  release_tag,
  branch,
  host,
  owner,
  name
}
