//! Print the number of active rewrite rules. Used to verify the
//! README's "~298 rules active" claim and detect regressions in the
//! generated-rules pipeline (e.g. a malformed `.rra` rule that
//! accidentally panics `all_generated_rules` and drops the batch).

#![expect(clippy::print_stdout, reason = "diagnostic example binary")]

fn main() {
    let rules = ra_engine::all_rules();
    let unsorted = ra_engine::all_rules_unsorted();
    println!("all_rules()           = {} rules", rules.len());
    println!("all_rules_unsorted()  = {} rules", unsorted.len());
}
