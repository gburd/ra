//! Differential DML tests: compare Ra planner DML results against native PG.
//!
//! Requires Docker services (postgres-native on 15432, postgres-ra on 15433).
//! Run via: `./scripts/run-difftest.sh` or manually start Docker services first.
//! Tests skip gracefully when services are unavailable.

use ra_difftest::{DiffResult, DiffTestRunner};

fn ra_url() -> String {
    std::env::var("RA_DATABASE_URL").unwrap_or_else(|_| {
        "host=localhost port=15433 user=ra_test password=ra_test dbname=ra_test".to_string()
    })
}

fn native_url() -> String {
    std::env::var("NATIVE_DATABASE_URL").unwrap_or_else(|_| {
        "host=localhost port=15432 user=ra_test password=ra_test dbname=ra_test".to_string()
    })
}

async fn runner() -> Option<DiffTestRunner> {
    match DiffTestRunner::connect(&ra_url(), &native_url()).await {
        Ok(r) => Some(r),
        Err(_) => {
            eprintln!("SKIP: Docker services not available (run ./scripts/run-difftest.sh)");
            None
        }
    }
}

fn assert_match(result: DiffResult) {
    match result {
        DiffResult::Match { .. } => {}
        other => panic!("Expected Match, got: {other:?}"),
    }
}

// =========================================================================
// INSERT tests
// =========================================================================

#[tokio::test]
async fn test_diff_insert_basic_values() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml(
            &[
                "DROP TABLE IF EXISTS diff_ins_basic CASCADE",
                "CREATE TABLE diff_ins_basic (id INT PRIMARY KEY, name TEXT, val INT)",
            ],
            "INSERT INTO diff_ins_basic VALUES (1, 'alpha', 10), (2, 'beta', 20), (3, 'gamma', 30)",
            "SELECT * FROM diff_ins_basic ORDER BY id",
        )
        .await;
    assert_match(result);
}

#[tokio::test]
async fn test_diff_insert_from_select() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml(
            &[
                "DROP TABLE IF EXISTS diff_ins_src CASCADE",
                "DROP TABLE IF EXISTS diff_ins_dst CASCADE",
                "CREATE TABLE diff_ins_src (id INT, val INT)",
                "CREATE TABLE diff_ins_dst (id INT, val INT)",
                "INSERT INTO diff_ins_src VALUES (1, 100), (2, 200), (3, 300), (4, 400)",
            ],
            "INSERT INTO diff_ins_dst SELECT * FROM diff_ins_src WHERE val > 200",
            "SELECT * FROM diff_ins_dst ORDER BY id",
        )
        .await;
    assert_match(result);
}

#[tokio::test]
async fn test_diff_insert_on_conflict_do_nothing() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml(
            &[
                "DROP TABLE IF EXISTS diff_ins_conflict CASCADE",
                "CREATE TABLE diff_ins_conflict (id INT PRIMARY KEY, val TEXT)",
                "INSERT INTO diff_ins_conflict VALUES (1, 'existing')",
            ],
            "INSERT INTO diff_ins_conflict VALUES (1, 'dup'), (2, 'new') ON CONFLICT (id) DO NOTHING",
            "SELECT * FROM diff_ins_conflict ORDER BY id",
        )
        .await;
    assert_match(result);
}

#[tokio::test]
async fn test_diff_insert_on_conflict_do_update() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml(
            &[
                "DROP TABLE IF EXISTS diff_ins_upsert CASCADE",
                "CREATE TABLE diff_ins_upsert (id INT PRIMARY KEY, val INT)",
                "INSERT INTO diff_ins_upsert VALUES (1, 10), (2, 20)",
            ],
            "INSERT INTO diff_ins_upsert VALUES (1, 99), (3, 30) ON CONFLICT (id) DO UPDATE SET val = EXCLUDED.val",
            "SELECT * FROM diff_ins_upsert ORDER BY id",
        )
        .await;
    assert_match(result);
}

#[tokio::test]
async fn test_diff_insert_returning() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml_returning(
            &[
                "DROP TABLE IF EXISTS diff_ins_ret CASCADE",
                "CREATE TABLE diff_ins_ret (id SERIAL PRIMARY KEY, name TEXT)",
            ],
            "INSERT INTO diff_ins_ret (name) VALUES ('a'), ('b'), ('c') RETURNING id, name",
            true,
        )
        .await;
    assert_match(result);
}

// =========================================================================
// UPDATE tests
// =========================================================================

#[tokio::test]
async fn test_diff_update_basic() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml(
            &[
                "DROP TABLE IF EXISTS diff_upd_basic CASCADE",
                "CREATE TABLE diff_upd_basic (id INT PRIMARY KEY, val INT)",
                "INSERT INTO diff_upd_basic VALUES (1, 10), (2, 20), (3, 30)",
            ],
            "UPDATE diff_upd_basic SET val = 99 WHERE id = 2",
            "SELECT * FROM diff_upd_basic ORDER BY id",
        )
        .await;
    assert_match(result);
}

