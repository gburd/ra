//! Encode/decode `Message` values as framed wire bytes.
//!
//! Serialization: bincode (compact binary, serde-based).
//! Compression:   zstd, applied per-frame when payload > 4 KiB.

use bytes::{Bytes, BytesMut};

use crate::error::WireError;
use crate::frame::{Frame, FrameHeader, COMPRESSION_THRESHOLD, HEADER_SIZE};
use crate::messages::{Message, MessageType};

/// Encode a `Message` into a framed byte buffer.
///
/// If `compress` is true and the serialized payload exceeds
/// `COMPRESSION_THRESHOLD`, the payload is zstd-compressed and
/// the `COMPRESSED` flag is set.
///
/// # Errors
///
/// Returns `WireError` on serialization or compression failure.
pub fn encode_message(
    msg: &Message,
    request_id: u64,
    compress: bool,
    dst: &mut BytesMut,
) -> Result<(), WireError> {
    let payload_bytes = bincode::serialize(msg)?;
    let msg_type = msg.message_type();

    let (final_payload, compressed) = if compress && payload_bytes.len() > COMPRESSION_THRESHOLD {
        let compressed = zstd::bulk::compress(
            &payload_bytes,
            3, // zstd level 3: good balance
        )
        .map_err(WireError::ZstdCompress)?;
        // Only use compressed version if it's smaller.
        if compressed.len() < payload_bytes.len() {
            (compressed, true)
        } else {
            (payload_bytes, false)
        }
    } else {
        (payload_bytes, false)
    };

    let payload_len: u32 =
        final_payload
            .len()
            .try_into()
            .map_err(|_| WireError::FrameTooLarge {
                size: u32::MAX,
                max: crate::frame::MAX_PAYLOAD_SIZE,
            })?;

    let mut header = FrameHeader::new(msg_type, payload_len, request_id);
    if compressed {
        header = header.with_compressed();
    }

    dst.reserve(HEADER_SIZE + final_payload.len());
    header.encode(dst);
    dst.extend_from_slice(&final_payload);
    Ok(())
}

/// Decode a `Message` from a complete `Frame`.
///
/// Handles zstd decompression when the `COMPRESSED` flag is set.
///
/// # Errors
///
/// Returns `WireError` on decompression or deserialization failure.
pub fn decode_message(frame: &Frame) -> Result<Message, WireError> {
    let payload = if frame.header.is_compressed() {
        zstd::bulk::decompress(&frame.payload, 64 * 1024 * 1024)
            .map_err(WireError::ZstdDecompress)?
    } else {
        frame.payload.to_vec()
    };

    let msg: Message = bincode::deserialize(&payload)?;
    Ok(msg)
}

/// Convenience: encode a `Message` and return owned `Bytes`.
///
/// # Errors
///
/// Returns `WireError` on encoding failure.
pub fn encode_to_bytes(msg: &Message, request_id: u64, compress: bool) -> Result<Bytes, WireError> {
    let mut buf = BytesMut::new();
    encode_message(msg, request_id, compress, &mut buf)?;
    Ok(buf.freeze())
}

/// Decode a `Message` from a byte slice containing exactly one
/// frame (header + payload).
///
/// # Errors
///
/// Returns `WireError` on framing, decompression, or
/// deserialization failure.
pub fn decode_from_bytes(data: &[u8]) -> Result<(Message, u64), WireError> {
    let mut bytes = Bytes::copy_from_slice(data);
    let frame = Frame::decode(&mut bytes)?.ok_or(WireError::IncompleteFrame {
        needed: HEADER_SIZE,
        have: data.len(),
    })?;
    let request_id = frame.header.request_id;
    let msg = decode_message(&frame)?;
    Ok((msg, request_id))
}

