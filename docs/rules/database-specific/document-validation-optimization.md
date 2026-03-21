# Rule: MongoDB Document Validation Optimization

**Category:** database-specific/mongodb
**File:** `rules/database-specific/mongodb/document-validation-optimization.rra`

## Metadata

- **ID:** `mongodb-document-validation`
- **Version:** "1.0.0"
- **Databases:** mongodb
- **Tags:** validation, schema, jsonschema, performance
- **Authors:** "MongoDB Inc."


# MongoDB Document Validation Optimization

## Description

Optimizes document validation by compiling JSON Schema validators once and
caching them, using minimal validation levels when appropriate, and pushing
validation to application layer when validation overhead is too high for
write-heavy workloads.

**When to apply**: Collections with schema validation enabled. For write-heavy
workloads, validation overhead can be reduced by using "moderate" validation
level or optimizing schema complexity.

**Why it works**: Document validation adds CPU overhead on every write. By
caching compiled validators, using appropriate validation levels (strict vs
moderate), and simplifying complex schemas, validation cost is minimized while
maintaining data quality.

## Test Cases

### Positive: Cached validator compilation

```javascript
// Validator is compiled once and cached
db.createCollection("users", {
  validator: {
    $jsonSchema: {
      bsonType: "object",
      required: ["email", "age"],
      properties: {
        email: {bsonType: "string", pattern: "^.+@.+$"},
        age: {bsonType: "int", minimum: 0, maximum: 120}
      }
    }
  },
  validationLevel: "moderate"  // Only validates updates to validated docs
})
```

## References

**Documentation:**
- MongoDB Manual: "Schema Validation"
- https://docs.mongodb.com/manual/core/schema-validation/
