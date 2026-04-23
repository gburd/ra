//! All wire protocol message types and their payloads.
//!
//! Message codes are grouped by function:
//! - `0x0000..0x00FF` — Protocol management
//! - `0x0100..0x01FF` — Data plane
//! - `0x0200..0x02FF` — Control plane

use serde::{Deserialize, Serialize};

use crate::capabilities::Capabilities;
use crate::types::{
    CopyFormat, ExecutionHints, FactCategory, FactValue, FactsInclude, HardwareProfileWire,
    InvalidationCause, InvalidationGranularity, InvalidationTarget, IsolationLevel, MetricValue,
    NoticeSeverity, PgConfigWire, ResourceId, ResultColumn, ResultSchema, RowData, RuntimeStats,
    StreamingMetric, TableFactsBundle,
};

// ── Message Type Codes ──────────────────────────────────────

/// Enumeration of every message type in the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum MessageType {
    // Protocol management
    Handshake = 0x0001,
    HandshakeAck = 0x0002,
    Shutdown = 0x0003,
    ProtocolError = 0x00FF,

    // Data plane — pooler to backend
    ExecuteSql = 0x0100,
    ExecuteRawSql = 0x0101,
    PrepareStatement = 0x0102,
    BindParameters = 0x0103,
    ExecutePrepared = 0x0104,
    ClosePrepared = 0x0105,
    DeclareCursor = 0x0106,
    FetchCursor = 0x0107,
    CloseCursor = 0x0108,
    CopyIn = 0x0109,
    CopyData = 0x010A,
    CopyDone = 0x010B,
    BeginTx = 0x0110,
    CommitTx = 0x0111,
    RollbackTx = 0x0112,
    Savepoint = 0x0113,
    RollbackTo = 0x0114,

    // Data plane — backend to pooler
    RowBatch = 0x0180,
    RowEnd = 0x0181,
    PreparedOk = 0x0182,
    ErrorResponse = 0x01F0,
    NoticeResponse = 0x01F1,
    NotificationMsg = 0x0186,

    // Control plane
    FactsSnapshotRequest = 0x0200,
    FactsSnapshotResponse = 0x0201,
    FactsListRequest = 0x0202,
    FactsListResponse = 0x0203,
    FactsQueryRequest = 0x0204,
    FactsQueryResponse = 0x0205,
    SubscribeInvalidations = 0x0206,
    InvalidationNotice = 0x0207,
    SubscribeStats = 0x0208,
    StatsUpdate = 0x0209,
    HealthCheck = 0x020A,
    HealthCheckAck = 0x020B,
    CapabilitiesRequest = 0x020C,
    CapabilitiesResponse = 0x020D,
}

impl MessageType {
    /// Numeric wire code for this message type.
    #[must_use]
    pub fn code(self) -> u16 {
        self as u16
    }

    /// Try to convert a u16 wire code to a `MessageType`.
    #[must_use]
    pub fn from_code(code: u16) -> Option<Self> {
        match code {
            0x0001 => Some(Self::Handshake),
            0x0002 => Some(Self::HandshakeAck),
            0x0003 => Some(Self::Shutdown),
            0x00FF => Some(Self::ProtocolError),
            0x0100 => Some(Self::ExecuteSql),
            0x0101 => Some(Self::ExecuteRawSql),
            0x0102 => Some(Self::PrepareStatement),
            0x0103 => Some(Self::BindParameters),
            0x0104 => Some(Self::ExecutePrepared),
            0x0105 => Some(Self::ClosePrepared),
            0x0106 => Some(Self::DeclareCursor),
            0x0107 => Some(Self::FetchCursor),
            0x0108 => Some(Self::CloseCursor),
            0x0109 => Some(Self::CopyIn),
            0x010A => Some(Self::CopyData),
            0x010B => Some(Self::CopyDone),
            0x0110 => Some(Self::BeginTx),
            0x0111 => Some(Self::CommitTx),
            0x0112 => Some(Self::RollbackTx),
            0x0113 => Some(Self::Savepoint),
            0x0114 => Some(Self::RollbackTo),
            0x0180 => Some(Self::RowBatch),
            0x0181 => Some(Self::RowEnd),
            0x0182 => Some(Self::PreparedOk),
            0x01F0 => Some(Self::ErrorResponse),
            0x01F1 => Some(Self::NoticeResponse),
            0x0186 => Some(Self::NotificationMsg),
            0x0200 => Some(Self::FactsSnapshotRequest),
            0x0201 => Some(Self::FactsSnapshotResponse),
            0x0202 => Some(Self::FactsListRequest),
            0x0203 => Some(Self::FactsListResponse),
            0x0204 => Some(Self::FactsQueryRequest),
            0x0205 => Some(Self::FactsQueryResponse),
            0x0206 => Some(Self::SubscribeInvalidations),
            0x0207 => Some(Self::InvalidationNotice),
            0x0208 => Some(Self::SubscribeStats),
            0x0209 => Some(Self::StatsUpdate),
            0x020A => Some(Self::HealthCheck),
            0x020B => Some(Self::HealthCheckAck),
            0x020C => Some(Self::CapabilitiesRequest),
            0x020D => Some(Self::CapabilitiesResponse),
            _ => None,
        }
    }
}

