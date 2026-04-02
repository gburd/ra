#!/usr/bin/env node
/**
 * Copy .rra rule files from rules/ to public/rules/ before VitePress build
 * This makes rule files accessible via /ra/rules/ URLs in the documentation
 */

import { execSync } from 'child_process'
import { existsSync, mkdirSync } from 'fs'
import { dirname } from 'path'
import { fileURLToPath } from 'url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)

console.log('Copying .rra rule files to public directory...')

const publicRulesDir = `${__dirname}/../../public/rules`
const projectRoot = `${__dirname}/../../..`
const rulesDir = `${projectRoot}/rules`

// Check if rules directory exists
if (!existsSync(rulesDir)) {
  console.log('ℹ Rules directory not found (Docker build context limitation). Skipping rule file copying.')
  console.log('✓ Documentation will build without rule files in public directory.')
  process.exit(0)
}

// Create public/rules directory if it doesn't exist
if (!existsSync(publicRulesDir)) {
  mkdirSync(publicRulesDir, { recursive: true })
}

try {
  // Copy all .rra files from rules/ to public/rules/
  // This preserves the directory structure
  execSync(`find rules -name "*.rra" -type f -exec bash -c 'mkdir -p "docs/public/$(dirname "{}")" && cp "{}" "docs/public/{}"' \\;`, {
    cwd: projectRoot,
    stdio: 'inherit'
  })

  // Count copied files
  const count = execSync(`find "${publicRulesDir}" -name "*.rra" | wc -l`, { encoding: 'utf-8' }).trim()
  console.log(`✓ Copied ${count} .rra files to public/rules/`)
} catch (error) {
  console.error('✗ Error copying .rra files:', error.message)
  process.exit(1)
}
