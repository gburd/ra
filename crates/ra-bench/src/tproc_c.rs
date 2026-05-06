//! TPROC-C (TPC-C-like) OLTP query set for benchmarking short transactions.
//!
//! TPC-C defines 5 transaction types that represent a typical OLTP workload:
//!
//! | Transaction    | Mix % | Tables | Purpose                                |
//! |----------------|-------|--------|----------------------------------------|
//! | New-Order      | 45%   | 9      | Insert a new customer order            |
//! | Payment        | 43%   | 4      | Process a customer payment             |
//! | Order-Status   | 4%    | 3      | Query customer's last order            |
//! | Delivery       | 4%    | 4      | Process pending delivery orders        |
//! | Stock-Level    | 4%    | 2      | Check warehouse stock below threshold  |
//!
//! The SELECT queries here represent the read-heavy portions of each
//! transaction type. They are designed to test:
//! - Index lookups by primary key
//! - Small aggregations on indexed columns
//! - Short range scans
//! - Simple multi-table joins with small cardinalities
//!
//! # Schema
//!
//! See `scripts/tpcc-schema.sql` for the DDL. The schema models a warehouse
//! with districts, customers, orders, and stock.

/// A TPROC-C query with metadata.
#[derive(Debug, Clone)]
pub struct TprocCQuery {
    /// Transaction type identifier.
    pub transaction: TprocCTransaction,
    /// Human-readable query identifier.
    pub id: &'static str,
    /// SQL SELECT statement.
    pub sql: &'static str,
    /// Expected cardinality category (for result validation).
    pub expected_rows: RowEstimate,
}

/// The TPC-C transaction type this query belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TprocCTransaction {
    NewOrder,
    Payment,
    OrderStatus,
    Delivery,
    StockLevel,
}

/// Expected cardinality category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowEstimate {
    /// Exactly 1 row (PK lookup).
    Single,
    /// A few rows (< 100).
    Few,
    /// Many rows (100+).
    Many,
}

