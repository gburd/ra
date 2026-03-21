#!/usr/bin/env python3
"""
Convert relational algebra notation in .rra files to LaTeX format.

Converts ASCII/Unicode operators in ```algebra blocks to LaTeX math notation.
"""

import re
import sys
from pathlib import Path
from typing import List, Tuple


def convert_line_to_latex(line: str) -> str:
    """Convert a single line of algebra notation to LaTeX."""
    # Skip empty lines and comments
    if not line.strip() or line.strip().startswith('--'):
        return line

    result = line

    # Replace operators with LaTeX equivalents
    # sigma[p] → \sigma_{p}
    result = re.sub(r'sigma\[([^\]]+)\]', r'\\sigma_{\1}', result)

    # pi[cols] → \pi_{cols}
    result = re.sub(r'pi\[([^\]]+)\]', r'\\pi_{\1}', result)

    # gamma[cols] → \gamma_{cols}
    result = re.sub(r'gamma\[([^\]]+)\]', r'\\gamma_{\1}', result)

    # join[c] → \bowtie_{c}
    result = re.sub(r'join\[([^\]]+)\]', r'\\bowtie_{\1}', result)

    # Handle plain "join" without subscript
    result = re.sub(r'\bjoin\b(?!\[)', r'\\bowtie', result)

    # Handle "x" as cross product → \times
    result = re.sub(r'\s+x\s+', r' \\times ', result)

    # -> → \rightarrow
    result = re.sub(r'->', r'\\rightarrow', result)

    # subset → \subseteq
    result = re.sub(r'\bsubset\b', r'\\subseteq', result)

    # superset → \supseteq
    result = re.sub(r'\bsuperset\b', r'\\supseteq', result)

    # union → \cup
    result = re.sub(r'\bunion\b', r'\\cup', result)

    # intersect → \cap
    result = re.sub(r'\bintersect\b', r'\\cap', result)

    # except → \setminus
    result = re.sub(r'\bexcept\b', r'\\setminus', result)

    # Handle "where" → \text{where}
    result = re.sub(r'\bwhere\b', r'\\text{where}', result)

    # Handle function names like attrs(), NDV(), Card() → \text{fname}()
    result = re.sub(r'\b(attrs|NDV|Card|selectivity|min|max|product|estimated_overlap)\s*\(',
                   r'\\text{\1}(', result)

    return result


def convert_algebra_block(lines: List[str]) -> List[str]:
    """Convert a full algebra block to LaTeX format."""
    # Group consecutive non-empty, non-comment lines together
    groups = []
    current_group = []

    for line in lines:
        stripped = line.strip()

        # Check if this is a separator (empty line or comment)
        if not stripped or stripped.startswith('--'):
            if current_group:
                groups.append(current_group)
                current_group = []
            groups.append([line])  # Keep separator as its own group
        else:
            current_group.append(line)

    # Don't forget last group
    if current_group:
        groups.append(current_group)

    # Convert each group
    converted = []
    for group in groups:
        # Check if this group is just a separator
        if len(group) == 1 and (not group[0].strip() or group[0].strip().startswith('--')):
            converted.append(group[0])
            continue

        # This is a content group - wrap in $$
        converted.append('$$')
        for line in group:
            latex_line = convert_line_to_latex(line)
            converted.append(latex_line)
        converted.append('$$')

    return converted


def process_file(file_path: Path, dry_run: bool = False) -> bool:
    """Process a single .rra file and convert algebra blocks to LaTeX."""
    try:
        content = file_path.read_text(encoding='utf-8')
    except Exception as e:
        print(f"Error reading {file_path}: {e}", file=sys.stderr)
        return False

    # Find all ```algebra blocks
    algebra_pattern = r'(```algebra\n)(.*?)(```)'

    def replace_algebra(match):
        header = match.group(1)
        body = match.group(2)
        footer = match.group(3)

        # Split into lines
        lines = body.split('\n')

        # Convert to LaTeX
        converted_lines = convert_algebra_block(lines)

        # Join back
        converted_body = '\n'.join(converted_lines)

        # Change header from algebra to latex
        return f"```latex\n{converted_body}\n{footer}"

    # Perform replacement
    new_content = re.sub(algebra_pattern, replace_algebra, content, flags=re.DOTALL)

    # Check if anything changed
    if new_content == content:
        return False  # No changes needed

    if dry_run:
        print(f"Would update: {file_path}")
        return True

    # Write back
    try:
        file_path.write_text(new_content, encoding='utf-8')
        print(f"Updated: {file_path}")
        return True
    except Exception as e:
        print(f"Error writing {file_path}: {e}", file=sys.stderr)
        return False


def main():
    """Main entry point."""
    import argparse

    parser = argparse.ArgumentParser(description='Convert algebra notation to LaTeX in .rra files')
    parser.add_argument('path', type=Path, help='Directory or file to process')
    parser.add_argument('--dry-run', action='store_true', help='Show what would be changed')

    args = parser.parse_args()

    # Find all .rra files
    if args.path.is_file():
        files = [args.path]
    else:
        files = sorted(args.path.glob('**/*.rra'))

    print(f"Processing {len(files)} files...")

    updated = 0
    for file_path in files:
        if process_file(file_path, dry_run=args.dry_run):
            updated += 1

    print(f"\nSummary: {updated}/{len(files)} files {'would be ' if args.dry_run else ''}updated")


if __name__ == '__main__':
    main()
