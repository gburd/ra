//! Shared utility functions for ra-cli.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use ra_parser::{parse_rule_file, RuleFile};

/// Resolve the SQL query from either the positional argument or stdin.
pub fn resolve_query(positional: &str, use_stdin: bool) -> Result<String> {
    if use_stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading SQL from stdin")?;
        let trimmed = buf.trim().to_owned();
        if trimmed.is_empty() {
            bail!(
                "no SQL received on stdin\n\
                 hint: pipe a query, e.g. \
                 echo \"SELECT 1\" | ra-cli explain --stdin"
            );
        }
        Ok(trimmed)
    } else {
        if positional.is_empty() {
            bail!(
                "no SQL query provided\n\
                 hint: pass a query argument or use --stdin"
            );
        }
        Ok(positional.to_owned())
    }
}

/// Load a hardware profile by name.
pub fn load_hardware_profile(name: &str) -> Result<ra_hardware::HardwareProfile> {
    let profile = match name.to_lowercase().as_str() {
        "auto" => ra_hardware::detect_hardware(),
        "cpu-only" => ra_hardware::HardwareProfile::cpu_only(),
        "gpu-server" => ra_hardware::HardwareProfile::gpu_server(),
        "fpga" => ra_hardware::HardwareProfile::fpga_appliance(),
        _ => bail!(
            "unknown hardware profile: {name}. Valid options: auto, cpu-only, gpu-server, fpga"
        ),
    };

    Ok(profile)
}

/// Convert a timeline `HardwareProfileDef` to a `HardwareProfile`.
pub fn hardware_profile_from_def(
    def: &ra_engine::HardwareProfileDef,
) -> ra_hardware::HardwareProfile {
    ra_hardware::HardwareProfile {
        name: def.name.clone(),
        // CPU
        cpu_available: true,
        cpu_cores: def.cpu_cores,
        cpu_memory_bandwidth_gbps: 100.0,
        l2_cache_bytes: def.l2_cache_size,
        l3_cache_bytes: def.l3_cache_size,
        l3_latency_ns: 12.0,
        dram_latency_ns: 80.0,
        simd_width_bits: def.simd_width,
        numa_nodes: 1,
        memory_level_parallelism: 10,
        // GPU
        gpu_available: def.has_gpu,
        gpu_memory_bytes: def.gpu_memory.unwrap_or(0),
        gpu_memory_bandwidth_gbps: if def.has_gpu { 900.0 } else { 0.0 },
        gpu_sm_count: if def.has_gpu { 80 } else { 0 },
        unified_memory_supported: def.has_gpu,
        page_migration_engine_available: def.has_gpu,
        um_page_size_bytes: if def.has_gpu { 65536 } else { 0 },
        um_fault_latency_us: if def.has_gpu { 20.0 } else { 0.0 },
        um_migration_bandwidth_gbps: if def.has_gpu { 12.0 } else { 0.0 },
        chunked_transfer_enabled: def.has_gpu,
        // FPGA
        fpga_available: false,
        fpga_clock_mhz: 0,
        fpga_bram_bytes: 0,
        fpga_max_pipeline_depth: 0,
        fpga_reconfig_ms: 0,
        fpga_near_storage: false,
        fpga_available_luts: 0,
        fpga_regex_engines: 0,
        // Interconnect
        pcie_bandwidth_gbps: if def.has_gpu { 32.0 } else { 0.0 },
        storage_bandwidth_gbps: 3.5,
    }
}

/// Collect all `.rra` files under a path (file or directory).
pub fn collect_rra_files(path: &str) -> Result<Vec<PathBuf>> {
    let p = Path::new(path);
    if p.is_file() {
        return Ok(vec![p.to_path_buf()]);
    }
    if !p.is_dir() {
        bail!("path not found: {path}");
    }
    let mut files = Vec::new();
    walk_dir(p, &mut files)?;
    files.sort();
    Ok(files)
}

/// Recursively walk a directory for `.rra` files.
fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rra") {
            out.push(path);
        }
    }
    Ok(())
}

/// Search for a rule by ID across a set of files.
pub fn find_rule_by_id(rule_id: &str, files: &[PathBuf]) -> Option<(RuleFile, PathBuf)> {
    for file in files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        if let Ok(rule) = parse_rule_file(&source) {
            if rule.metadata.id == rule_id {
                return Some((rule, file.clone()));
            }
        }
    }
    None
}

