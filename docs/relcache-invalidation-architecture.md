# Relcache Invalidation Architecture

## System Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                        PostgreSQL Core                              │
│                                                                     │
│  ┌───────────────┐                                                 │
│  │  DDL Command  │                                                 │
│  │  (ALTER TABLE)│                                                 │
│  └───────┬───────┘                                                 │
│          │                                                         │
│          v                                                         │
│  ┌──────────────────┐                                             │
│  │   Apply Schema   │                                             │
│  │   Change to      │                                             │
│  │   pg_class       │                                             │
│  └──────────────────┘                                             │
│          │                                                         │
│          v                                                         │
│  ┌──────────────────────┐                                         │
│  │  Invalidate Relcache │                                         │
│  │  Entry (inval.c)     │                                         │
│  └──────────────────────┘                                         │
│          │                                                         │
│          v                                                         │
│  ┌──────────────────────────────────────┐                         │
│  │  Call Registered Callbacks           │                         │
│  │  CacheRegisterRelcacheCallback()     │                         │
│  └──────────────────────────────────────┘                         │
└─────────────────┼─────────────────────────────────────────────────┘
                  │
                  │ (Callback invocation with relid)
                  │
                  v
┌─────────────────────────────────────────────────────────────────────┐
│                    Ra Extension (C FFI Layer)                       │
│                                                                     │
│  ┌───────────────────────────────────────────┐                    │
│  │  ra_relcache_callback(arg, relid)         │                    │
│  │  - Receives OID of invalidated relation   │                    │
│  │  - Forwards to Rust implementation        │                    │
│  └───────────────────────────────────────────┘                    │
└─────────────────┼─────────────────────────────────────────────────┘
                  │
                  │ (FFI call)
                  │
                  v
┌─────────────────────────────────────────────────────────────────────┐
│               Ra Extension (Rust Implementation)                    │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐   │
│  │  MetadataCache (Global Mutex-protected)                    │   │
│  │                                                            │   │
│  │  ┌──────────────────────────────────────────────────────┐ │   │
│  │  │  tables: HashMap<Oid, CachedTableMetadata>          │ │   │
│  │  │  - OID → (Statistics, refresh_time, is_valid)       │ │   │
│  │  └──────────────────────────────────────────────────────┘ │   │
│  │                                                            │   │
│  │  ┌──────────────────────────────────────────────────────┐ │   │
│  │  │  invalidated: HashSet<Oid>                          │ │   │
│  │  │  - OIDs pending refresh                             │ │   │
│  │  └──────────────────────────────────────────────────────┘ │   │
│  │                                                            │   │
│  │  ┌──────────────────────────────────────────────────────┐ │   │
│  │  │  Metrics: hits, misses, invalidations, hit_rate     │ │   │
│  │  └──────────────────────────────────────────────────────┘ │   │
│  │                                                            │   │
│  │  Methods:                                                  │   │
│  │  - invalidate(oid) → mark entry as stale                  │   │
│  │  - get_or_refresh(oid) → fetch from cache or refresh      │   │
│  │  - refresh(oid) → query pg_catalog, update cache          │   │
│  │  - clear() → remove all entries                            │   │
│  └────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐   │
│  │  ra_rust_invalidate_table(oid)                            │   │
│  │  1. Lock cache                                             │   │
│  │  2. Mark entry.is_valid = false                            │   │
│  │  3. Add OID to invalidated set                             │   │
│  │  4. Increment invalidations counter                        │   │
│  │  5. Log event (if debug logging enabled)                   │   │
│  └────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
                  │
                  │ (Next query on invalidated table)
                  │
                  v
