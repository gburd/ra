# Rule: Column-at-a-Time SWAR String Processing

**Category:** execution-models/column-at-a-time
**File:** `rules/execution-models/column-at-a-time/column-string-swar.rra`

## Metadata

- **ID:** `column-string-swar`
- **Version:** "1.0.0"
- **Databases:** monetdb, duckdb
- **Tags:** execution, columnar, string, swar, simd, bitwise


# Column-at-a-Time SWAR String Processing

## Description

Uses SWAR (SIMD Within A Register) techniques to process multiple string
characters simultaneously using standard 64-bit integer operations. This enables
string comparisons, LIKE pattern matching, and string hashing to process 8
characters per CPU cycle using word-level parallelism on columnar string data.

**When to apply**: String predicates (equality, prefix match, LIKE) on columnar
string data where the string length is short enough to fit in one or two 64-bit
registers (up to 16 bytes). SWAR avoids the overhead of byte-by-byte comparison
and does not require SIMD instruction set extensions.

**Why it works**: Loading an 8-byte string into a 64-bit register and comparing
it with a single integer comparison processes all 8 characters in one instruction.
For prefix matching, masking the register to the prefix length and comparing gives
an 8x throughput improvement over byte-at-a-time comparison. This technique works
on all CPU architectures without SIMD extensions.

## Implementation

```rust
/// Compare short strings using SWAR (8 bytes at a time)
pub fn swar_string_eq(data: &[u8], pattern: &[u8; 8]) -> bool {
    let data_word = u64::from_le_bytes(
        data[..8].try_into().unwrap_or([0u8; 8]));
    let pattern_word = u64::from_le_bytes(*pattern);
    data_word == pattern_word
}

/// SWAR prefix match: compare first `len` bytes
pub fn swar_prefix_match(
    data: &[u8], prefix: &[u8], len: usize,
) -> bool {
    let mask = if len >= 8 { u64::MAX } else { (1u64 << (len * 8)) - 1 };
    let data_word = u64::from_le_bytes(
        data[..8].try_into().unwrap_or([0u8; 8]));
    let prefix_word = u64::from_le_bytes(
        prefix[..8].try_into().unwrap_or([0u8; 8]));
    (data_word & mask) == (prefix_word & mask)
}
```

## Cost Model

- Byte-at-a-time: O(n * L) comparisons where L = string length
- SWAR: O(n * ceil(L/8)) comparisons
- Speedup: up to 8x for short strings, diminishing for strings > 16 bytes
- No SIMD instructions required (works on any 64-bit CPU)

## Test Cases

```sql
-- Short string equality: SWAR processes 8 chars at once
SELECT * FROM users WHERE country = 'US';
-- 'US\0\0\0\0\0\0' loaded as u64, single comparison per row

-- Prefix matching
SELECT * FROM logs WHERE message LIKE 'ERROR:%';
-- First 6 bytes compared as masked u64
-- 6x faster than byte-by-byte for 100M rows
```

## References

1. Boncz et al., "MonetDB/X100: Hyper-Pipelining Query Execution", CIDR 2005
2. Mytkowicz et al., "Data-Parallel Finite-State Machines", ASPLOS 2014
3. Muhlbauer et al., "Exploiting Hardware Transactional Memory in Main-Memory
   Databases", ICDE 2015
