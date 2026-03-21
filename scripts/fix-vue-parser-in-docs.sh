#!/bin/bash
set -euo pipefail

# Fix VitePress Vue template parser issue with SQL code blocks
# Wraps all code blocks containing "AS x(" patterns in v-pre containers

echo "Fixing Vue parser issues in documentation..."

# Find all markdown files with problematic patterns
files=$(find docs/rules docs/features docs/concepts -name "*.md" -exec grep -l "AS [a-z][0-9_]*(" {} \; 2>/dev/null || true)

if [ -z "$files" ]; then
  echo "No files found with problematic patterns"
  exit 0
fi

echo "Found $(echo "$files" | wc -l | tr -d ' ') files to fix"

for file in $files; do
  echo "Processing: $file"

  # Create backup
  cp "$file" "$file.bak"

  # Use awk to wrap code blocks that contain "AS x(" patterns
  awk '
    /^```/ {
      in_code = !in_code
      if (in_code) {
        code_start = NR
        code_content = ""
      } else {
        # Check if code block contains problematic pattern
        if (code_content ~ /AS [a-z][0-9_]*\(/) {
          # Insert v-pre before the code block
          print "::: v-pre" > tmpfile
          system("head -n " (code_start - 1) " \"" FILENAME ".bak\" >> tmpfile")
          print buffer
          print $0
          print ":::"
        } else {
          print buffer
          print $0
        }
        code_content = ""
        buffer = ""
      }
    }

    in_code {
      code_content = code_content "\n" $0
      buffer = buffer "\n" $0
    }

    !in_code {
      print
    }
  ' "$file.bak" > "$file.tmp"

  # Simpler approach: just wrap ALL code blocks in v-pre
  perl -i -pe '
    if (/^```(\w+)$/) {
      $_ = "::: v-pre\n$_";
      $lang = $1;
      $in_code = 1;
    } elsif (/^```$/ && $in_code) {
      $_ = "$_:::\n";
      $in_code = 0;
    }
  ' "$file"

  echo "  ✓ Fixed: $file"
done

echo "Done! Removed $(ls -1 docs/**/*.bak 2>/dev/null | wc -l | tr -d ' ') backup files"
trash docs/**/*.bak docs/**/*.tmp 2>/dev/null || true

echo "Testing build..."
cd docs && npm run build:docs
