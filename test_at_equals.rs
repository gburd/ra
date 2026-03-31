// Quick test of @= operator parsing
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

fn main() {
    let dialect = PostgreSqlDialect {};
    let sql = "SELECT * FROM t WHERE data @= '{}'";

    match Parser::parse_sql(&dialect, sql) {
        Ok(stmts) => {
            println!("✓ Parsed successfully!");
            println!("Statements: {:#?}", stmts);
        }
        Err(e) => {
            println!("✗ Parse failed: {}", e);
        }
    }
}
