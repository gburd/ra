//! DML integration tests for the Ra planner extension.
//!
//! Each test creates a table, seeds it using the stock planner, then enables
//! the Ra planner to execute DML, and verifies the resulting table state.

use pgrx::prelude::*;

#[cfg(any(test, feature = "pg_test"))]
#[pg_schema]
mod tests {
    use pgrx::prelude::*;

    /// Run DML with Ra planner enabled, then verify state with stock planner.
    fn run_dml_and_verify(
        setup_sqls: &[&str],
        dml_sql: &str,
        verify_sql: &str,
        expected: i64,
    ) {
        for sql in setup_sqls {
            Spi::run(sql).unwrap();
        }

        // Enable Ra planner for the DML statement
        Spi::run("SET ra_planner.enabled = true").unwrap();
        Spi::run(dml_sql).unwrap();
        Spi::run("SET ra_planner.enabled = false").unwrap();

        // Verify with stock planner
        let result = Spi::get_one::<i64>(verify_sql);
        assert_eq!(
            result,
            Ok(Some(expected)),
            "DML: {dml_sql}\nVerify: {verify_sql}\nExpected: {expected}, Got: {result:?}"
        );
    }

    #[pg_test]
    fn test_dml_insert_basic_values() {
        run_dml_and_verify(
            &[
                "DROP TABLE IF EXISTS dml_insert_basic CASCADE",
                "CREATE TABLE dml_insert_basic (id INT PRIMARY KEY, name TEXT, val INT)",
            ],
            "INSERT INTO dml_insert_basic VALUES (1, 'alpha', 10), (2, 'beta', 20), (3, 'gamma', 30)",
            "SELECT COUNT(*) FROM dml_insert_basic",
            3,
        );
    }

    #[pg_test]
    fn test_dml_insert_from_select() {
        run_dml_and_verify(
            &[
                "DROP TABLE IF EXISTS dml_insert_src CASCADE",
                "DROP TABLE IF EXISTS dml_insert_dst CASCADE",
                "CREATE TABLE dml_insert_src (id INT, val INT)",
                "CREATE TABLE dml_insert_dst (id INT, val INT)",
                "INSERT INTO dml_insert_src VALUES (1, 100), (2, 200), (3, 300)",
            ],
            "INSERT INTO dml_insert_dst SELECT * FROM dml_insert_src WHERE val > 100",
            "SELECT COUNT(*) FROM dml_insert_dst",
            2,
        );
    }

    #[pg_test]
    fn test_dml_insert_on_conflict_do_nothing() {
        run_dml_and_verify(
            &[
                "DROP TABLE IF EXISTS dml_insert_conflict CASCADE",
                "CREATE TABLE dml_insert_conflict (id INT PRIMARY KEY, val TEXT)",
                "INSERT INTO dml_insert_conflict VALUES (1, 'original')",
            ],
            "INSERT INTO dml_insert_conflict VALUES (1, 'duplicate'), (2, 'new') ON CONFLICT (id) DO NOTHING",
            "SELECT COUNT(*) FROM dml_insert_conflict",
            2,
        );
    }

    #[pg_test]
    fn test_dml_insert_on_conflict_do_update() {
        run_dml_and_verify(
            &[
                "DROP TABLE IF EXISTS dml_insert_upsert CASCADE",
                "CREATE TABLE dml_insert_upsert (id INT PRIMARY KEY, val INT)",
                "INSERT INTO dml_insert_upsert VALUES (1, 10), (2, 20)",
            ],
            "INSERT INTO dml_insert_upsert VALUES (1, 99), (3, 30) ON CONFLICT (id) DO UPDATE SET val = EXCLUDED.val",
            "SELECT SUM(val) FROM dml_insert_upsert",
            149,  // 99 + 20 + 30
        );
    }

    #[pg_test]
    fn test_dml_insert_returning() {
        Spi::run("DROP TABLE IF EXISTS dml_insert_ret CASCADE").unwrap();
        Spi::run("CREATE TABLE dml_insert_ret (id SERIAL PRIMARY KEY, name TEXT)").unwrap();

        Spi::run("SET ra_planner.enabled = true").unwrap();
        let result = Spi::get_one::<i32>(
            "INSERT INTO dml_insert_ret (name) VALUES ('test') RETURNING id",
        );
        Spi::run("SET ra_planner.enabled = false").unwrap();

        assert_eq!(result, Ok(Some(1)));
    }

    #[pg_test]
    fn test_dml_update_basic() {
        run_dml_and_verify(
            &[
                "DROP TABLE IF EXISTS dml_update_basic CASCADE",
                "CREATE TABLE dml_update_basic (id INT PRIMARY KEY, val INT)",
                "INSERT INTO dml_update_basic VALUES (1, 10), (2, 20), (3, 30)",
            ],
            "UPDATE dml_update_basic SET val = 99 WHERE id = 2",
            "SELECT val FROM dml_update_basic WHERE id = 2",
            99,
        );
    }

