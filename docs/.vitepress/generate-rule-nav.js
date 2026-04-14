#!/usr/bin/env node

/**
 * Generate navigation for rule documentation
 * Creates sidebar entries for all rule .md files
 */

import fs from 'fs'
import path from 'path'
import { fileURLToPath } from 'url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

const docsDir = path.join(__dirname, '..')
const rulesDir = path.join(docsDir, 'rules')

function scanRuleDirectory(dir, prefix = '') {
  const items = []

  if (!fs.existsSync(dir)) {
    return items
  }

  const entries = fs.readdirSync(dir, { withFileTypes: true })

  // First, add .md files
  for (const entry of entries) {
    if (entry.isFile() && entry.name.endsWith('.md') && entry.name !== 'README.md') {
      const name = entry.name.replace('.md', '')
      const title = name.split('-').map(word =>
        word.charAt(0).toUpperCase() + word.slice(1)
      ).join(' ')

      items.push({
        text: title,
        link: `${prefix}/${name}`
      })
    }
  }

  // Then, add subdirectories
  for (const entry of entries) {
    if (entry.isDirectory()) {
      const subItems = scanRuleDirectory(
        path.join(dir, entry.name),
        `${prefix}/${entry.name}`
      )

      if (subItems.length > 0) {
        const title = entry.name.split('-').map(word =>
          word.charAt(0).toUpperCase() + word.slice(1)
        ).join(' ')

        items.push({
          text: title,
          collapsed: true,
          items: subItems
        })
      }
    }
  }

  return items
}

function generateRuleNavigation() {
  console.log('Generating rule navigation...')

  const navigation = {
    text: 'Rules Reference',
    collapsed: true,
    items: [
      { text: 'Overview', link: '/rules/' },
      { text: 'Rule Index', link: '/rules/rule-index' },
      { text: 'By Category', link: '/rules/by-category' },
      { text: 'By Database', link: '/rules/by-database' },
      ...scanRuleDirectory(rulesDir, '/rules')
    ]
  }

  const outputPath = path.join(__dirname, 'rule-nav.json')
  fs.writeFileSync(outputPath, JSON.stringify(navigation, null, 2))

  console.log(`Generated navigation with ${navigation.items.length} items`)
  console.log(`Saved to: ${outputPath}`)

  return navigation
}

// Run if called directly
if (import.meta.url === `file://${process.argv[1]}`) {
  generateRuleNavigation()
}

export { generateRuleNavigation }