/// Build a [`ResourceBudget`] from CLI flags.
pub fn build_resource_budget(
    profile: Option<&str>,
    max_time: Option<&str>,
    max_memory: Option<&str>,
    max_iterations: Option<usize>,
    overflow_strategy: Option<&str>,
    rule_tracking_requested: bool,
) -> Result<Option<ra_engine::ResourceBudget>> {
    let has_custom = max_time.is_some()
        || max_memory.is_some()
        || max_iterations.is_some()
        || overflow_strategy.is_some();

    if profile.is_none() && !has_custom {
        if rule_tracking_requested {
            return Ok(Some(ra_engine::ResourceBudget::standard()));
        }
        return Ok(None);
    }

    let mut budget = match profile {
        Some("interactive") => ra_engine::ResourceBudget::interactive(),
        Some("standard") => ra_engine::ResourceBudget::standard(),
        Some("batch") => ra_engine::ResourceBudget::batch(),
        Some("memory-constrained") => ra_engine::ResourceBudget::memory_constrained(),
        Some("unlimited") => ra_engine::ResourceBudget::unlimited(),
        Some(other) => bail!(
            "unknown resource budget profile: '{other}'. \
             Valid: interactive, standard, batch, \
             memory-constrained, unlimited"
        ),
        None if rule_tracking_requested => ra_engine::ResourceBudget::standard(),
        None => ra_engine::ResourceBudget::unlimited(),
    };

    if let Some(t) = max_time {
        budget = budget.with_time_limit(parse_duration(t)?);
    }
    if let Some(m) = max_memory {
        budget = budget.with_memory_limit(parse_byte_size(m)?);
    }
    if let Some(n) = max_iterations {
        budget = budget.with_iteration_limit(n);
    }
    if let Some(s) = overflow_strategy {
        budget = budget.with_overflow_strategy(parse_overflow(s)?);
    }

    Ok(Some(budget))
}

/// Parse a human-readable duration string (e.g. "100ms", "1s", "10s").
pub fn parse_duration(s: &str) -> Result<std::time::Duration> {
    let s = s.trim();
    if let Some(ms) = s.strip_suffix("ms") {
        let n: u64 = ms.trim().parse().context("invalid millisecond value")?;
        return Ok(std::time::Duration::from_millis(n));
    }
    if let Some(secs) = s.strip_suffix('s') {
        let n: u64 = secs.trim().parse().context("invalid seconds value")?;
        return Ok(std::time::Duration::from_secs(n));
    }
    let n: u64 = s
        .parse()
        .context("invalid duration; use e.g. '100ms' or '1s'")?;
    Ok(std::time::Duration::from_secs(n))
}

/// Parse a human-readable byte size (e.g. "10MB", "500MB", "2GB").
pub fn parse_byte_size(s: &str) -> Result<u64> {
    let s = s.trim();
    let upper = s.to_uppercase();
    if let Some(gb) = upper.strip_suffix("GB") {
        let n: u64 = gb.trim().parse().context("invalid GB value")?;
        return Ok(n.saturating_mul(1024 * 1024 * 1024));
    }
    if let Some(mb) = upper.strip_suffix("MB") {
        let n: u64 = mb.trim().parse().context("invalid MB value")?;
        return Ok(n.saturating_mul(1024 * 1024));
    }
    if let Some(kb) = upper.strip_suffix("KB") {
        let n: u64 = kb.trim().parse().context("invalid KB value")?;
        return Ok(n.saturating_mul(1024));
    }
    s.parse::<u64>()
        .context("invalid byte size; use e.g. '10MB', '2GB', or raw bytes")
}

/// Parse an overflow strategy string.
pub fn parse_overflow(s: &str) -> Result<ra_engine::OverflowStrategy> {
    match s.to_lowercase().as_str() {
        "best-so-far" | "best" => Ok(ra_engine::OverflowStrategy::ReturnBestSoFar),
        "original" => Ok(ra_engine::OverflowStrategy::ReturnOriginal),
        "fail" => Ok(ra_engine::OverflowStrategy::Fail),
        _ => bail!(
            "unknown overflow strategy: '{s}'. \
             Valid: best-so-far, original, fail"
        ),
    }
}

/// Parse a dialect name string into a `Dialect` enum.
pub fn parse_dialect(name: &str) -> Result<ra_dialect::Dialect> {
    match name.to_lowercase().as_str() {
        "postgresql" | "postgres" | "pg" => Ok(ra_dialect::Dialect::PostgreSql),
        "mysql" => Ok(ra_dialect::Dialect::MySql),
        "sqlite" => Ok(ra_dialect::Dialect::Sqlite),
        "duckdb" => Ok(ra_dialect::Dialect::DuckDb),
        "mssql" | "mssqlserver" | "sqlserver" => Ok(ra_dialect::Dialect::MsSql),
        "oracle" => Ok(ra_dialect::Dialect::Oracle),
        other => bail!(
            "unknown dialect: '{other}'. Valid: postgresql, \
             mysql, sqlite, duckdb, mssql, oracle"
        ),
    }
}

/// Load schema from database URL or JSON file for analysis commands.
pub fn load_schema_for_analysis(
    database_url: Option<&str>,
    schema_path: Option<&str>,
) -> Result<ra_metadata::SchemaInfo> {
    if let Some(url) = database_url {
        let mut connector =
            ra_metadata::connect(url).with_context(|| format!("connecting to database: {url}"))?;
        let schema = connector
            .gather_schema()
            .with_context(|| "gathering schema metadata from database")?;
        return Ok(schema);
    }

    if let Some(path) = schema_path {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("reading schema file: {path}"))?;
        let schema: ra_metadata::SchemaInfo = serde_json::from_str(&contents)
            .with_context(|| format!("parsing schema JSON: {path}"))?;
        return Ok(schema);
    }

    bail!(
        "must provide either --database-url or --schema \
         for trigger analysis"
    );
}
