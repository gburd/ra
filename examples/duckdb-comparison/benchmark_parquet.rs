//! Benchmark Parquet file queries comparing DuckDB native execution vs Ra optimization.
//!
//! This benchmark focuses on:
//! - Parquet file scanning
//! - Filter pushdown
//! - Column pruning
//! - Predicate pushdown
//! - Partition elimination

use ra_adapters::DuckDBAdapter;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn main() -> anyhow::Result<()> {
    println!("DuckDB Parquet Benchmark - Native vs Ra Optimization\n");
    println!("=".repeat(80));

    let temp_dir = TempDir::new()?;
    let parquet_path = temp_dir.path().join("test_data.parquet");

    let mut adapter = DuckDBAdapter::new();
    adapter.open(":memory:")?;

    create_parquet_file(&adapter, &parquet_path)?;

    println!("\n1. Full Table Scan Queries");
    println!("-".repeat(80));
    benchmark_full_scans(&adapter, &parquet_path)?;

    println!("\n2. Filter Pushdown Queries");
    println!("-".repeat(80));
    benchmark_filter_pushdown(&adapter, &parquet_path)?;

    println!("\n3. Column Pruning Queries");
    println!("-".repeat(80));
    benchmark_column_pruning(&adapter, &parquet_path)?;

    println!("\n4. Aggregation on Parquet");
    println!("-".repeat(80));
    benchmark_aggregations(&adapter, &parquet_path)?;

    println!("\n5. Join with Parquet");
    println!("-".repeat(80));
    benchmark_joins(&adapter, &parquet_path)?;

    println!("\n".repeat(2));
    println!("=".repeat(80));
    println!("Benchmark Complete");

    Ok(())
}

