#!/usr/bin/env node

/**
 * RFC Processing Script
 *
 * This script runs before VitePress build to:
 * 1. Copy all RFCs from rfcs/text/ to docs/rfcs/
 * 2. Add automatic cross-linking for RFC references
 * 3. Generate RFC index and navigation
 */

import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const PROJECT_ROOT = path.resolve(__dirname, '../../..');
const RFC_SOURCE_DIR = path.join(PROJECT_ROOT, 'rfcs', 'text');
const RFC_DEST_DIR = path.join(PROJECT_ROOT, 'docs', 'rfcs');
const MAINTAINER_DIR = path.join(PROJECT_ROOT, 'docs', 'maintainers');

// RFC metadata extracted from frontmatter
const rfcMetadata = new Map();
const rfcReferences = new Map(); // tracks which RFCs reference which

/**
 * Parse RFC frontmatter to extract metadata
 */
function parseFrontmatter(content, filename) {
  const match = content.match(/^# RFC (\d+):\s*(.+?)$/m);
  if (!match) {
    console.warn(`No RFC header found in ${filename}`);
    return null;
  }

  const rfcNumber = match[1].padStart(4, '0');
  const title = match[2].trim();

  // Extract status from frontmatter
  const statusMatch = content.match(/^-\s*Status:\s*(.+?)$/m);
  const status = statusMatch ? statusMatch[1].trim() : 'Unknown';

  // Extract author
  const authorMatch = content.match(/^-\s*Author:\s*(.+?)$/m);
  const author = authorMatch ? authorMatch[1].trim() : 'Unknown';

  // Extract start date
  const dateMatch = content.match(/^-\s*Start Date:\s*(.+?)$/m);
  const startDate = dateMatch ? dateMatch[1].trim() : 'Unknown';

  // Extract tracking issue
  const issueMatch = content.match(/^-\s*Tracking Issue:\s*(.+?)$/m);
  const trackingIssue = issueMatch ? issueMatch[1].trim() : 'TBD';

  return {
    number: rfcNumber,
    title,
    status,
    author,
    startDate,
    trackingIssue,
    filename: path.basename(filename, '.md'),
  };
}

/**
 * Find all RFC references in markdown content
 */
function findRFCReferences(content) {
  // Match patterns like "RFC 0080", "RFC 0062", etc.
  const pattern = /(?:^|\s)RFC\s+(\d+)(?:\s|$|[.,;:])/g;
  const references = new Set();
  let match;

  while ((match = pattern.exec(content)) !== null) {
    references.add(match[1].padStart(4, '0'));
  }

  return Array.from(references);
}

/**
 * Add cross-linking to RFC content
 */
function addCrossLinks(content, currentRFC) {
  const references = findRFCReferences(content);

  // Track references for backlink generation
  if (currentRFC) {
    references.forEach(ref => {
      if (!rfcReferences.has(ref)) {
        rfcReferences.set(ref, []);
      }
      rfcReferences.get(ref).push(currentRFC);
    });
  }

  // Replace RFC references with markdown links
  // Don't replace if already in a markdown link or heading
  let processed = content.replace(
    /(?<!#\s*)(?<!\[)(?<!\]\()RFC\s+0*(\d+)(?!\])/g,
    (match, num) => {
      const paddedNum = num.padStart(4, '0');
      const metadata = rfcMetadata.get(paddedNum);
      if (metadata) {
        const slug = metadata.filename;
        // Keep the original format (with or without leading zeros)
        const displayNum = match.match(/RFC\s+(0*\d+)/)[1];
        return `[RFC ${displayNum}](/ra/maintainers/rfcs/${slug})`;
      }
      // If RFC doesn't exist in our collection, leave it as-is
      return match;
    }
  );

  return processed;
}

/**
 * Generate "Referenced By" section for an RFC
 */
function generateReferencedBySection(rfcNumber) {
  const referencedBy = rfcReferences.get(rfcNumber) || [];
  if (referencedBy.length === 0) {
    return '';
  }

  const uniqueRefs = [...new Set(referencedBy)];
  const links = uniqueRefs
    .map(num => {
      const metadata = rfcMetadata.get(num);
      if (metadata) {
        return `- [RFC ${parseInt(num)}: ${metadata.title}](/ra/maintainers/rfcs/${metadata.filename})`;
      }
      return null;
    })
    .filter(Boolean);

  if (links.length === 0) {
    return '';
  }

  return `\n## Referenced By\n\nThis RFC is referenced by:\n\n${links.join('\n')}\n`;
}

/**
 * Copy and process a single RFC file
 */
function processRFCFile(filename) {
  const sourcePath = path.join(RFC_SOURCE_DIR, filename);
  const content = fs.readFileSync(sourcePath, 'utf-8');

  const metadata = parseFrontmatter(content, filename);
  if (!metadata) {
    return;
  }

  rfcMetadata.set(metadata.number, metadata);

  // Find references in this RFC
  const references = findRFCReferences(content);
  references.forEach(ref => {
    if (!rfcReferences.has(ref)) {
      rfcReferences.set(ref, []);
    }
    rfcReferences.get(ref).push(metadata.number);
  });
}

/**
 * Write processed RFC file with cross-links
 */
function writeProcessedRFC(filename) {
  const sourcePath = path.join(RFC_SOURCE_DIR, filename);
  let content = fs.readFileSync(sourcePath, 'utf-8');

  const metadata = parseFrontmatter(content, filename);
  if (!metadata) {
    return;
  }

  // Add cross-links
  content = addCrossLinks(content, metadata.number);

  // Add "Referenced By" section at the end
  const referencedBy = generateReferencedBySection(metadata.number);
  if (referencedBy) {
    content += '\n' + referencedBy;
  }

  const destPath = path.join(RFC_DEST_DIR, filename);
  fs.writeFileSync(destPath, content, 'utf-8');
  console.log(`Processed: ${filename}`);
}

/**
 * Generate RFC index markdown
 */
function generateRFCIndex() {
  const rfcs = Array.from(rfcMetadata.values())
    .sort((a, b) => a.number.localeCompare(b.number));

  // Categorize RFCs
  const categories = {
    'Core Optimizer': [],
    'Database-Specific': [],
    'Performance & Resources': [],
    'Query Features': [],
    'Platform & Integration': [],
  };

  rfcs.forEach(rfc => {
    const title = rfc.title.toLowerCase();
    if (
      title.includes('postgresql') ||
      title.includes('documentdb') ||
      title.includes('citus') ||
      title.includes('mongodb') ||
      title.includes('oracle')
    ) {
      categories['Database-Specific'].push(rfc);
    } else if (
      title.includes('memory') ||
      title.includes('resource') ||
      title.includes('parallelism') ||
      title.includes('numa') ||
      title.includes('buffer')
    ) {
      categories['Performance & Resources'].push(rfc);
    } else if (
      title.includes('spatial') ||
      title.includes('vector') ||
      title.includes('time series') ||
      title.includes('full-text') ||
      title.includes('xpath')
    ) {
      categories['Query Features'].push(rfc);
    } else if (
      title.includes('platform') ||
      title.includes('extension') ||
      title.includes('dialect')
    ) {
      categories['Platform & Integration'].push(rfc);
    } else {
      categories['Core Optimizer'].push(rfc);
    }
  });

  const statusBadges = {
    'Proposed': '📋',
    'Draft': '📝',
    'Active': '🔄',
    'Complete': '✓',
    'Implemented': '✓',
    'Deprecated': '⚠️',
  };

  let markdown = `# RFC Index for Maintainers

This page provides a comprehensive index of all RFCs (Request for Comments) documents
in the Ra optimizer project. Each RFC represents a design proposal, feature implementation,
or architectural decision.

## Quick Stats

- **Total RFCs:** ${rfcs.length}
- **Proposed:** ${rfcs.filter(r => r.status === 'Proposed').length}
- **Draft:** ${rfcs.filter(r => r.status === 'Draft').length}
- **Active:** ${rfcs.filter(r => r.status === 'Active').length}
- **Complete:** ${rfcs.filter(r => r.status === 'Complete' || r.status === 'Implemented').length}

`;

  // Generate category sections
  for (const [category, categoryRFCs] of Object.entries(categories)) {
    if (categoryRFCs.length === 0) continue;

    markdown += `\n## ${category} (${categoryRFCs.length} RFCs)\n\n`;

    categoryRFCs.forEach(rfc => {
      const badge = statusBadges[rfc.status] || '❓';
      const num = parseInt(rfc.number);
      markdown += `### [RFC ${num}: ${rfc.title}](/ra/maintainers/rfcs/${rfc.filename}) ${badge}\n\n`;
      markdown += `- **Status:** ${rfc.status}\n`;
      markdown += `- **Author:** ${rfc.author}\n`;
      markdown += `- **Date:** ${rfc.startDate}\n`;

      if (rfc.trackingIssue && rfc.trackingIssue !== 'TBD') {
        markdown += `- **Tracking:** ${rfc.trackingIssue}\n`;
      }

      // Show which RFCs this one references
      const references = rfcReferences.get(rfc.number) || [];
      if (references.length > 0) {
        const refLinks = references
          .map(ref => {
            const refMeta = rfcMetadata.get(ref);
            return refMeta ? `RFC ${parseInt(ref)}` : null;
          })
          .filter(Boolean)
          .join(', ');
        if (refLinks) {
          markdown += `- **References:** ${refLinks}\n`;
        }
      }

      markdown += '\n';
    });
  }

  // Add status summary table
  markdown += `\n## RFC Status Distribution\n\n`;
  markdown += `| Status | Count | Percentage |\n`;
  markdown += `|--------|-------|------------|\n`;

  const statusCounts = {};
  rfcs.forEach(rfc => {
    statusCounts[rfc.status] = (statusCounts[rfc.status] || 0) + 1;
  });

  Object.entries(statusCounts)
    .sort((a, b) => b[1] - a[1])
    .forEach(([status, count]) => {
      const percentage = ((count / rfcs.length) * 100).toFixed(1);
      const badge = statusBadges[status] || '❓';
      markdown += `| ${status} ${badge} | ${count} | ${percentage}% |\n`;
    });

  return markdown;
}

/**
 * Generate RFC overview README
 */
function generateRFCReadme() {
  return `# Request for Comments (RFCs)

This directory contains all RFC documents for the Ra optimizer project.

## What are RFCs?

RFCs (Request for Comments) are design documents that describe new features,
architectural changes, or significant modifications to the Ra optimizer.
Each RFC goes through a review process before being accepted or implemented.

## RFC Process

1. **Proposed**: Initial RFC draft submitted for discussion
2. **Draft**: RFC is being refined based on feedback
3. **Active**: RFC is approved and implementation is in progress
4. **Complete**: RFC is fully implemented and merged
5. **Deprecated**: RFC is no longer relevant or has been superseded

## Finding RFCs

- [Comprehensive RFC Index](/ra/maintainers/rfcs/) - All RFCs organized by category
- Browse this directory for individual RFC documents
- Use the site search to find RFCs by keyword

## Contributing

To propose a new RFC, see the [RFC Process Guide](/ra/maintainers/rfc-process).
`;
}

/**
 * Generate navigation structure for VitePress config
 */
function generateNavigation() {
  const rfcs = Array.from(rfcMetadata.values())
    .sort((a, b) => a.number.localeCompare(b.number));

  const navItems = rfcs.map(rfc => {
    const num = parseInt(rfc.number);
    const statusEmoji = {
      'Proposed': '📋',
      'Draft': '📝',
      'Active': '🔄',
      'Complete': '✓',
      'Implemented': '✓',
    }[rfc.status] || '';

    return {
      text: `RFC ${num}: ${rfc.title} ${statusEmoji}`,
      link: `/maintainers/rfcs/${rfc.filename}`,
    };
  });

  const navConfig = {
    text: 'RFCs',
    collapsed: true,
    items: navItems,
  };

  const configPath = path.join(PROJECT_ROOT, 'docs', '.vitepress', 'rfc-nav.json');
  fs.writeFileSync(configPath, JSON.stringify(navConfig, null, 2));
  console.log(`Generated navigation config: ${configPath}`);
}

/**
 * Main processing function
 */
function main() {
  console.log('Starting RFC processing...');

  // Ensure destination directory exists
  if (!fs.existsSync(RFC_DEST_DIR)) {
    fs.mkdirSync(RFC_DEST_DIR, { recursive: true });
  }

  // Get all RFC files
  const rfcFiles = fs.readdirSync(RFC_SOURCE_DIR)
    .filter(f => f.endsWith('.md'))
    .sort();

  console.log(`Found ${rfcFiles.length} RFC files`);

  // First pass: collect metadata and references
  console.log('\nPass 1: Collecting metadata...');
  rfcFiles.forEach(processRFCFile);

  // Second pass: write processed files with cross-links
  console.log('\nPass 2: Processing and writing RFCs...');
  rfcFiles.forEach(writeProcessedRFC);

  // Generate index and navigation
  console.log('\nGenerating RFC index...');
  const indexContent = generateRFCIndex();
  const indexPath = path.join(MAINTAINER_DIR, 'rfcs', 'index.md');
  fs.mkdirSync(path.dirname(indexPath), { recursive: true });
  fs.writeFileSync(indexPath, indexContent);
  console.log(`Generated: ${indexPath}`);

  // Generate README for rfcs directory
  console.log('\nGenerating RFC README...');
  const readmePath = path.join(RFC_DEST_DIR, 'README.md');
  fs.writeFileSync(readmePath, generateRFCReadme());
  console.log(`Generated: ${readmePath}`);

  // Generate navigation config
  console.log('\nGenerating navigation...');
  generateNavigation();

  console.log('\nRFC processing complete!');
  console.log(`Total RFCs processed: ${rfcMetadata.size}`);
  console.log(`Total cross-references: ${Array.from(rfcReferences.values()).reduce((sum, refs) => sum + refs.length, 0)}`);
}

// Run the script
main();
