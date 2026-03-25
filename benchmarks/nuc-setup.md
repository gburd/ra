# nuc Server: IMDB Benchmark Setup

Setup guide for running JOB (Join Order Benchmark) queries against the
IMDB dataset on the nuc FreeBSD server.

## Server Access

```bash
ssh nuc
```

Requires SSH key authentication configured in `~/.ssh/config`.

- **OS**: FreeBSD
- **Hostname**: nuc (alias for gmktec-k9)

## PostgreSQL

| Property       | Value                                    |
|----------------|------------------------------------------|
| Binary path    | `~/ws/postgres/build/bin/psql`           |
| Data directory | `~/pgdata/imdb`                          |
| Database name  | `imdb`                                   |
| Tables         | 21 (IMDB May 2013 snapshot)              |
| Dataset size   | ~2.1 GB on disk                          |
| Total rows     | ~60M across all tables                   |

### Verify PostgreSQL is Running

```bash
~/ws/postgres/build/bin/psql -d imdb -c "SELECT 1"
```

If PostgreSQL is not running, start it:

```bash
~/ws/postgres/build/bin/pg_ctl -D ~/pgdata/imdb -l ~/pgdata/imdb/logfile start
```

### Verify Tables

```bash
~/ws/postgres/build/bin/psql -d imdb -c '\dt+'
```

All 21 tables should be present:

```
aka_name, aka_title, cast_info, char_name, comp_cast_type,
company_name, company_type, complete_cast, info_type, keyword,
kind_type, link_type, movie_companies, movie_info, movie_info_idx,
movie_keyword, movie_link, name, person_info, role_type, title
```

### Expected Row Counts

| Table             | Expected Rows |
|-------------------|---------------|
| aka_name          |       901,343 |
| aka_title         |       361,472 |
| cast_info         |    36,244,344 |
| char_name         |     3,140,339 |
| comp_cast_type    |             4 |
| company_name      |       234,997 |
| company_type      |             4 |
| complete_cast     |       135,086 |
| info_type         |           113 |
| keyword           |       134,170 |
| kind_type         |             7 |
| link_type         |            18 |
| movie_companies   |     2,609,129 |
| movie_info        |    14,835,720 |
| movie_info_idx    |     1,380,035 |
| movie_keyword     |     4,523,930 |
| movie_link        |        29,997 |
| name              |     4,167,491 |
| person_info       |     2,963,664 |
| role_type         |            12 |
| title             |     2,528,312 |

Validate with:

```bash
~/ws/postgres/build/bin/psql -d imdb -c "
  SELECT relname AS table_name,
         n_live_tup AS row_estimate
  FROM pg_stat_user_tables
  ORDER BY relname;
"
```

## JOB Queries

### Transfer queries to nuc

From the repo root on the local machine:

```bash
scp -r benchmarks/job/queries/ nuc:~/job-queries/
```

Or clone the repo on nuc and reference them directly.

### Run a single query

```bash
~/ws/postgres/build/bin/psql -d imdb -f ~/job-queries/1a.sql
```

### Run with EXPLAIN ANALYZE

```bash
~/ws/postgres/build/bin/psql -d imdb -c \
  "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) $(cat ~/job-queries/1a.sql)"
```

### Run a sample set

```bash
cd ~/job-queries
for q in 1a.sql 5a.sql 10a.sql 17a.sql 25a.sql; do
  echo "=== $q ==="
  ~/ws/postgres/build/bin/psql -d imdb -f "$q"
done
```

### Run all 113 queries with timing

Use the comprehensive benchmark script:

```bash
# Copy the script to nuc
scp benchmarks/job/run_nuc_benchmark.sh nuc:~/run_nuc_benchmark.sh

# Run on nuc
ssh nuc "chmod +x ~/run_nuc_benchmark.sh && ~/run_nuc_benchmark.sh"
```

Or use the existing `run_job_comparison.sh` after setting up psql on PATH:

```bash
ssh nuc "export PATH=~/ws/postgres/build/bin:\$PATH && cd /path/to/ra && benchmarks/job/run_job_comparison.sh imdb"
```

## Query Complexity Tiers

| Tier     | Templates | Tables/Query | Example Queries           |
|----------|-----------|--------------|---------------------------|
| Baseline | 1-8       | 3-5          | 1a, 2a, 3a, 5a, 8a       |
| Moderate | 9-16      | 5-8          | 10a, 13a, 15a             |
| Hard     | 17-25     | 8-10         | 17a, 20a, 25a             |
| Stress   | 26-33     | 10-15        | 26a, 29a, 33a             |

## Troubleshooting

### PostgreSQL not starting

Check the log:

```bash
cat ~/pgdata/imdb/logfile
```

Common issues:
- Port conflict: check `~/pgdata/imdb/postgresql.conf` for `port` setting
- Shared memory: FreeBSD may need `sysctl kern.ipc.shmmax` increased

### Data load incomplete

Check if any tables are empty or have 0 rows:

```bash
~/ws/postgres/build/bin/psql -d imdb -c "
  SELECT relname, n_live_tup
  FROM pg_stat_user_tables
  WHERE n_live_tup = 0
  ORDER BY relname;
"
```

If tables are missing data, re-run the load for specific tables or
re-run `ANALYZE`:

```bash
~/ws/postgres/build/bin/psql -d imdb -c "ANALYZE"
```

### Connection refused

Ensure PostgreSQL accepts local connections. Check:

```bash
cat ~/pgdata/imdb/pg_hba.conf
```

Should include a line like:

```
local   all   all   trust
```