/// Validate that the message type in the frame header matches
/// the deserialized message.
///
/// # Errors
///
/// Returns `WireError::FrameIntegrity` on mismatch.
pub fn validate_frame_message(frame: &Frame, msg: &Message) -> Result<(), WireError> {
    let header_type = MessageType::from_code(frame.header.message_type)
        .ok_or(WireError::UnknownMessageType(frame.header.message_type))?;
    let payload_type = msg.message_type();
    if header_type != payload_type {
        return Err(WireError::FrameIntegrity(format!(
            "header says {header_type:?}, payload is {payload_type:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::Capabilities;
    use crate::messages::*;
    use crate::types::*;

    /// Helper: round-trip a message through encode/decode.
    fn round_trip(msg: Message) {
        let request_id = 42;
        let bytes = encode_to_bytes(&msg, request_id, false).expect("encode");
        let (decoded, rid) = decode_from_bytes(&bytes).expect("decode");
        assert_eq!(rid, request_id);
        assert_eq!(msg, decoded);
    }

    /// Helper: round-trip with compression.
    fn round_trip_compressed(msg: Message) {
        let request_id = 99;
        let bytes = encode_to_bytes(&msg, request_id, true).expect("encode");
        let (decoded, rid) = decode_from_bytes(&bytes).expect("decode");
        assert_eq!(rid, request_id);
        assert_eq!(msg, decoded);
    }

    #[test]
    fn handshake_round_trip() {
        round_trip(Message::Handshake(HandshakePayload {
            version: 1,
            capabilities: Capabilities::pooler_defaults(),
            pooler_id: "pooler-1".into(),
            auth_token: vec![1, 2, 3],
        }));
    }

    #[test]
    fn handshake_ack_round_trip() {
        round_trip(Message::HandshakeAck(HandshakeAckPayload {
            version: 1,
            server_id: "backend-1".into(),
            capabilities: Capabilities::backend_defaults(),
            pg_version: "17.2".into(),
        }));
    }

    #[test]
    fn shutdown_round_trip() {
        round_trip(Message::Shutdown(ShutdownPayload {
            reason: "maintenance".into(),
        }));
    }

    #[test]
    fn protocol_error_round_trip() {
        round_trip(Message::ProtocolError(ProtocolErrorPayload {
            code: 0x0001,
            message: "bad frame".into(),
        }));
    }

    #[test]
    fn execute_sql_round_trip() {
        round_trip(Message::ExecuteSql(ExecuteSqlPayload {
            sql: "SELECT * FROM users WHERE id = $1".into(),
            plan_advice: vec!["INDEX_SCAN(users users_pkey)".into()],
            param_types: vec![23], // int4
            params: Some(vec![Some(vec![0, 0, 0, 1])]),
            hints: ExecutionHints::default(),
            table_oids: vec![("users".into(), 16384)],
        }));
    }

    #[test]
    fn execute_raw_sql_round_trip() {
        round_trip(Message::ExecuteRawSql(ExecuteRawSqlPayload {
            sql: "SELECT 1".into(),
            params: None,
        }));
    }

    #[test]
    fn prepare_statement_round_trip() {
        round_trip(Message::PrepareStatement(PrepareStatementPayload {
            name: "stmt1".into(),
            sql: "SELECT $1::int".into(),
            plan_advice: vec![],
            param_types: vec![23],
        }));
    }

    #[test]
    fn bind_parameters_round_trip() {
        round_trip(Message::BindParameters(BindParametersPayload {
            statement_name: "stmt1".into(),
            params: vec![Some(vec![0, 0, 0, 42])],
            param_formats: vec![1],
            result_formats: vec![1],
        }));
    }

    #[test]
    fn execute_prepared_round_trip() {
        round_trip(Message::ExecutePrepared(ExecutePreparedPayload {
            statement_name: "stmt1".into(),
            row_limit: 100,
        }));
    }

    #[test]
    fn close_prepared_round_trip() {
        round_trip(Message::ClosePrepared(ClosePreparedPayload {
            statement_name: "stmt1".into(),
        }));
    }

    #[test]
    fn cursor_round_trips() {
        round_trip(Message::DeclareCursor(DeclareCursorPayload {
            name: "cur1".into(),
            sql: "SELECT * FROM t".into(),
            plan_advice: vec![],
        }));
        round_trip(Message::FetchCursor(FetchCursorPayload {
            name: "cur1".into(),
            count: 100,
        }));
        round_trip(Message::CloseCursor(CloseCursorPayload {
            name: "cur1".into(),
        }));
    }

    #[test]
    fn copy_round_trips() {
        round_trip(Message::CopyIn(CopyInPayload {
            format: CopyFormat::Binary,
            columns: vec!["a".into(), "b".into()],
        }));
        round_trip(Message::CopyData(CopyDataPayload {
            data: vec![1, 2, 3, 4, 5],
        }));
        round_trip(Message::CopyDone);
    }

    #[test]
    fn transaction_round_trips() {
        round_trip(Message::BeginTx(BeginTxPayload {
            isolation: IsolationLevel::Serializable,
            read_only: true,
            deferrable: false,
        }));
        round_trip(Message::CommitTx);
        round_trip(Message::RollbackTx);
        round_trip(Message::Savepoint(SavepointPayload { name: "sp1".into() }));
        round_trip(Message::RollbackTo(RollbackToPayload {
            name: "sp1".into(),
        }));
    }

    #[test]
    fn row_batch_round_trip() {
        round_trip(Message::RowBatch(RowBatchPayload {
            schema: Some(ResultSchema {
                columns: vec![ResultColumn {
                    name: "id".into(),
                    type_oid: 23,
                    type_mod: -1,
                    format_len: 4,
                }],
            }),
            data: RowData::PgBinary(vec![0, 0, 0, 1]),
            row_count: 1,
            sequence: 0,
        }));
    }

    #[test]
    fn row_end_round_trip() {
        round_trip(Message::RowEnd(RowEndPayload {
            rows_affected: 42,
            command_tag: "SELECT 42".into(),
            runtime_stats: Some(RuntimeStats {
                execution_time_us: 1234,
                planning_time_us: 56,
                rows_scanned: 1000,
                shared_hits: 500,
                shared_reads: 10,
            }),
        }));
    }

    #[test]
    fn prepared_ok_round_trip() {
        round_trip(Message::PreparedOk(PreparedOkPayload {
            param_types: vec![23, 25],
            result_columns: vec![ResultColumn {
                name: "count".into(),
                type_oid: 20,
                type_mod: -1,
                format_len: 8,
            }],
        }));
    }

    #[test]
    fn error_response_round_trip() {
        round_trip(Message::ErrorResponse(ErrorResponsePayload {
            sqlstate: "42P01".into(),
            message: "relation does not exist".into(),
            detail: Some("table 'foo' not found".into()),
            hint: Some("check schema".into()),
        }));
    }

    #[test]
    fn notice_response_round_trip() {
        round_trip(Message::NoticeResponse(NoticeResponsePayload {
            severity: NoticeSeverity::Warning,
            message: "deprecated feature".into(),
        }));
    }

    #[test]
    fn notification_msg_round_trip() {
        round_trip(Message::NotificationMsg(NotificationMsgPayload {
            channel: "events".into(),
            payload: "{\"id\":1}".into(),
            pid: 12345,
        }));
    }

    #[test]
    fn facts_snapshot_request_round_trip() {
        round_trip(Message::FactsSnapshotRequest(FactsSnapshotRequestPayload {
            tables: vec!["public.users".into(), "public.orders".into()],
            include: FactsInclude::default(),
        }));
    }

    #[test]
    fn facts_snapshot_response_round_trip() {
        use ra_core::statistics::Statistics;
        round_trip(Message::FactsSnapshotResponse(
            FactsSnapshotResponsePayload {
                timestamp: 1_700_000_000,
                tables: vec![TableFactsBundle {
                    schema: "public".into(),
                    table: "users".into(),
                    table_oid: 16384,
                    stats: Statistics::new(1000.0),
                    foreign_keys: vec![],
                    mvcc: Some(MvccStats {
                        live_tuples: 1000.0,
                        dead_tuples: 50.0,
                        bloat_factor: 1.05,
                        last_vacuum: Some(1_700_000_000),
                        last_analyze: Some(1_700_000_000),
                    }),
                }],
                hardware: Some(HardwareProfileWire {
                    cpu_cores: 8,
                    total_memory_bytes: 16 * 1024 * 1024 * 1024,
                    available_memory_bytes: 12 * 1024 * 1024 * 1024,
                    storage_type: StorageType::NvmeSsd,
                    l1_cache_bytes: 32 * 1024,
                    l2_cache_bytes: 256 * 1024,
                    l3_cache_bytes: 16 * 1024 * 1024,
                }),
                pg_config: Some(PgConfigWire {
                    version_major: 17,
                    shared_buffers_bytes: 4 * 1024 * 1024 * 1024,
                    work_mem_bytes: 64 * 1024 * 1024,
                    effective_cache_size_bytes: 12 * 1024 * 1024 * 1024,
                    random_page_cost: 1.1,
                    seq_page_cost: 1.0,
                    cpu_tuple_cost: 0.01,
                    max_parallel_workers_per_gather: 4,
                    extensions: vec!["pg_stat_statements".into()],
                }),
            },
        ));
    }

    #[test]
    fn facts_list_round_trip() {
        round_trip(Message::FactsListRequest);
        round_trip(Message::FactsListResponse(FactsListResponsePayload {
            categories: vec![FactCategory {
                name: "table_stats".into(),
                description: "Table-level statistics".into(),
                resources: vec!["row_count".into()],
            }],
        }));
    }

    #[test]
    fn facts_query_round_trip() {
        round_trip(Message::FactsQueryRequest(FactsQueryRequestPayload {
            resources: vec![ResourceId("users.row_count".into())],
        }));
        round_trip(Message::FactsQueryResponse(FactsQueryResponsePayload {
            values: vec![FactValue::Float(1000.0)],
        }));
    }

    #[test]
    fn subscribe_invalidations_round_trip() {
        round_trip(Message::SubscribeInvalidations(
            SubscribeInvalidationsPayload {
                granularity: InvalidationGranularity::Table,
                tables: vec!["public.users".into()],
            },
        ));
    }

    #[test]
    fn invalidation_notice_round_trip() {
        round_trip(Message::InvalidationNotice(InvalidationNoticePayload {
            timestamp: 1_700_000_000,
            target: InvalidationTarget::Table {
                schema: "public".into(),
                table: "users".into(),
                oid: 16384,
            },
            cause: InvalidationCause::Analyze,
            changes: vec![ResourceId("users.row_count".into())],
        }));
    }

    #[test]
    fn subscribe_stats_round_trip() {
        round_trip(Message::SubscribeStats(SubscribeStatsPayload {
            interval_ms: 5000,
            metrics: vec![
                StreamingMetric::ActiveQueries,
                StreamingMetric::BufferCacheHitRatio,
            ],
        }));
    }

    #[test]
    fn stats_update_round_trip() {
        round_trip(Message::StatsUpdate(StatsUpdatePayload {
            timestamp: 1_700_000_000,
            metrics: vec![MetricValue {
                metric: StreamingMetric::TransactionRate,
                value: 1500.0,
            }],
        }));
    }

    #[test]
    fn health_check_round_trip() {
        round_trip(Message::HealthCheck(HealthCheckPayload {
            pooler_load: 0.65,
        }));
        round_trip(Message::HealthCheckAck(HealthCheckAckPayload {
            backend_load: 0.4,
            active_queries: 12,
            tx_rate: 350.0,
        }));
    }

    #[test]
    fn capabilities_round_trip() {
        round_trip(Message::CapabilitiesRequest);
        round_trip(Message::CapabilitiesResponse(CapabilitiesResponsePayload {
            pg_version: "17.2".into(),
            extensions: vec!["pg_stat_statements".into()],
            features: vec!["plan_advice".into()],
            installed_indexes: vec!["btree".into()],
        }));
    }

    #[test]
    fn compressed_large_payload() {
        // Create a large-ish message that benefits from
        // compression.
        let big_sql = "SELECT ".to_string() + &"a, ".repeat(2000) + "b FROM t";
        let msg = Message::ExecuteRawSql(ExecuteRawSqlPayload {
            sql: big_sql,
            params: None,
        });
        round_trip_compressed(msg);
    }
}
