#!/usr/bin/env bash
set -euo pipefail

# Comprehensive documentation validation script
# Checks: broken links, spelling, style, external links

DOCS_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DOCS_DIR"

echo "📚 Validating documentation..."
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

ERRORS=0
WARNINGS=0

# Function to report errors
error() {
    echo -e "${RED}ERROR:${NC} $1"
    ((ERRORS++))
}

# Function to report warnings
warn() {
    echo -e "${YELLOW}WARNING:${NC} $1"
    ((WARNINGS++))
}

# Function to report success
success() {
    echo -e "${GREEN}✓${NC} $1"
}

##
## 1. BUILD CHECK - Ensures all markdown compiles
##
echo "1️⃣  Building documentation..."
if npm run build:docs > /tmp/docs-build.log 2>&1; then
    success "Documentation builds successfully"
else
    error "Documentation build failed"
    echo "   See /tmp/docs-build.log for details"
    grep -E "(error|Error|ERROR)" /tmp/docs-build.log | head -20 || true
fi
echo ""

##
## 2. INTERNAL LINK VALIDATION
##
echo "2️⃣  Checking internal links..."
# VitePress validates internal links during build when ignoreDeadLinks: false
# Parse build output for dead link errors
if grep -q "Dead link" /tmp/docs-build.log 2>/dev/null; then
    error "Found broken internal links:"
    grep "Dead link" /tmp/docs-build.log | while IFS= read -r line; do
        echo "   $line"
    done
else
    success "No broken internal links"
fi
echo ""

##
## 3. EXTERNAL LINK CHECK (non-blocking)
##
echo "3️⃣  Checking external links (warnings only)..."
# Extract all external links from markdown files
EXTERNAL_LINKS=$(find . -name "*.md" -not -path "*/node_modules/*" \
    -exec grep -oP 'https?://[^\s\)]+' {} \; | sort -u)

if command -v curl &> /dev/null; then
    echo "$EXTERNAL_LINKS" | while IFS= read -r url; do
        if [ -n "$url" ]; then
            # Quick HEAD request with 5s timeout
            if curl -sSf --head --max-time 5 "$url" > /dev/null 2>&1; then
                : # Link OK, no output to reduce noise
            else
                warn "External link may be broken: $url"
                # Find which files contain this link
                grep -l "$url" **/*.md 2>/dev/null | sed 's/^/     Found in: /' || true
            fi
        fi
    done
    success "External link check complete ($(echo "$EXTERNAL_LINKS" | wc -l) links)"
else
    warn "curl not found, skipping external link check"
fi
echo ""

##
## 4. SPELLING CHECK
##
echo "4️⃣  Checking spelling..."
if command -v aspell &> /dev/null; then
    MISSPELLED=$(find . -name "*.md" -not -path "*/node_modules/*" \
        -exec aspell list --lang=en --mode=markdown < {} \; | sort -u)

    if [ -n "$MISSPELLED" ]; then
        WORD_COUNT=$(echo "$MISSPELLED" | wc -l)
        if [ "$WORD_COUNT" -gt 50 ]; then
            warn "Found $WORD_COUNT potentially misspelled words (review .aspell.en.pws)"
            echo "$MISSPELLED" | head -20
            echo "   ... ($(($WORD_COUNT - 20)) more)"
        else
            warn "Found potentially misspelled words:"
            echo "$MISSPELLED" | sed 's/^/   /'
        fi
    else
        success "No spelling issues found"
    fi
elif command -v hunspell &> /dev/null; then
    success "Using hunspell for spell check"
    find . -name "*.md" -not -path "*/node_modules/*" \
        -exec hunspell -l {} \; | sort -u | head -20
else
    warn "No spell checker found (install aspell or hunspell)"
fi
echo ""

##
## 5. STYLE VALIDATION
##
echo "5️⃣  Checking documentation style..."

# Check for common style issues
STYLE_ISSUES=0

# Check for trailing whitespace
if find . -name "*.md" -not -path "*/node_modules/*" -exec grep -l ' $' {} \; 2>/dev/null | head -1 | grep -q .; then
    warn "Files with trailing whitespace:"
    find . -name "*.md" -not -path "*/node_modules/*" -exec grep -l ' $' {} \; | head -5 | sed 's/^/   /'
    ((STYLE_ISSUES++))
fi

# Check for multiple consecutive blank lines
if find . -name "*.md" -not -path "*/node_modules/*" -exec grep -Pzo '\n\n\n+' {} \; 2>/dev/null | grep -q .; then
    warn "Files with multiple consecutive blank lines"
    ((STYLE_ISSUES++))
fi

# Check for inconsistent heading styles (ATX vs Setext)
SETEXT_HEADINGS=$(find . -name "*.md" -not -path "*/node_modules/*" \
    -exec grep -l '^===\|^---' {} \; 2>/dev/null | wc -l)
if [ "$SETEXT_HEADINGS" -gt 0 ]; then
    warn "Found $SETEXT_HEADINGS files using Setext headings (prefer ATX style with #)"
    ((STYLE_ISSUES++))
fi

if [ "$STYLE_ISSUES" -eq 0 ]; then
    success "No style issues found"
fi
echo ""

##
## 6. SUMMARY
##
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
if [ "$ERRORS" -eq 0 ]; then
    echo -e "${GREEN}✓ All checks passed!${NC}"
    if [ "$WARNINGS" -gt 0 ]; then
        echo -e "${YELLOW}⚠ $WARNINGS warnings (non-blocking)${NC}"
    fi
    exit 0
else
    echo -e "${RED}✗ $ERRORS errors found${NC}"
    if [ "$WARNINGS" -gt 0 ]; then
        echo -e "${YELLOW}⚠ $WARNINGS warnings${NC}"
    fi
    echo ""
    echo "Fix errors before committing. Warnings are advisory only."
    exit 1
fi
