# Rule: FPGA Hardware Regex Filter

**Category:** hardware/fpga
**File:** `rules/hardware/fpga/fpga-regex-filter.rra`

## Metadata

- **ID:** `fpga-regex-filter`
- **Version:** "1.0.0"
- **Databases:** xilinx-alveo, intel-pac
- **Tags:** fpga, regex, filter, nfa, streaming, pattern-matching
- **Authors:** "RA Contributors"


# FPGA Hardware Regex Filter

## Description

Compiles regular expressions into hardware NFAs (non-deterministic
finite automata) implemented in FPGA logic. Each character of each
input string is processed in a single clock cycle, enabling line-rate
pattern matching. Multiple regex patterns can be evaluated in parallel
using separate NFA instances.

**When to apply**: The query filters on regex or LIKE patterns over
high-volume text data. The pattern must be convertible to an NFA
that fits in the FPGA's LUT budget. This is suited for network
packet inspection, log analysis, and text-heavy analytics.

**Why it works**: An NFA transitions all active states in a single
clock cycle per input character. The FPGA implements each NFA state
as a flip-flop and each transition as combinational logic. Processing
one character per cycle at 200-300 MHz achieves 200-300 million
characters per second per NFA instance, with multiple instances
running in parallel.

## Relational Algebra

```algebra
sigma[REGEXP(col, pattern)](R)
  -> fpga_regex_filter[NFA(pattern)](col, R)
  where pattern is NFA-convertible
    AND nfa_size(pattern) <= fpga_lut_budget
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("fpga-regex-filter";
    "(filter (regexp ?col ?pattern) ?input)" =>
    "(fpga_regex_filter ?col ?pattern ?input)"
    if pattern_fits_fpga_nfa("?pattern")
    if input_is_high_volume("?input")
),
```

## Preconditions

```rust
fn applicable(
    pattern: &str,
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    let nfa_states = estimate_nfa_states(pattern);
    let luts_per_state = 4; // typical for 8-bit character NFA

    let required_luts = nfa_states * luts_per_state;
    required_luts <= hw.fpga_available_luts
        && stats.row_count > 100_000.0
        && !has_backreferences(pattern)
}
```

**Restrictions:**
- No backreferences (not expressible as NFA)
- NFA state count must fit in FPGA LUT budget
- Reconfiguration needed when pattern changes (ms-scale)
- Unicode support requires wider character processing (16-bit)

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    avg_string_len: f64,
    hw: &HardwareProfile,
) -> f64 {
    let total_chars =
        stats.row_count * avg_string_len;

    // CPU: PCRE/RE2 regex at ~100-500 MB/s
    let cpu_throughput_chars_per_ns = 0.3; // ~300 MB/s
    let cpu_ns = total_chars / cpu_throughput_chars_per_ns;

    // FPGA: one char per clock cycle per NFA instance
    let fpga_clock_ns =
        1.0 / hw.fpga_clock_mhz as f64 * 1e3;
    let fpga_nfa_instances = hw.fpga_regex_engines;
    let fpga_ns =
        total_chars * fpga_clock_ns / fpga_nfa_instances as f64;

    if cpu_ns > fpga_ns {
        (cpu_ns - fpga_ns) / cpu_ns
    } else {
        0.0
    }
}
```

**Typical benefit**: 5x-20x for high-volume regex filtering. Multiple
NFA instances can evaluate different patterns or the same pattern on
different data streams simultaneously.

## Test Cases

### Positive: Log analysis regex

```sql
-- web_logs: 5B rows, avg URL length 120 chars
SELECT * FROM web_logs
WHERE url REGEXP '/api/v[0-9]+/orders/[0-9]+/items';

-- Expected: FPGA NFA filter at line rate
-- Plan: FpgaRegexFilter(nfa=compiled, col=url, input=web_logs)
```

### Negative: Backreference pattern

```sql
-- Pattern uses backreference, not NFA-expressible
SELECT * FROM documents
WHERE content REGEXP '(\\w+)\\s+\\1';

-- Expected: CPU regex engine
-- Plan: Filter(regexp=backreference, col=content, input=documents)
```

## References

**Implementation in databases:**
- Xilinx Alveo: Regular expression offload framework
- Intel Hyperscan: Software NFA/DFA matching (CPU baseline)

**Academic papers:**
- Sidhu and Prasanna, "Fast Regular Expression Matching using FPGAs", FCCM 2001
- Becchi and Crowley, "Efficient Regular Expression Evaluation on FPGAs", ANCS 2007
- Zu et al., "GPU-based NFA Implementation for Memory Efficient High Speed Regular Expression Matching", PPoPP 2012