/// Returns the complete TPROC-C query set.
pub fn tproc_c_queries() -> Vec<TprocCQuery> {
    vec![
        // ---- New-Order reads -----------------------------------------------
        TprocCQuery {
            transaction: TprocCTransaction::NewOrder,
            id: "NO_warehouse",
            sql: "SELECT w_tax FROM warehouse WHERE w_id = 1",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::NewOrder,
            id: "NO_district",
            sql: "SELECT d_tax, d_next_o_id FROM district \
                   WHERE d_w_id = 1 AND d_id = 1",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::NewOrder,
            id: "NO_customer",
            sql: "SELECT c_discount, c_last, c_credit \
                    FROM customer \
                   WHERE c_w_id = 1 AND c_d_id = 1 AND c_id = 1",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::NewOrder,
            id: "NO_item",
            sql: "SELECT i_price, i_name, i_data \
                    FROM item \
                   WHERE i_id = 42",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::NewOrder,
            id: "NO_stock",
            sql: "SELECT s_quantity, s_data, s_dist_01, s_dist_02, \
                         s_dist_03, s_dist_04, s_dist_05 \
                    FROM stock \
                   WHERE s_i_id = 42 AND s_w_id = 1",
            expected_rows: RowEstimate::Single,
        },

        // ---- Payment reads --------------------------------------------------
        TprocCQuery {
            transaction: TprocCTransaction::Payment,
            id: "PAY_warehouse",
            sql: "SELECT w_name, w_street_1, w_street_2, w_city, \
                         w_state, w_zip \
                    FROM warehouse \
                   WHERE w_id = 1",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::Payment,
            id: "PAY_district",
            sql: "SELECT d_name, d_street_1, d_street_2, d_city, \
                         d_state, d_zip \
                    FROM district \
                   WHERE d_w_id = 1 AND d_id = 1",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::Payment,
            id: "PAY_customer_by_id",
            sql: "SELECT c_first, c_middle, c_last, \
                         c_street_1, c_street_2, c_city, c_state, c_zip, \
                         c_phone, c_credit, c_credit_lim, \
                         c_discount, c_balance, c_since \
                    FROM customer \
                   WHERE c_w_id = 1 AND c_d_id = 1 AND c_id = 1",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::Payment,
            id: "PAY_customer_by_name",
            sql: "SELECT c_id, c_first, c_middle, \
                         c_credit, c_credit_lim, c_discount, c_balance \
                    FROM customer \
                   WHERE c_w_id = 1 AND c_d_id = 1 \
                     AND c_last = 'BARBARBAR' \
                   ORDER BY c_first",
            expected_rows: RowEstimate::Few,
        },

        // ---- Order-Status reads ---------------------------------------------
        TprocCQuery {
            transaction: TprocCTransaction::OrderStatus,
            id: "OS_customer",
            sql: "SELECT c_first, c_middle, c_last, c_balance \
                    FROM customer \
                   WHERE c_w_id = 1 AND c_d_id = 1 AND c_id = 1",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::OrderStatus,
            id: "OS_last_order",
            sql: "SELECT o_id, o_carrier_id, o_entry_d \
                    FROM orders \
                   WHERE o_w_id = 1 AND o_d_id = 1 AND o_c_id = 1 \
                   ORDER BY o_id DESC \
                   LIMIT 1",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::OrderStatus,
            id: "OS_order_lines",
            sql: "SELECT ol_i_id, ol_supply_w_id, ol_quantity, \
                         ol_amount, ol_delivery_d \
                    FROM order_line \
                   WHERE ol_o_id = 3001 AND ol_d_id = 1 AND ol_w_id = 1",
            expected_rows: RowEstimate::Few,
        },

        // ---- Delivery reads -------------------------------------------------
        TprocCQuery {
            transaction: TprocCTransaction::Delivery,
            id: "DEL_oldest_new_order",
            sql: "SELECT no_o_id \
                    FROM new_order \
                   WHERE no_d_id = 1 AND no_w_id = 1 \
                   ORDER BY no_o_id ASC \
                   LIMIT 1",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::Delivery,
            id: "DEL_order_customer",
            sql: "SELECT o_c_id FROM orders \
                   WHERE o_id = 3001 AND o_d_id = 1 AND o_w_id = 1",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::Delivery,
            id: "DEL_order_line_sum",
            sql: "SELECT SUM(ol_amount) AS total \
                    FROM order_line \
                   WHERE ol_o_id = 3001 AND ol_d_id = 1 AND ol_w_id = 1",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::Delivery,
            id: "DEL_pending_by_district",
            sql: "SELECT COUNT(*) AS pending_count, MIN(no_o_id) AS oldest_id \
                    FROM new_order \
                   WHERE no_w_id = 1 \
                   GROUP BY no_d_id \
                   ORDER BY no_d_id",
            expected_rows: RowEstimate::Few,
        },

        // ---- Stock-Level reads ----------------------------------------------
        TprocCQuery {
            transaction: TprocCTransaction::StockLevel,
            id: "SL_district_next_oid",
            sql: "SELECT d_next_o_id FROM district \
                   WHERE d_w_id = 1 AND d_id = 1",
            expected_rows: RowEstimate::Single,
        },
        TprocCQuery {
            transaction: TprocCTransaction::StockLevel,
            id: "SL_low_stock_count",
            sql: "SELECT COUNT(DISTINCT ol.ol_i_id) AS low_stock \
                    FROM order_line AS ol, stock AS s \
                   WHERE ol.ol_w_id = 1 \
                     AND ol.ol_d_id = 1 \
                     AND ol.ol_o_id < 3020 \
                     AND ol.ol_o_id >= 3020 - 20 \
                     AND s.s_w_id = 1 \
                     AND s.s_i_id = ol.ol_i_id \
                     AND s.s_quantity < 15",
            expected_rows: RowEstimate::Single,
        },

        // ---- Analytical overlaps (mixed OLTP+reporting) --------------------
        TprocCQuery {
            transaction: TprocCTransaction::OrderStatus,
            id: "MIXED_district_summary",
            sql: "SELECT d.d_id, d.d_name, \
                         COUNT(o.o_id) AS order_count, \
                         SUM(ol.ol_amount) AS total_revenue \
                    FROM district AS d \
                    JOIN orders AS o ON o.o_w_id = d.d_w_id \
                                    AND o.o_d_id = d.d_id \
                    JOIN order_line AS ol ON ol.ol_w_id = o.o_w_id \
                                         AND ol.ol_d_id = o.o_d_id \
                                         AND ol.ol_o_id = o.o_id \
                   WHERE d.d_w_id = 1 \
                   GROUP BY d.d_id, d.d_name \
                   ORDER BY total_revenue DESC",
            expected_rows: RowEstimate::Few,
        },
        TprocCQuery {
            transaction: TprocCTransaction::Payment,
            id: "MIXED_customer_ranking",
            sql: "SELECT c.c_id, c.c_last, c.c_balance, \
                         RANK() OVER (PARTITION BY c.c_d_id \
                                      ORDER BY c.c_balance DESC) AS balance_rank \
                    FROM customer AS c \
                   WHERE c.c_w_id = 1 \
                   ORDER BY c.c_d_id, balance_rank \
                   LIMIT 100",
            expected_rows: RowEstimate::Many,
        },
    ]
}

/// Return queries grouped by transaction type.
pub fn queries_by_transaction() -> Vec<(TprocCTransaction, Vec<TprocCQuery>)> {
    use TprocCTransaction::*;
    let all = tproc_c_queries();
    vec![
        (NewOrder, all.iter().cloned().filter(|q| q.transaction == NewOrder).collect()),
        (Payment, all.iter().cloned().filter(|q| q.transaction == Payment).collect()),
        (OrderStatus, all.iter().cloned().filter(|q| q.transaction == OrderStatus).collect()),
        (Delivery, all.iter().cloned().filter(|q| q.transaction == Delivery).collect()),
        (StockLevel, all.iter().cloned().filter(|q| q.transaction == StockLevel).collect()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queries_non_empty() {
        assert!(tproc_c_queries().len() >= 10);
    }

    #[test]
    fn test_all_have_select() {
        for q in tproc_c_queries() {
            assert!(q.sql.contains("SELECT"), "{} missing SELECT", q.id);
        }
    }

    #[test]
    fn test_all_transactions_represented() {
        use TprocCTransaction::*;
        let txns: Vec<_> = tproc_c_queries().iter().map(|q| q.transaction).collect();
        for t in [NewOrder, Payment, OrderStatus, Delivery, StockLevel] {
            assert!(txns.contains(&t), "missing transaction type {:?}", t);
        }
    }

    #[test]
    fn test_grouped_by_transaction() {
        let groups = queries_by_transaction();
        assert_eq!(groups.len(), 5);
        for (_, queries) in &groups {
            assert!(!queries.is_empty());
        }
    }
}
