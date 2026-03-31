# sqlparser-rs Fork Plan: Add DocumentDB/BSON Operators

## Current Issue

sqlparser-rs v0.52.0 tokenizer doesn't recognize DocumentDB BSON operators:
- `@=` (exact match)
- `@>=` (greater than or equal with JSON semantics)
- `@<=` (less than or equal with JSON semantics)

**Location:** `src/tokenizer.rs` lines 1188-1208

## Changes Needed

### 1. Add Token Variants

**File:** `src/tokenizer.rs` (around line 100-200 where Token enum is defined)

Add after existing @ tokens:
```rust
/// `@=` operator (DocumentDB/BSON exact match)
AtEquals,
/// `@>=` operator (DocumentDB/BSON greater-equal comparison)
AtGreaterEquals,
/// `@<=` operator (DocumentDB/BSON less-equal comparison)
AtLessEquals,
```

### 2. Update Tokenizer Logic

**File:** `src/tokenizer.rs` lines 1188-1208

**Current code:**
```rust
'@' => {
    chars.next();
    match chars.peek() {
        Some('>') => self.consume_and_return(chars, Token::AtArrow),
        Some('?') => self.consume_and_return(chars, Token::AtQuestion),
        Some('@') => {
            chars.next();
            match chars.peek() {
                Some(' ') => Ok(Some(Token::AtAt)),
                Some(tch) if self.dialect.is_identifier_start('@') => {
                    self.tokenize_identifier_or_keyword([ch, '@', *tch], chars)
                }
                _ => Ok(Some(Token::AtAt)),
            }
        }
        Some(' ') => Ok(Some(Token::AtSign)),
        Some(sch) if self.dialect.is_identifier_start('@') => {
            self.tokenize_identifier_or_keyword([ch, *sch], chars)
        }
        _ => Ok(Some(Token::AtSign)),
    }
}
```

**Updated code:**
```rust
'@' => {
    chars.next();
    match chars.peek() {
        Some('>') => {
            chars.next();
            match chars.peek() {
                Some('=') => self.consume_and_return(chars, Token::AtGreaterEquals),
                _ => Ok(Some(Token::AtArrow)),
            }
        }
        Some('<') => {
            chars.next();
            match chars.peek() {
                Some('=') => self.consume_and_return(chars, Token::AtLessEquals),
                _ => self.start_binop(chars, "@<", Token::AtLess),  // @< already exists
            }
        }
        Some('=') => self.consume_and_return(chars, Token::AtEquals),
        Some('?') => self.consume_and_return(chars, Token::AtQuestion),
        Some('@') => {
            chars.next();
            match chars.peek() {
                Some(' ') => Ok(Some(Token::AtAt)),
                Some(tch) if self.dialect.is_identifier_start('@') => {
                    self.tokenize_identifier_or_keyword([ch, '@', *tch], chars)
                }
                _ => Ok(Some(Token::AtAt)),
            }
        }
        Some(' ') => Ok(Some(Token::AtSign)),
        Some(sch) if self.dialect.is_identifier_start('@') => {
            self.tokenize_identifier_or_keyword([ch, *sch], chars)
        }
        _ => Ok(Some(Token::AtSign)),
    }
}
```

### 3. Add Binary Operator Support

**File:** `src/parser.rs` (in the binary operator precedence function)

Add these operators to the precedence table with comparison operator precedence (typically 20):
```rust
Token::AtEquals => 20,
Token::AtGreaterEquals => 20,
Token::AtLessEquals => 20,
```

### 4. Add Tests

**File:** `tests/sqlparser_common.rs` or `tests/sqlparser_postgres.rs`

```rust
#[test]
fn test_documentdb_operators() {
    let sql = "SELECT * FROM t WHERE data @= '{}'";
    let dialect = PostgreSqlDialect {};
    let ast = Parser::parse_sql(&dialect, sql).unwrap();
    // Verify @= is recognized as binary operator

    let sql2 = "SELECT * FROM t WHERE data @>= '{}'";
    let ast2 = Parser::parse_sql(&dialect, sql2).unwrap();
    // Verify @>= is recognized

    let sql3 = "SELECT * FROM t WHERE data @<= '{}'";
    let ast3 = Parser::parse_sql(&dialect, sql3).unwrap();
    // Verify @<= is recognized
}
```

## Implementation Steps

### Option 1: Patch in Ra Codebase (Quick)

1. Copy sqlparser-rs 0.52.0 into `crates/sqlparser-ra/`
2. Apply the 3 changes above
3. Update `Cargo.toml`:
   ```toml
   sqlparser = { path = "../sqlparser-ra", version = "0.52.0" }
   ```
4. Test with DocumentDB queries
5. Commit to ra repository

**Time:** 2-3 hours
**Pros:** Immediate fix, full control
**Cons:** Maintenance burden, need to track upstream

### Option 2: Fork on GitHub (Better)

1. Fork sqlparser-rs to `ra-query-optimizer/sqlparser-rs`
2. Create branch `add-documentdb-operators`
3. Apply changes
4. Push to fork
5. Update `Cargo.toml`:
   ```toml
   sqlparser = { git = "https://github.com/ra-query-optimizer/sqlparser-rs", branch = "add-documentdb-operators" }
   ```
6. Later: Submit upstream PR to sqlparser-rs/sqlparser-rs

**Time:** 3-4 hours (including fork setup)
**Pros:** Can contribute upstream later, cleaner
**Cons:** GitHub access required

### Option 3: Contribute Upstream First (Slowest but best)

1. Fork sqlparser-rs officially
2. Apply changes on feature branch
3. Run full sqlparser test suite
4. Submit PR immediately
5. Use fork temporarily until PR merged

**Time:** 1-2 weeks (waiting for review)
**Pros:** Fixes for everyone, no maintenance burden
**Cons:** Blocks Ra development while waiting

## Recommendation

**Use Option 1 for now:**
- Put sqlparser fork in `crates/sqlparser-ra/`
- Apply minimal patch for @=, @>=, @<= operators
- Get DocumentDB queries working today
- Then pursue Option 3 (contribute upstream) in parallel

This unblocks Ra development while doing the right thing for the ecosystem.

## Test Query

After applying patch, this should work:
```sql
SELECT document
FROM documentdb_api.collection('mydb', 'users')
WHERE document @= '{"status": "active"}'
  AND document @>= '{"age": 25}'
  AND document @<= '{"age": 65}';
```

## Next Steps

1. Copy sqlparser 0.52.0 to `crates/sqlparser-ra/`
2. Apply tokenizer patches
3. Test with ra-cli
4. Commit and verify
5. Submit upstream PR (separate workstream)
