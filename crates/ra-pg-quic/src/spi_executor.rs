//! Execute SQL via PostgreSQL's SPI (Server Programming Interface)
//! and encode results into wire protocol `RowBatch` / `RowEnd`
//! messages.
//!
//! All SPI calls happen inside `Spi::connect(|client| { ... })`
//! blocks as required by pgrx.

use bytes::BytesMut;
use pgrx::prelude::*;

use ra_wire::messages::{
    Message, RowBatchPayload, RowEndPayload,
};
use ra_wire::types::{ResultColumn, ResultSchema, RowData};

use crate::config::RA_QUIC_BATCH_SIZE;
use crate::error::QuicWorkerError;

/// Result of executing a SQL statement via SPI.
pub struct SpiResult {
    /// Encoded `RowBatch` messages, one per batch of rows.
    pub batches: Vec<Message>,
    /// Final `RowEnd` message with command tag and row count.
    pub row_end: Message,
}

/// Execute a raw SQL query through SPI and return wire protocol
/// messages ready for transmission.
///
/// # Errors
///
/// Returns `QuicWorkerError::Spi` if SPI execution fails.
pub fn execute_sql(sql: &str) -> Result<SpiResult, QuicWorkerError> {
    Spi::connect(|client| execute_sql_inner(&client, sql))
}

fn execute_sql_inner(
    client: &SpiClient<'_>,
    sql: &str,
) -> Result<SpiResult, QuicWorkerError> {
    let result = client
        .select(sql, None, None)
        .map_err(|e| QuicWorkerError::Spi(e.to_string()))?;

    let batch_size = RA_QUIC_BATCH_SIZE.get() as usize;
    let mut batches: Vec<Message> = Vec::new();
    let mut total_rows: u64 = 0;
    let mut sequence: u64 = 0;

    // Build the result schema from the first tuple descriptor.
    let schema = build_result_schema(&result);

    let mut row_buf = BytesMut::with_capacity(4096);
    let mut batch_row_count: u32 = 0;

    for row in result {
        encode_row(&row, &mut row_buf)?;
        batch_row_count += 1;
        total_rows += 1;

        if batch_row_count >= batch_size as u32 {
            batches.push(Message::RowBatch(RowBatchPayload {
                schema: if sequence == 0 {
                    Some(schema.clone())
                } else {
                    None
                },
                data: RowData::PgBinary(row_buf.to_vec()),
                row_count: batch_row_count,
                sequence,
            }));
            row_buf.clear();
            batch_row_count = 0;
            sequence += 1;
        }
    }

    // Flush remaining rows.
    if batch_row_count > 0 {
        batches.push(Message::RowBatch(RowBatchPayload {
            schema: if sequence == 0 {
                Some(schema)
            } else {
                None
            },
            data: RowData::PgBinary(row_buf.to_vec()),
            row_count: batch_row_count,
            sequence,
        }));
    }

    let command_tag = format!("SELECT {total_rows}");

    let row_end = Message::RowEnd(RowEndPayload {
        rows_affected: total_rows,
        command_tag,
        runtime_stats: None,
    });

    Ok(SpiResult { batches, row_end })
}

/// Build a `ResultSchema` from the SPI tuple descriptor.
///
/// For Phase 1, we return a minimal schema. Full type introspection
/// will come in a later phase when we integrate with pg_type catalog
/// lookups.
fn build_result_schema(
    result: &SpiTupleTable,
) -> ResultSchema {
    let mut columns = Vec::new();

    for i in 1..=result.columns() {
        let name = result
            .column_name(i)
            .unwrap_or_else(|_| format!("column_{i}"));

        let type_oid = result
            .column_type_oid(i)
            .map(|oid| oid.as_u32())
            .unwrap_or(0);

        columns.push(ResultColumn {
            name,
            type_oid,
            type_mod: -1,
            format_len: -1,
        });
    }

    ResultSchema { columns }
}

/// Encode a single SPI result row into the binary buffer.
///
/// Uses PostgreSQL binary format: for each column, write a 4-byte
/// length prefix followed by the column data bytes. NULL values
/// are encoded as length = -1 (0xFFFF_FFFF).
fn encode_row(
    row: &SpiHeapTupleData,
    buf: &mut BytesMut,
) -> Result<(), QuicWorkerError> {
    use bytes::BufMut;

    let ncols = row.columns();
    for i in 1..=ncols {
        let datum = row.by_ordinal(i);
        match datum {
            Ok(Some(val)) => {
                // Get the text representation for Phase 1.
                // Full binary encoding will use the type's
                // send function in a future phase.
                let text = val
                    .value::<String>()
                    .unwrap_or_default()
                    .unwrap_or_default();
                let bytes = text.as_bytes();
                buf.put_i32(bytes.len() as i32);
                buf.put_slice(bytes);
            }
            Ok(None) | Err(_) => {
                // NULL: length = -1
                buf.put_i32(-1);
            }
        }
    }
    Ok(())
}