#[tokio::test]
async fn test_diff_update_expression() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml(
            &[
                "DROP TABLE IF EXISTS diff_upd_expr CASCADE",
                "CREATE TABLE diff_upd_expr (id INT PRIMARY KEY, val INT)",
                "INSERT INTO diff_upd_expr VALUES (1, 10), (2, 20), (3, 30)",
            ],
            "UPDATE diff_upd_expr SET val = val * 2 WHERE val > 15",
            "SELECT * FROM diff_upd_expr ORDER BY id",
        )
        .await;
    assert_match(result);
}

#[tokio::test]
async fn test_diff_update_from_clause() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml(
            &[
                "DROP TABLE IF EXISTS diff_upd_src CASCADE",
                "DROP TABLE IF EXISTS diff_upd_dst CASCADE",
                "CREATE TABLE diff_upd_src (id INT PRIMARY KEY, new_val INT)",
                "CREATE TABLE diff_upd_dst (id INT PRIMARY KEY, val INT)",
                "INSERT INTO diff_upd_src VALUES (1, 100), (2, 200)",
                "INSERT INTO diff_upd_dst VALUES (1, 10), (2, 20), (3, 30)",
            ],
            "UPDATE diff_upd_dst SET val = s.new_val FROM diff_upd_src s WHERE diff_upd_dst.id = s.id",
            "SELECT * FROM diff_upd_dst ORDER BY id",
        )
        .await;
    assert_match(result);
}

#[tokio::test]
async fn test_diff_update_returning() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml_returning(
            &[
                "DROP TABLE IF EXISTS diff_upd_ret CASCADE",
                "CREATE TABLE diff_upd_ret (id INT PRIMARY KEY, val INT)",
                "INSERT INTO diff_upd_ret VALUES (1, 10), (2, 20), (3, 30)",
            ],
            "UPDATE diff_upd_ret SET val = val + 5 WHERE id <= 2 RETURNING id, val",
            true,
        )
        .await;
    assert_match(result);
}

// =========================================================================
// DELETE tests
// =========================================================================

#[tokio::test]
async fn test_diff_delete_basic() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml(
            &[
                "DROP TABLE IF EXISTS diff_del_basic CASCADE",
                "CREATE TABLE diff_del_basic (id INT PRIMARY KEY, val INT)",
                "INSERT INTO diff_del_basic VALUES (1, 10), (2, 20), (3, 30), (4, 40)",
            ],
            "DELETE FROM diff_del_basic WHERE val > 25",
            "SELECT * FROM diff_del_basic ORDER BY id",
        )
        .await;
    assert_match(result);
}

#[tokio::test]
async fn test_diff_delete_using() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml(
            &[
                "DROP TABLE IF EXISTS diff_del_ref CASCADE",
                "DROP TABLE IF EXISTS diff_del_tgt CASCADE",
                "CREATE TABLE diff_del_ref (id INT PRIMARY KEY, remove BOOLEAN)",
                "CREATE TABLE diff_del_tgt (id INT PRIMARY KEY, ref_id INT)",
                "INSERT INTO diff_del_ref VALUES (1, true), (2, false)",
                "INSERT INTO diff_del_tgt VALUES (10, 1), (20, 2), (30, 1)",
            ],
            "DELETE FROM diff_del_tgt USING diff_del_ref r WHERE diff_del_tgt.ref_id = r.id AND r.remove = true",
            "SELECT * FROM diff_del_tgt ORDER BY id",
        )
        .await;
    assert_match(result);
}

#[tokio::test]
async fn test_diff_delete_returning() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml_returning(
            &[
                "DROP TABLE IF EXISTS diff_del_ret CASCADE",
                "CREATE TABLE diff_del_ret (id INT PRIMARY KEY, val INT)",
                "INSERT INTO diff_del_ret VALUES (1, 10), (2, 20), (3, 30)",
            ],
            "DELETE FROM diff_del_ret WHERE id = 2 RETURNING id, val",
            true,
        )
        .await;
    assert_match(result);
}

#[tokio::test]
async fn test_diff_delete_all_rows() {
    let Some(r) = runner().await else { return };
    let result = r
        .compare_dml(
            &[
                "DROP TABLE IF EXISTS diff_del_all CASCADE",
                "CREATE TABLE diff_del_all (id INT, val TEXT)",
                "INSERT INTO diff_del_all VALUES (1, 'a'), (2, 'b'), (3, 'c')",
            ],
            "DELETE FROM diff_del_all",
            "SELECT * FROM diff_del_all ORDER BY id",
        )
        .await;
    assert_match(result);
}