┌─────────────────────────────────────────────────────────────────────┐
│                      Query Planning Path                            │
│                                                                     │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │  Planner Hook: ra_optimize_query()                          │ │
│  │  1. Extract table OIDs from parse tree                      │ │
│  │  2. For each OID:                                            │ │
│  │     metadata_cache::get_table_metadata(oid)                 │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                          │                                          │
│                          v                                          │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │  MetadataCache::get_or_refresh(oid)                         │ │
│  │  if is_valid(oid):                                           │ │
│  │    ✅ Cache Hit                                              │ │
│  │    return cached Statistics                                  │ │
│  │  else:                                                        │ │
│  │    ❌ Cache Miss                                             │ │
│  │    refresh(oid)                                              │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                          │                                          │
│                          v                                          │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │  MetadataCache::refresh(oid)                                │ │
│  │  1. Call stats_bridge::gather_table_stats_by_oid(oid)       │ │
│  │  2. Query pg_class (reltuples, relpages)                    │ │
│  │  3. Query pg_statistic (column stats)                       │ │
│  │  4. Query pg_index (indexes)                                │ │
│  │  5. Build Statistics struct                                 │ │
│  │  6. Update cache: tables.insert(oid, entry)                 │ │
│  │  7. Remove from invalidated set                             │ │
│  │  8. Increment misses counter                                │ │
│  │  9. Log refresh event (if debug logging)                    │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                          │                                          │
│                          v                                          │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │  Query Optimization with Fresh Metadata                     │ │
│  │  - Use current row counts                                    │ │
│  │  - Use current column statistics                            │ │
│  │  - Use current index list                                   │ │
│  │  - Generate optimal plan                                     │ │
│  └──────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

## Sequence Diagram: DDL Triggers Invalidation

```
User          PostgreSQL      Ra Extension       MetadataCache       stats_bridge
 │                │                 │                   │                   │
 │ ALTER TABLE    │                 │                   │                   │
 │───────────────>│                 │                   │                   │
 │                │                 │                   │                   │
 │                │ Update pg_class │                   │                   │
 │                │─────────────────>                   │                   │
 │                │                 │                   │                   │
 │                │ Invalidate      │                   │                   │
 │                │   relcache      │                   │                   │
 │                │─────────────────>                   │                   │
 │                │                 │                   │                   │
 │                │ ra_relcache_    │                   │                   │
 │                │   callback(oid) │                   │                   │
 │                │─────────────────>│                   │                   │
 │                │                 │                   │                   │
 │                │                 │ ra_rust_invalidate│                   │
 │                │                 │   _table(oid)     │                   │
 │                │                 │──────────────────>│                   │
 │                │                 │                   │                   │
 │                │                 │                   │ Lock cache        │
 │                │                 │                   │ Mark invalid      │
 │                │                 │                   │ Add to set        │
 │                │                 │                   │ Inc counter       │
 │                │                 │                   │                   │
 │                │                 │<──────────────────│                   │
 │                │                 │   (return)        │                   │
 │                │<─────────────────│                   │                   │
 │<───────────────│                 │                   │                   │
 │   OK           │                 │                   │                   │
 │                │                 │                   │                   │
 │ SELECT ...     │                 │                   │                   │
 │───────────────>│                 │                   │                   │
 │                │                 │                   │                   │
 │                │ Planner hook    │                   │                   │
 │                │─────────────────>│                   │                   │
 │                │                 │                   │                   │
 │                │                 │ get_table_        │                   │
 │                │                 │   metadata(oid)   │                   │
 │                │                 │──────────────────>│                   │
 │                │                 │                   │                   │
 │                │                 │                   │ is_valid(oid)?    │
 │                │                 │                   │ → false           │
 │                │                 │                   │                   │
 │                │                 │                   │ refresh(oid)      │
 │                │                 │                   │──────────────────>│
 │                │                 │                   │                   │
 │                │                 │                   │ Query pg_class    │
 │                │                 │                   │ Query pg_statistic│
 │                │                 │                   │ Query pg_index    │
 │                │                 │                   │<──────────────────│
 │                │                 │                   │  Statistics       │
 │                │                 │                   │                   │
 │                │                 │                   │ Update cache      │
 │                │                 │<──────────────────│                   │
 │                │                 │   Statistics      │                   │
 │                │                 │                   │                   │
 │                │  Optimized plan │                   │                   │
 │                │<─────────────────│                   │                   │
 │                │                 │                   │                   │
 │   Result       │                 │                   │                   │
 │<───────────────│                 │                   │                   │
```

