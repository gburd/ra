# Rule: Human-Readable Rule Name

**Category:** hardware/SUBCATEGORY
**File:** `rules/templates/template-hardware.rra`

## Metadata

- **ID:** `RULE-ID-HERE`
- **Version:** "1.0.0"
- **Databases:** heavydb, pg-strom
- **Tags:** hardware, gpu, optimization
- **Authors:** "Your Name"


# Rule Name

## Description

Describe the hardware-specific optimization. Explain what hardware
capability it exploits and why it is faster than the CPU baseline.

**When to apply**: Under what data size, hardware availability, and
workload conditions this rule applies.

**Why it works**: How the hardware property (parallelism, bandwidth,
custom logic, etc.) makes this faster than CPU execution.

## Relational Algebra

```algebra
CPU_OPERATOR(R) -> DEVICE_OPERATOR(R)
  where CONDITION
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("RULE-ID-HERE";
    "(CPU_PATTERN)" =>
    "(DEVICE_PATTERN)"
    if GUARD_CONDITION
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Check data size thresholds
    // Check hardware availability
    // Check operator compatibility
    true
}
```

**Restrictions:**
- Data size thresholds (crossover point)
- Hardware requirements (memory, compute units)
- Operator compatibility (what can run on this device)

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> f64 {
    // Model CPU cost
    // Model device cost (transfer + compute)
    // Return improvement fraction
    0.5
}
```

**Assumptions:**
- Hardware parameters (bandwidth, latency, compute)
- Transfer overhead model
- Parallelism model

**Typical benefit**: X-Y range under stated conditions.

## Test Cases

### Positive Case: Large enough for hardware acceleration

```sql
-- Input exceeds crossover threshold
SELECT ...;

-- Expected: uses hardware-accelerated operator
-- Plan: DeviceOperator(...)
```

### Negative Case: Too small for hardware acceleration

```sql
-- Input below crossover threshold
SELECT ...;

-- Expected: stays on CPU
-- Plan: CpuOperator(...)
```

## References

**Implementation in databases:**
- Database: `path/to/source/file` - description

**Academic papers:**
- Author, "Title", Conference Year
