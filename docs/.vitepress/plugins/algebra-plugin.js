/**
 * markdown-it plugin for relational algebra notation.
 *
 * Syntax: {{sigma[p](R)}} or \ra{sigma[p](R)}
 *
 * Converts text-based relational algebra to Unicode symbols
 * wrapped in a <span> with tooltip.
 */

const operators = {
  sigma: { symbol: '\u03C3', name: 'Selection' },
  pi: { symbol: '\u03C0', name: 'Projection' },
  rho: { symbol: '\u03C1', name: 'Rename' },
  gamma: { symbol: '\u03B3', name: 'Aggregation' },
  delta: { symbol: '\u03B4', name: 'Duplicate Elimination' },
  tau: { symbol: '\u03C4', name: 'Sort' },
  join: { symbol: '\u22C8', name: 'Join' },
  semijoin: { symbol: '\u22C9', name: 'Semijoin' },
  antijoin: { symbol: '\u22CA', name: 'Antijoin' },
  leftjoin: { symbol: '\u27D5', name: 'Left Outer Join' },
  rightjoin: { symbol: '\u27D6', name: 'Right Outer Join' },
  fulljoin: { symbol: '\u27D7', name: 'Full Outer Join' },
  union: { symbol: '\u222A', name: 'Union' },
  intersect: { symbol: '\u2229', name: 'Intersection' },
  except: { symbol: '\u2212', name: 'Difference' },
  cross: { symbol: '\u00D7', name: 'Cartesian Product' },
  divide: { symbol: '\u00F7', name: 'Division' },
  natural: { symbol: '\u22C8', name: 'Natural Join' }
}

function escapeHtml(str) {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function convertToUnicode(expr) {
  let result = escapeHtml(expr)

  for (const [keyword, op] of Object.entries(operators)) {
    const prefixRe = new RegExp(
      `\\b${keyword}(?=\\[|\\()`,
      'g'
    )
    result = result.replace(
      prefixRe,
      `<span class="ra-op" title="${op.name}">${op.symbol}</span>`
    )

    const infixRe = new RegExp(
      `\\s+${keyword}(\\[[^\\]]*\\])?\\s+`,
      'g'
    )
    result = result.replace(infixRe, (match, sub) => {
      const subscript = sub
        ? `<sub class="ra-sub">${escapeHtml(sub)}</sub>`
        : ''
      return ` <span class="ra-op" title="${op.name}">${op.symbol}</span>${subscript} `
    })
  }

  return result
}

function algebraBraceRule(state, silent) {
  const src = state.src
  const pos = state.pos
  const max = state.posMax

  if (pos + 3 >= max) return false
  if (src.charCodeAt(pos) !== 0x7B) return false
  if (src.charCodeAt(pos + 1) !== 0x7B) return false

  const closeIdx = src.indexOf('}}', pos + 2)
  if (closeIdx < 0) return false
  if (closeIdx >= max) return false

  if (silent) return true

  const content = src.slice(pos + 2, closeIdx)
  const token = state.push('algebra_inline', '', 0)
  token.content = content
  state.pos = closeIdx + 2
  return true
}

function algebraBackslashRule(state, silent) {
  const src = state.src
  const pos = state.pos
  const max = state.posMax

  if (pos + 4 >= max) return false
  if (src.slice(pos, pos + 4) !== '\\ra{') return false

  const closeIdx = src.indexOf('}', pos + 4)
  if (closeIdx < 0) return false
  if (closeIdx >= max) return false

  if (silent) return true

  const content = src.slice(pos + 4, closeIdx)
  const token = state.push('algebra_inline', '', 0)
  token.content = content
  state.pos = closeIdx + 1
  return true
}

export function algebraPlugin(md) {
  md.inline.ruler.before('escape', 'algebra_brace', algebraBraceRule)
  md.inline.ruler.before('escape', 'algebra_backslash', algebraBackslashRule)

  md.renderer.rules.algebra_inline = (tokens, idx) => {
    const content = tokens[idx].content
    const html = convertToUnicode(content)
    return `<span class="rel-algebra">${html}</span>`
  }
}