fn create_parquet_file(adapter: &DuckDBAdapter, path: &PathBuf) -> anyhow::Result<()> {
    println!("\nCreating test Parquet file...");

    adapter.execute("CREATE TABLE temp_data (
        id INTEGER,
        timestamp TIMESTAMP,
        sensor_id INTEGER,
        temperature DECIMAL(5,2),
        humidity DECIMAL(5,2),
        pressure DECIMAL(7,2),
        location VARCHAR,
        status VARCHAR
    )")?;

    adapter.execute("INSERT INTO temp_data
        SELECT
            i as id,
            TIMESTAMP '2024-01-01 00:00:00' + INTERVAL (i) MINUTE as timestamp,
            (i % 100) + 1 as sensor_id,
            15.0 + (i % 30) + (RANDOM() * 5) as temperature,
            40.0 + (i % 40) + (RANDOM() * 10) as humidity,
            980.0 + (i % 50) + (RANDOM() * 20) as pressure,
            CASE (i % 5)
                WHEN 0 THEN 'Building_A'
                WHEN 1 THEN 'Building_B'
                WHEN 2 THEN 'Building_C'
                WHEN 3 THEN 'Building_D'
                ELSE 'Building_E'
            END as location,
            CASE (i % 10)
                WHEN 0 THEN 'ERROR'
                WHEN 1 THEN 'WARNING'
                ELSE 'OK'
            END as status
        FROM range(1000000) t(i)
    ")?;

    let path_str = path.to_str().unwrap();
    adapter.execute(&format!("COPY temp_data TO '{path_str}' (FORMAT PARQUET)"))?;
    adapter.execute("DROP TABLE temp_data")?;

    println!("Parquet file created: {}", path.display());

    Ok(())
}

fn benchmark_full_scans(adapter: &DuckDBAdapter, parquet_path: &PathBuf) -> anyhow::Result<()> {
    let path_str = parquet_path.to_str().unwrap();

    let queries = vec![
        (
            "Count all rows",
            format!("SELECT COUNT(*) FROM read_parquet('{path_str}')")
        ),
        (
            "Select all columns",
            format!("SELECT * FROM read_parquet('{path_str}') LIMIT 10000")
        ),
        (
            "Distinct locations",
            format!("SELECT DISTINCT location FROM read_parquet('{path_str}')")
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, &query)?;
    }

    Ok(())
}

fn benchmark_filter_pushdown(adapter: &DuckDBAdapter, parquet_path: &PathBuf) -> anyhow::Result<()> {
    let path_str = parquet_path.to_str().unwrap();

    let queries = vec![
        (
            "Filter by sensor_id",
            format!("SELECT * FROM read_parquet('{path_str}')
                     WHERE sensor_id = 42")
        ),
        (
            "Filter by temperature range",
            format!("SELECT * FROM read_parquet('{path_str}')
                     WHERE temperature > 25.0 AND temperature < 35.0")
        ),
        (
            "Filter by status",
            format!("SELECT * FROM read_parquet('{path_str}')
                     WHERE status = 'ERROR'")
        ),
        (
            "Multiple filters",
            format!("SELECT * FROM read_parquet('{path_str}')
                     WHERE location = 'Building_A'
                       AND temperature > 20.0
                       AND status != 'OK'")
        ),
        (
            "Date range filter",
            format!("SELECT * FROM read_parquet('{path_str}')
                     WHERE timestamp >= TIMESTAMP '2024-01-01 12:00:00'
                       AND timestamp < TIMESTAMP '2024-01-02 00:00:00'")
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, &query)?;
    }

    Ok(())
}

fn benchmark_column_pruning(adapter: &DuckDBAdapter, parquet_path: &PathBuf) -> anyhow::Result<()> {
    let path_str = parquet_path.to_str().unwrap();

    let queries = vec![
        (
            "Select single column",
            format!("SELECT temperature FROM read_parquet('{path_str}')")
        ),
        (
            "Select two columns",
            format!("SELECT sensor_id, temperature FROM read_parquet('{path_str}')")
        ),
        (
            "Select with filter (column pruning)",
            format!("SELECT sensor_id, temperature, status
                     FROM read_parquet('{path_str}')
                     WHERE temperature > 30.0")
        ),
        (
            "Project after filter",
            format!("SELECT timestamp, location
                     FROM read_parquet('{path_str}')
                     WHERE status = 'ERROR'")
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, &query)?;
    }

    Ok(())
}

fn benchmark_aggregations(adapter: &DuckDBAdapter, parquet_path: &PathBuf) -> anyhow::Result<()> {
    let path_str = parquet_path.to_str().unwrap();

    let queries = vec![
        (
            "Aggregate by location",
            format!("SELECT location,
                            COUNT(*) as reading_count,
                            AVG(temperature) as avg_temp,
                            AVG(humidity) as avg_humidity
                     FROM read_parquet('{path_str}')
                     GROUP BY location")
        ),
        (
            "Aggregate by status",
            format!("SELECT status,
                            COUNT(*) as count,
                            MIN(temperature) as min_temp,
                            MAX(temperature) as max_temp
                     FROM read_parquet('{path_str}')
                     GROUP BY status")
        ),
        (
            "Time-based aggregation",
            format!("SELECT DATE_TRUNC('hour', timestamp) as hour,
                            COUNT(*) as readings,
                            AVG(temperature) as avg_temp,
                            AVG(pressure) as avg_pressure
                     FROM read_parquet('{path_str}')
                     GROUP BY hour
                     ORDER BY hour")
        ),
        (
            "Multi-level aggregation",
            format!("SELECT location, status,
                            COUNT(*) as count,
                            AVG(temperature) as avg_temp
                     FROM read_parquet('{path_str}')
                     GROUP BY location, status
                     HAVING COUNT(*) > 1000
                     ORDER BY location, status")
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, &query)?;
    }

    Ok(())
}

fn benchmark_joins(adapter: &DuckDBAdapter, parquet_path: &PathBuf) -> anyhow::Result<()> {
    let path_str = parquet_path.to_str().unwrap();

    adapter.execute("CREATE TABLE sensors (
        sensor_id INTEGER,
        sensor_name VARCHAR,
        sensor_type VARCHAR,
        install_date DATE
    )")?;

    adapter.execute("INSERT INTO sensors
        SELECT
            i as sensor_id,
            'Sensor_' || i as sensor_name,
            CASE (i % 3)
                WHEN 0 THEN 'Temperature'
                WHEN 1 THEN 'Humidity'
                ELSE 'Pressure'
            END as sensor_type,
            DATE '2023-01-01' + INTERVAL (i) DAY as install_date
        FROM range(100) t(i)
    ")?;

    let queries = vec![
        (
            "Join with dimension table",
            format!("SELECT s.sensor_name, s.sensor_type,
                            COUNT(*) as reading_count,
                            AVG(p.temperature) as avg_temp
                     FROM read_parquet('{path_str}') p
                     JOIN sensors s ON p.sensor_id = s.sensor_id
                     GROUP BY s.sensor_name, s.sensor_type")
        ),
        (
            "Filtered join",
            format!("SELECT s.sensor_name, p.location,
                            AVG(p.temperature) as avg_temp
                     FROM read_parquet('{path_str}') p
                     JOIN sensors s ON p.sensor_id = s.sensor_id
                     WHERE s.sensor_type = 'Temperature'
                       AND p.status = 'OK'
                     GROUP BY s.sensor_name, p.location")
        ),
        (
            "Join with aggregated parquet",
            format!("SELECT s.sensor_type,
                            SUM(agg.reading_count) as total_readings,
                            AVG(agg.avg_temp) as overall_avg_temp
                     FROM (
                         SELECT sensor_id,
                                COUNT(*) as reading_count,
                                AVG(temperature) as avg_temp
                         FROM read_parquet('{path_str}')
                         GROUP BY sensor_id
                     ) agg
                     JOIN sensors s ON agg.sensor_id = s.sensor_id
                     GROUP BY s.sensor_type")
        ),
    ];

    for (name, query) in queries {
        println!("\n  {name}");
        run_comparison(adapter, &query)?;
    }

    adapter.execute("DROP TABLE sensors")?;

    Ok(())
}

fn run_comparison(adapter: &DuckDBAdapter, query: &str) -> anyhow::Result<()> {
    let metrics = adapter.compare_execution(query)?;

    println!("    Native: {:>8} μs ({} rows)",
        metrics.native_duration.as_micros(), metrics.row_count);
    println!("    Ra:     {:>8} μs ({} rows)",
        metrics.ra_duration.as_micros(), metrics.row_count);
    println!("    Speedup: {:.2}x {}",
        metrics.speedup,
        if metrics.speedup > 1.0 { "✓" } else { "✗" });

    Ok(())
}
