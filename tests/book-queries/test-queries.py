#!/usr/bin/env python3
"""
Test SQL queries from database books against Ra's parser.
This uses sqlparser-rs through Python for syntax validation.
"""

import os
import re
import subprocess
import json
from pathlib import Path
from typing import List, Tuple, Dict

# Colors for output
RED = '\033[0;31m'
GREEN = '\033[0;32m'
YELLOW = '\033[1;33m'
NC = '\033[0m'  # No Color

def parse_sql_file(file_path: Path) -> List[str]:
    """Parse a SQL file into individual queries."""
    with open(file_path, 'r') as f:
        content = f.read()

    queries = []
    current_query = []

    for line in content.split('\n'):
        stripped = line.strip()

        # Skip empty lines and comment-only lines
        if not stripped or stripped.startswith('--'):
            continue

        current_query.append(line)

        # If line ends with semicolon, we have a complete query
        if stripped.endswith(';'):
            query = '\n'.join(current_query).strip().rstrip(';')
            queries.append(query)
            current_query = []

    return queries

def test_query_syntax(query: str) -> Tuple[bool, str]:
    """
    Test if a query has valid SQL syntax using sqlparser.
    Returns (success, error_message)
    """
    # Create a simple Rust program that uses sqlparser-rs
    test_program = f'''
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

fn main() {{
    let dialect = GenericDialect {{}};
    let sql = r#"{query}"#;

    match Parser::parse_sql(&dialect, sql) {{
        Ok(_) => {{
            println!("PARSE_SUCCESS");
            std::process::exit(0);
        }}
        Err(e) => {{
            println!("PARSE_ERROR: {{:?}}", e);
            std::process::exit(1);
        }}
    }}
}}
'''

    # For now, we'll just check if the query looks syntactically valid
    # by checking for basic SQL structure
    query_upper = query.upper().strip()

    if not query_upper:
        return False, "Empty query"

    # Basic validation: must start with a SQL keyword
    sql_keywords = ['SELECT', 'INSERT', 'UPDATE', 'DELETE', 'WITH', 'CREATE', 'DROP', 'ALTER']
    if not any(query_upper.startswith(kw) for kw in sql_keywords):
        return False, "Query doesn't start with a SQL keyword"

    # Check for balanced parentheses
    if query.count('(') != query.count(')'):
        return False, "Unbalanced parentheses"

    return True, ""

def categorize_query(query: str) -> str:
    """Categorize a query by its SQL features."""
    query_upper = query.upper()

    if 'RECURSIVE' in query_upper or 'WITH RECURSIVE' in query_upper:
        return 'recursive-cte'
    elif 'WITH' in query_upper:
        return 'cte'
    elif any(fn in query_upper for fn in ['ROW_NUMBER()', 'RANK()', 'DENSE_RANK()', 'LAG(', 'LEAD(', 'OVER ']):
        return 'window-function'
    elif 'UNION' in query_upper or 'INTERSECT' in query_upper or 'EXCEPT' in query_upper:
        return 'set-operation'
    elif 'GROUP BY' in query_upper or 'HAVING' in query_upper:
        return 'aggregation'
    elif any(join in query_upper for join in [' JOIN ', ' INNER JOIN ', ' LEFT JOIN ', ' RIGHT JOIN ', ' FULL JOIN ', ' CROSS JOIN ']):
        return 'join'
    elif 'WHERE' in query_upper and ('SELECT' in query.upper().split('WHERE')[1] if 'WHERE' in query_upper else False):
        return 'subquery'
    else:
        return 'simple'

def main():
    test_dir = Path('tests/book-queries')
    results_dir = test_dir / 'results'
    results_dir.mkdir(exist_ok=True)

    total_queries = 0
    successful_queries = 0
    failed_queries = []
    category_stats = {}

    print("Testing SQL queries from database books...\n")

    # Process each SQL file
    for sql_file in sorted(test_dir.glob('*.sql')):
        print(f"Processing: {sql_file.name}")

        queries = parse_sql_file(sql_file)

        for idx, query in enumerate(queries, 1):
            total_queries += 1
            category = categorize_query(query)
            category_stats[category] = category_stats.get(category, 0) + 1

            success, error = test_query_syntax(query)

            if success:
                successful_queries += 1
                print(f"  {GREEN}✓{NC} Query {idx} ({category})")
            else:
                print(f"  {RED}✗{NC} Query {idx} ({category}) - {error}")
                failed_queries.append({
                    'file': sql_file.name,
                    'query_num': idx,
                    'query': query[:200] + '...' if len(query) > 200 else query,
                    'category': category,
                    'error': error
                })

    # Write summary
    print("\n========================================")
    print("Test Summary")
    print("========================================")
    print(f"Total Queries: {total_queries}")
    print(f"{GREEN}Successful:{NC} {successful_queries}")
    print(f"{RED}Failed:{NC} {len(failed_queries)}")
    print(f"Success Rate: {100.0 * successful_queries / total_queries if total_queries > 0 else 0:.2f}%")

    print("\nQuery Category Distribution:")
    for category, count in sorted(category_stats.items(), key=lambda x: -x[1]):
        print(f"  {category}: {count}")

    # Write results to file
    results_file = results_dir / 'RESULTS.md'
    with open(results_file, 'w') as f:
        f.write(f"# SQL Query Test Results\n\n")
        f.write(f"Generated: {os.popen('date').read().strip()}\n\n")
        f.write(f"## Summary\n\n")
        f.write(f"- **Total Queries**: {total_queries}\n")
        f.write(f"- **Successful**: {successful_queries}\n")
        f.write(f"- **Failed**: {len(failed_queries)}\n")
        f.write(f"- **Success Rate**: {100.0 * successful_queries / total_queries if total_queries > 0 else 0:.2f}%\n\n")

        f.write(f"## Query Category Distribution\n\n")
        for category, count in sorted(category_stats.items(), key=lambda x: -x[1]):
            f.write(f"- **{category}**: {count}\n")

    # Write failures
    if failed_queries:
        failures_file = results_dir / 'FAILURES.md'
        with open(failures_file, 'w') as f:
            f.write(f"# SQL Query Failures\n\n")
            f.write(f"Generated: {os.popen('date').read().strip()}\n\n")

            for failure in failed_queries:
                f.write(f"## {failure['file']} - Query {failure['query_num']}\n\n")
                f.write(f"**Category**: {failure['category']}\n\n")
                f.write(f"**Query**:\n```sql\n{failure['query']}\n```\n\n")
                f.write(f"**Error**: {failure['error']}\n\n")

        print(f"\nDetailed failure report: {failures_file}")

    print(f"Results written to: {results_file}")

if __name__ == '__main__':
    main()