    #[pg_test]
    fn test_dml_update_expression() {
        run_dml_and_verify(
            &[
                "DROP TABLE IF EXISTS dml_update_expr CASCADE",
                "CREATE TABLE dml_update_expr (id INT PRIMARY KEY, val INT)",
                "INSERT INTO dml_update_expr VALUES (1, 10), (2, 20), (3, 30)",
            ],
            "UPDATE dml_update_expr SET val = val * 2 WHERE val > 10",
            "SELECT SUM(val) FROM dml_update_expr",
            110,  // 10 + 40 + 60
        );
    }

    #[pg_test]
    fn test_dml_update_from_clause() {
        run_dml_and_verify(
            &[
                "DROP TABLE IF EXISTS dml_update_from_src CASCADE",
                "DROP TABLE IF EXISTS dml_update_from_dst CASCADE",
                "CREATE TABLE dml_update_from_src (id INT PRIMARY KEY, new_val INT)",
                "CREATE TABLE dml_update_from_dst (id INT PRIMARY KEY, val INT)",
                "INSERT INTO dml_update_from_src VALUES (1, 100), (2, 200)",
                "INSERT INTO dml_update_from_dst VALUES (1, 10), (2, 20), (3, 30)",
            ],
            "UPDATE dml_update_from_dst SET val = s.new_val FROM dml_update_from_src s WHERE dml_update_from_dst.id = s.id",
            "SELECT SUM(val) FROM dml_update_from_dst",
            330,  // 100 + 200 + 30
        );
    }

    #[pg_test]
    fn test_dml_update_returning() {
        Spi::run("DROP TABLE IF EXISTS dml_update_ret CASCADE").unwrap();
        Spi::run("CREATE TABLE dml_update_ret (id INT PRIMARY KEY, val INT)").unwrap();
        Spi::run("INSERT INTO dml_update_ret VALUES (1, 10), (2, 20)").unwrap();

        Spi::run("SET ra_planner.enabled = true").unwrap();
        let result = Spi::get_one::<i32>(
            "UPDATE dml_update_ret SET val = 99 WHERE id = 1 RETURNING val",
        );
        Spi::run("SET ra_planner.enabled = false").unwrap();

        assert_eq!(result, Ok(Some(99)));
    }

    #[pg_test]
    fn test_dml_delete_basic() {
        run_dml_and_verify(
            &[
                "DROP TABLE IF EXISTS dml_delete_basic CASCADE",
                "CREATE TABLE dml_delete_basic (id INT PRIMARY KEY, val INT)",
                "INSERT INTO dml_delete_basic VALUES (1, 10), (2, 20), (3, 30), (4, 40)",
            ],
            "DELETE FROM dml_delete_basic WHERE val > 20",
            "SELECT COUNT(*) FROM dml_delete_basic",
            2,
        );
    }

    #[pg_test]
    fn test_dml_delete_using() {
        run_dml_and_verify(
            &[
                "DROP TABLE IF EXISTS dml_delete_using_ref CASCADE",
                "DROP TABLE IF EXISTS dml_delete_using_tgt CASCADE",
                "CREATE TABLE dml_delete_using_ref (id INT PRIMARY KEY, remove BOOLEAN)",
                "CREATE TABLE dml_delete_using_tgt (id INT PRIMARY KEY, ref_id INT)",
                "INSERT INTO dml_delete_using_ref VALUES (1, true), (2, false)",
                "INSERT INTO dml_delete_using_tgt VALUES (10, 1), (20, 2), (30, 1)",
            ],
            "DELETE FROM dml_delete_using_tgt USING dml_delete_using_ref r WHERE dml_delete_using_tgt.ref_id = r.id AND r.remove = true",
            "SELECT COUNT(*) FROM dml_delete_using_tgt",
            1,
        );
    }

    #[pg_test]
    fn test_dml_delete_returning() {
        Spi::run("DROP TABLE IF EXISTS dml_delete_ret CASCADE").unwrap();
        Spi::run("CREATE TABLE dml_delete_ret (id INT PRIMARY KEY, val INT)").unwrap();
        Spi::run("INSERT INTO dml_delete_ret VALUES (1, 10), (2, 20), (3, 30)").unwrap();

        Spi::run("SET ra_planner.enabled = true").unwrap();
        let result = Spi::get_one::<i32>(
            "DELETE FROM dml_delete_ret WHERE id = 2 RETURNING val",
        );
        Spi::run("SET ra_planner.enabled = false").unwrap();

        assert_eq!(result, Ok(Some(20)));
    }

    #[pg_test]
    fn test_dml_delete_all_rows() {
        run_dml_and_verify(
            &[
                "DROP TABLE IF EXISTS dml_delete_all CASCADE",
                "CREATE TABLE dml_delete_all (id INT, val TEXT)",
                "INSERT INTO dml_delete_all VALUES (1, 'a'), (2, 'b'), (3, 'c')",
            ],
            "DELETE FROM dml_delete_all",
            "SELECT COUNT(*) FROM dml_delete_all",
            0,
        );
    }
}