## Cache State Transitions

```
┌─────────────────┐
│  Cache Empty    │  Initial state
│  (no entries)   │
└────────┬────────┘
         │
         │ First query on table
         │
         v
┌─────────────────┐
│  Cache Miss     │  Table not in cache
│                 │  → Query pg_catalog
└────────┬────────┘
         │
         │ Metadata loaded
         │
         v
┌─────────────────┐
│  Cache Valid    │  Entry cached, is_valid = true
│  (entry exists) │  → Serve from cache (fast)
└────────┬────────┘
         │
         │ DDL event (ALTER, CREATE INDEX, ANALYZE)
         │
         v
┌─────────────────┐
│ Cache Invalid   │  Entry marked stale, is_valid = false
│  (invalidated)  │  → Must refresh on next access
└────────┬────────┘
         │
         │ Next query on table
         │
         v
┌─────────────────┐
│  Cache Miss     │  Entry invalid
│                 │  → Query pg_catalog (refresh)
└────────┬────────┘
         │
         │ Metadata refreshed
         │
         v
┌─────────────────┐
│  Cache Valid    │  Entry updated, is_valid = true
│  (refreshed)    │  → Serve from cache
└────────┬────────┘
         │
         │ LRU eviction (cache > 1000 entries)
         │
         v
┌─────────────────┐
│  Cache Empty    │  Entry removed
│  (evicted)      │  → Cycle repeats
└─────────────────┘
```

## Data Flow: Cache Hit vs. Miss

### Cache Hit (Fast Path)

```
┌──────────────────────────────────────────┐
│ Query Parser                             │
│ - Extract table OIDs                     │
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ get_table_metadata(oid)                  │
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ Lock cache (Mutex)                       │  ← ~0.005ms
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ is_valid(oid)?                           │  ← ~0.001ms
│ → Check HashMap + invalidated set        │     (HashMap lookup)
└────────────────┬─────────────────────────┘
                 │
                 v YES (valid)
┌──────────────────────────────────────────┐
│ Return cached Statistics                 │  ← ~0.002ms
│ - Clone Statistics struct                │     (memory copy)
│ - Increment hits counter                 │
│ - Touch entry (LRU)                      │
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ Unlock cache                             │  ← ~0.002ms
└────────────────┬─────────────────────────┘
                 │
                 v
         Total: ~0.01ms
```

### Cache Miss (Slow Path)

```
┌──────────────────────────────────────────┐
│ Query Parser                             │
│ - Extract table OIDs                     │
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ get_table_metadata(oid)                  │
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ Lock cache (Mutex)                       │  ← ~0.005ms
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ is_valid(oid)?                           │  ← ~0.001ms
│ → Check HashMap + invalidated set        │
└────────────────┬─────────────────────────┘
                 │
                 v NO (invalid or missing)
┌──────────────────────────────────────────┐
│ refresh(oid)                             │
│ → stats_bridge::gather_table_stats_      │
│    by_oid(oid)                           │
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ Query pg_class (syscache)                │  ← ~0.02ms
│ - reltuples, relpages                    │
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ Query pg_statistic (N times)             │  ← ~0.15ms
│ - Column stats for N columns             │     (N=10 cols)
│ - stadistinct, stanullfrac, stawidth     │
│ - MCV, histogram                         │
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ Query pg_index (syscache)                │  ← ~0.02ms
│ - Index list, index stats                │
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ Build Statistics struct                  │  ← ~0.01ms
│ - Aggregate column stats                 │
│ - Calculate avg_row_size                 │
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ Update cache                             │  ← ~0.002ms
│ - Insert entry into HashMap              │
│ - Remove from invalidated set            │
│ - Increment misses counter               │
└────────────────┬─────────────────────────┘
                 │
                 v
┌──────────────────────────────────────────┐
│ Unlock cache                             │  ← ~0.002ms
└────────────────┬─────────────────────────┘
                 │
                 v
         Total: ~0.21ms
```

## Memory Layout