// ── Message Enum ────────────────────────────────────────────

/// Top-level enum carrying any protocol message and its payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Message {
    // Protocol management
    Handshake(HandshakePayload),
    HandshakeAck(HandshakeAckPayload),
    Shutdown(ShutdownPayload),
    ProtocolError(ProtocolErrorPayload),

    // Data plane — requests
    ExecuteSql(ExecuteSqlPayload),
    ExecuteRawSql(ExecuteRawSqlPayload),
    PrepareStatement(PrepareStatementPayload),
    BindParameters(BindParametersPayload),
    ExecutePrepared(ExecutePreparedPayload),
    ClosePrepared(ClosePreparedPayload),
    DeclareCursor(DeclareCursorPayload),
    FetchCursor(FetchCursorPayload),
    CloseCursor(CloseCursorPayload),
    CopyIn(CopyInPayload),
    CopyData(CopyDataPayload),
    CopyDone,
    BeginTx(BeginTxPayload),
    CommitTx,
    RollbackTx,
    Savepoint(SavepointPayload),
    RollbackTo(RollbackToPayload),

    // Data plane — responses
    RowBatch(RowBatchPayload),
    RowEnd(RowEndPayload),
    PreparedOk(PreparedOkPayload),
    ErrorResponse(ErrorResponsePayload),
    NoticeResponse(NoticeResponsePayload),
    NotificationMsg(NotificationMsgPayload),

    // Control plane
    FactsSnapshotRequest(FactsSnapshotRequestPayload),
    FactsSnapshotResponse(FactsSnapshotResponsePayload),
    FactsListRequest,
    FactsListResponse(FactsListResponsePayload),
    FactsQueryRequest(FactsQueryRequestPayload),
    FactsQueryResponse(FactsQueryResponsePayload),
    SubscribeInvalidations(SubscribeInvalidationsPayload),
    InvalidationNotice(InvalidationNoticePayload),
    SubscribeStats(SubscribeStatsPayload),
    StatsUpdate(StatsUpdatePayload),
    HealthCheck(HealthCheckPayload),
    HealthCheckAck(HealthCheckAckPayload),
    CapabilitiesRequest,
    CapabilitiesResponse(CapabilitiesResponsePayload),
}

