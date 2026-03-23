# Rule: MongoDB Text Search Index Optimization

**Category:** database-specific/mongodb
**File:** `rules/database-specific/mongodb/text-search-index.rra`

## Metadata

- **ID:** `mongodb-text-search-index`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** text-search, full-text, index, tokenization
- **Authors:** "MongoDB Inc."


# MongoDB Text Search Index Optimization

## Description

Uses text indexes for $text queries, enabling efficient full-text search with
stemming, stop words, and relevance scoring. Text indexes tokenize and index
words, dramatically outperforming regex scans on large text fields.

**When to apply**: Queries searching text content ($text operator) in string
fields. Text indexes enable linguistic search (stemming, case-insensitive)
with ranking by relevance score.

**Why it works**: Text indexes use inverted index structure (word -> document IDs)
allowing fast lookups of documents containing search terms. Regex on unindexed
fields requires scanning all documents.

## Test Cases

### Positive: $text search with index

```javascript
// Index: {description: "text", title: "text"}
db.articles.find({
  $text: {$search: "mongodb database optimization"}
}, {
  score: {$meta: "textScore"}
}).sort({score: {$meta: "textScore"}})

// Returns documents matching terms, sorted by relevance
```

### Negative: Regex without index (slow)

```javascript
// No text index - full collection scan
db.articles.find({
  description: /mongodb.*optimization/i
})
// O(n) with regex evaluation per document
```

## References

**Documentation:**
- MongoDB Manual: "Text Indexes"
- https://docs.mongodb.com/manual/core/index-text/
