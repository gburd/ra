/**
 * markdown-it plugin that adds a "Try it!" button to SQL code blocks.
 *
 * Detects fenced code blocks with language "sql" and injects a link
 * that opens the Ra Web Explorer with the query pre-loaded.
 *
 * For "bash" blocks, extracts the SQL query from ra-cli commands.
 */

const RA_WEB_BASE = 'http://localhost:8000'

function escapeAttr(str) {
  return str
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

/**
 * Extract a SQL query from a bash code block containing ra-cli
 * commands. Joins continuation lines first, then looks for a
 * quoted SQL string after common ra-cli subcommands.
 * Returns the SQL string or null if none found.
 */
function extractQueryFromBash(code) {
  const joined = code.replace(/\\\n\s*/g, ' ')
  const lines = joined.split('\n')
  for (const line of lines) {
    const trimmed = line.replace(/^#.*$/, '').trim()
    const match = trimmed.match(
      /ra-cli\s+(?:optimize|explain|translate|execute)\b.*?"([^"]+)"/
    )
    if (match) {
      return match[1]
    }
  }
  return null
}

function buildButton(query) {
  const encoded = encodeURIComponent(query)
  const url = `${RA_WEB_BASE}/?query=${encoded}`
  return (
    '<a class="try-it-link" ' +
    `href="${escapeAttr(url)}" ` +
    'target="_blank" ' +
    'rel="noopener noreferrer" ' +
    'title="Open in Ra Web Explorer">' +
    'Try it!</a>'
  )
}

export function tryItPlugin(md) {
  const defaultFence =
    md.renderer.rules.fence ||
    function (tokens, idx, options, env, self) {
      return self.renderToken(tokens, idx, options)
    }

  md.renderer.rules.fence = (tokens, idx, options, env, self) => {
    const token = tokens[idx]
    const lang = (token.info || '').trim().split(/\s+/)[0]
    const code = token.content

    let query = null

    if (lang === 'sql') {
      query = code.trim()
    } else if (lang === 'bash' || lang === 'sh') {
      query = extractQueryFromBash(code)
    }

    const rendered = defaultFence(tokens, idx, options, env, self)

    if (!query) {
      return rendered
    }

    const button = buildButton(query)
    // Insert the button right after the copy button inside the
    // wrapper div. The VitePress structure is:
    //   <div class="language-X ...">
    //     <button class="copy" ...></button>
    //     <span class="lang">X</span>
    //     <pre ...>...</pre>
    //   </div>
    return rendered.replace(
      '<button title="Copy Code" class="copy"></button>',
      '<button title="Copy Code" class="copy"></button>' + button
    )
  }
}