impl Message {
    /// Return the `MessageType` for this message variant.
    #[must_use]
    pub fn message_type(&self) -> MessageType {
        match self {
            Self::Handshake(_) => MessageType::Handshake,
            Self::HandshakeAck(_) => MessageType::HandshakeAck,
            Self::Shutdown(_) => MessageType::Shutdown,
            Self::ProtocolError(_) => MessageType::ProtocolError,
            Self::ExecuteSql(_) => MessageType::ExecuteSql,
            Self::ExecuteRawSql(_) => MessageType::ExecuteRawSql,
            Self::PrepareStatement(_) => MessageType::PrepareStatement,
            Self::BindParameters(_) => MessageType::BindParameters,
            Self::ExecutePrepared(_) => MessageType::ExecutePrepared,
            Self::ClosePrepared(_) => MessageType::ClosePrepared,
            Self::DeclareCursor(_) => MessageType::DeclareCursor,
            Self::FetchCursor(_) => MessageType::FetchCursor,
            Self::CloseCursor(_) => MessageType::CloseCursor,
            Self::CopyIn(_) => MessageType::CopyIn,
            Self::CopyData(_) => MessageType::CopyData,
            Self::CopyDone => MessageType::CopyDone,
            Self::BeginTx(_) => MessageType::BeginTx,
            Self::CommitTx => MessageType::CommitTx,
            Self::RollbackTx => MessageType::RollbackTx,
            Self::Savepoint(_) => MessageType::Savepoint,
            Self::RollbackTo(_) => MessageType::RollbackTo,
            Self::RowBatch(_) => MessageType::RowBatch,
            Self::RowEnd(_) => MessageType::RowEnd,
            Self::PreparedOk(_) => MessageType::PreparedOk,
            Self::ErrorResponse(_) => MessageType::ErrorResponse,
            Self::NoticeResponse(_) => MessageType::NoticeResponse,
            Self::NotificationMsg(_) => MessageType::NotificationMsg,
            Self::FactsSnapshotRequest(_) => MessageType::FactsSnapshotRequest,
            Self::FactsSnapshotResponse(_) => MessageType::FactsSnapshotResponse,
            Self::FactsListRequest => MessageType::FactsListRequest,
            Self::FactsListResponse(_) => MessageType::FactsListResponse,
            Self::FactsQueryRequest(_) => MessageType::FactsQueryRequest,
            Self::FactsQueryResponse(_) => MessageType::FactsQueryResponse,
            Self::SubscribeInvalidations(_) => MessageType::SubscribeInvalidations,
            Self::InvalidationNotice(_) => MessageType::InvalidationNotice,
            Self::SubscribeStats(_) => MessageType::SubscribeStats,
            Self::StatsUpdate(_) => MessageType::StatsUpdate,
            Self::HealthCheck(_) => MessageType::HealthCheck,
            Self::HealthCheckAck(_) => MessageType::HealthCheckAck,
            Self::CapabilitiesRequest => MessageType::CapabilitiesRequest,
            Self::CapabilitiesResponse(_) => MessageType::CapabilitiesResponse,
        }
    }
}

