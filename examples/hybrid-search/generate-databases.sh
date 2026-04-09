#!/usr/bin/env bash
# Generate sample SQLite databases for hybrid search examples
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Generating SQLite sample databases..."

# Generate Wikipedia FTS5 database
echo "Creating wikipedia-fts5.db..."
rm -f wikipedia-fts5.db
sqlite3 wikipedia-fts5.db < create-wikipedia-fts5.sql
echo "✓ wikipedia-fts5.db created ($(du -h wikipedia-fts5.db | cut -f1))"

# Generate Products vector database
echo "Creating products-vec.db..."
rm -f products-vec.db
sqlite3 products-vec.db < create-products-vec.sql
echo "✓ products-vec.db created ($(du -h products-vec.db | cut -f1))"

echo ""
echo "Database generation complete!"
echo ""
echo "Wikipedia FTS5 database:"
sqlite3 wikipedia-fts5.db "SELECT COUNT(*) as articles FROM articles;"
echo ""
echo "Products vector database:"
sqlite3 products-vec.db "SELECT COUNT(*) as products FROM products;"
echo ""
echo "To test FTS5 search:"
echo "  sqlite3 wikipedia-fts5.db \"SELECT title FROM articles WHERE articles MATCH 'database' LIMIT 5;\""
echo ""
echo "To test basic queries:"
echo "  sqlite3 products-vec.db \"SELECT name, price FROM products WHERE category='Electronics' LIMIT 5;\""