```
┌───────────────────────────────────────────────────────┐
│ METADATA_CACHE (Global Static)                       │
│ Type: Lazy<Mutex<MetadataCache>>                     │
│ Size: 8 bytes (pointer to heap allocation)           │
└───────────────────────────────────────────────────────┘
                        │
                        v
┌───────────────────────────────────────────────────────┐
│ MetadataCache (Heap-allocated)                        │
│ ┌───────────────────────────────────────────────────┐ │
│ │ tables: HashMap<Oid, CachedTableMetadata>        │ │
│ │ - Capacity: 1024 (grows dynamically)              │ │
│ │ - Max entries: 1000 (LRU eviction)                │ │
│ │ - Per entry: ~1 KB                                │ │
│ │ - Total: ~1 MB                                    │ │
│ └───────────────────────────────────────────────────┘ │
│                                                         │
│ ┌───────────────────────────────────────────────────┐ │
│ │ invalidated: HashSet<Oid>                        │ │
│ │ - Capacity: grows dynamically                     │ │
│ │ - Max entries: ~100 (transient)                   │ │
│ │ - Per entry: 4 bytes (Oid)                        │ │
│ │ - Total: ~400 bytes                               │ │
│ └───────────────────────────────────────────────────┘ │
│                                                         │
│ ┌───────────────────────────────────────────────────┐ │
│ │ Metrics (hits, misses, invalidations)            │ │
│ │ - 3 × u64 = 24 bytes                              │ │
│ └───────────────────────────────────────────────────┘ │
│                                                         │
│ Total size: ~1 MB per backend                          │
└───────────────────────────────────────────────────────┘

Per-Entry Memory Breakdown:
┌─────────────────────────────────────────┐
│ CachedTableMetadata                     │
│ ┌─────────────────────────────────────┐ │
│ │ stats: Statistics                   │ │
│ │ - row_count: f64 (8 bytes)          │ │
│ │ - columns: HashMap<String, ...>     │ │
│ │   → ~600 bytes (10 columns)         │ │
│ │ - indexes: HashMap<String, ...>     │ │
│ │   → ~200 bytes (2 indexes)          │ │
│ │ - total_size, avg_row_size: u64     │ │
│ │   → 16 bytes                        │ │
│ └─────────────────────────────────────┘ │
│ refresh_time: SystemTime (16 bytes)     │
│ is_valid: bool (1 byte)                 │
│ last_access: SystemTime (16 bytes)      │
│                                         │
│ Total: ~857 bytes ≈ 1 KB                │
└─────────────────────────────────────────┘
```

## Concurrency Model

```
┌────────────────────────────────────────────────────────┐
│ Multiple PostgreSQL Backends (Processes)               │
│                                                        │
│  Backend 1          Backend 2          Backend N      │
│  ┌────────┐         ┌────────┐         ┌────────┐    │
│  │ Cache  │         │ Cache  │         │ Cache  │    │
│  │ (1 MB) │         │ (1 MB) │         │ (1 MB) │    │
│  └────────┘         └────────┘         └────────┘    │
│      │                  │                  │          │
│      │                  │                  │          │
│  Isolated          Isolated          Isolated        │
│  No sharing        No sharing        No sharing      │
│                                                        │
│  Each backend:                                        │
│  - Separate MetadataCache instance                   │
│  - Separate relcache callback registration           │
│  - Separate invalidation tracking                    │
│  - Independent cache hit/miss stats                  │
└────────────────────────────────────────────────────────┘
                        │
                        │ All backends query
                        │ same pg_catalog
                        v
┌────────────────────────────────────────────────────────┐
│ PostgreSQL System Catalogs (Shared)                   │
│                                                        │
│  pg_class        pg_statistic        pg_index         │
│  - reltuples     - stadistinct       - indkey         │
│  - relpages      - stanullfrac       - indisunique    │
│  - relnatts      - stawidth          - indisprimary   │
│                                                        │
│  Synchronized by PostgreSQL MVCC and WAL              │
└────────────────────────────────────────────────────────┘
```

---

**Note**: Diagrams are ASCII-art representations. For production documentation, consider generating SVG/PNG diagrams using tools like PlantUML or Mermaid.