// ── Protocol Management Payloads ────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandshakePayload {
    pub version: u8,
    pub capabilities: Capabilities,
    pub pooler_id: String,
    pub auth_token: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandshakeAckPayload {
    pub version: u8,
    pub server_id: String,
    pub capabilities: Capabilities,
    pub pg_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShutdownPayload {
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProtocolErrorPayload {
    pub code: u32,
    pub message: String,
}

// ── Data Plane Request Payloads ─────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecuteSqlPayload {
    pub sql: String,
    pub plan_advice: Vec<String>,
    pub param_types: Vec<u32>,
    pub params: Option<Vec<Option<Vec<u8>>>>,
    pub hints: ExecutionHints,
    pub table_oids: Vec<(String, u32)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecuteRawSqlPayload {
    pub sql: String,
    pub params: Option<Vec<Option<Vec<u8>>>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrepareStatementPayload {
    pub name: String,
    pub sql: String,
    pub plan_advice: Vec<String>,
    pub param_types: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindParametersPayload {
    pub statement_name: String,
    pub params: Vec<Option<Vec<u8>>>,
    pub param_formats: Vec<i16>,
    pub result_formats: Vec<i16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutePreparedPayload {
    pub statement_name: String,
    pub row_limit: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClosePreparedPayload {
    pub statement_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeclareCursorPayload {
    pub name: String,
    pub sql: String,
    pub plan_advice: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FetchCursorPayload {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloseCursorPayload {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopyInPayload {
    pub format: CopyFormat,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopyDataPayload {
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BeginTxPayload {
    pub isolation: IsolationLevel,
    pub read_only: bool,
    pub deferrable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavepointPayload {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RollbackToPayload {
    pub name: String,
}

// ── Data Plane Response Payloads ────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RowBatchPayload {
    pub schema: Option<ResultSchema>,
    pub data: RowData,
    pub row_count: u32,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RowEndPayload {
    pub rows_affected: u64,
    pub command_tag: String,
    pub runtime_stats: Option<RuntimeStats>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedOkPayload {
    pub param_types: Vec<u32>,
    pub result_columns: Vec<ResultColumn>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponsePayload {
    pub sqlstate: String,
    pub message: String,
    pub detail: Option<String>,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoticeResponsePayload {
    pub severity: NoticeSeverity,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationMsgPayload {
    pub channel: String,
    pub payload: String,
    pub pid: u32,
}

// ── Control Plane Payloads ──────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactsSnapshotRequestPayload {
    pub tables: Vec<String>,
    pub include: FactsInclude,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactsSnapshotResponsePayload {
    pub timestamp: u64,
    pub tables: Vec<TableFactsBundle>,
    pub hardware: Option<HardwareProfileWire>,
    pub pg_config: Option<PgConfigWire>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactsListResponsePayload {
    pub categories: Vec<FactCategory>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactsQueryRequestPayload {
    pub resources: Vec<ResourceId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactsQueryResponsePayload {
    pub values: Vec<FactValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscribeInvalidationsPayload {
    pub granularity: InvalidationGranularity,
    pub tables: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvalidationNoticePayload {
    pub timestamp: u64,
    pub target: InvalidationTarget,
    pub cause: InvalidationCause,
    pub changes: Vec<ResourceId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubscribeStatsPayload {
    pub interval_ms: u32,
    pub metrics: Vec<StreamingMetric>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatsUpdatePayload {
    pub timestamp: u64,
    pub metrics: Vec<MetricValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthCheckPayload {
    pub pooler_load: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthCheckAckPayload {
    pub backend_load: f64,
    pub active_queries: u32,
    pub tx_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilitiesResponsePayload {
    pub pg_version: String,
    pub extensions: Vec<String>,
    pub features: Vec<String>,
    pub installed_indexes: Vec<String>,
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_type_round_trip() {
        let all_types = [
            MessageType::Handshake,
            MessageType::HandshakeAck,
            MessageType::Shutdown,
            MessageType::ProtocolError,
            MessageType::ExecuteSql,
            MessageType::ExecuteRawSql,
            MessageType::PrepareStatement,
            MessageType::BindParameters,
            MessageType::ExecutePrepared,
            MessageType::ClosePrepared,
            MessageType::DeclareCursor,
            MessageType::FetchCursor,
            MessageType::CloseCursor,
            MessageType::CopyIn,
            MessageType::CopyData,
            MessageType::CopyDone,
            MessageType::BeginTx,
            MessageType::CommitTx,
            MessageType::RollbackTx,
            MessageType::Savepoint,
            MessageType::RollbackTo,
            MessageType::RowBatch,
            MessageType::RowEnd,
            MessageType::PreparedOk,
            MessageType::ErrorResponse,
            MessageType::NoticeResponse,
            MessageType::NotificationMsg,
            MessageType::FactsSnapshotRequest,
            MessageType::FactsSnapshotResponse,
            MessageType::FactsListRequest,
            MessageType::FactsListResponse,
            MessageType::FactsQueryRequest,
            MessageType::FactsQueryResponse,
            MessageType::SubscribeInvalidations,
            MessageType::InvalidationNotice,
            MessageType::SubscribeStats,
            MessageType::StatsUpdate,
            MessageType::HealthCheck,
            MessageType::HealthCheckAck,
            MessageType::CapabilitiesRequest,
            MessageType::CapabilitiesResponse,
        ];
        for mt in all_types {
            let code = mt.code();
            let decoded =
                MessageType::from_code(code).unwrap_or_else(|| panic!("unknown code {code:#06x}"));
            assert_eq!(mt, decoded);
        }
    }

    #[test]
    fn unknown_code_returns_none() {
        assert!(MessageType::from_code(0xFFFF).is_none());
    }
}
