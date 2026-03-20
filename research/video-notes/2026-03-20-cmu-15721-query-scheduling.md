# CMU 15-721 Lecture 7: Query Scheduling

**Source:** https://15721.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Query scheduling determines how work is distributed across CPU cores
- Work-stealing vs work-sharing tradeoffs
- Morsel-driven parallelism dominates modern analytical engines
- Memory allocation and NUMA topology affect scheduling decisions

## Scheduling Techniques

### Task-Based Scheduling
- Break query into independent tasks
- Thread pool executes tasks from a queue
- Dynamic load balancing through work stealing
- Fine-grained: per-morsel; coarse-grained: per-pipeline

### Morsel-Driven Parallelism (TUM/HyPer)
- Divide data into fixed-size "morsels" (e.g., 10K tuples)
- Assign morsels to worker threads dynamically
- Pipeline-local state: no synchronization within a morsel
- Merge results at pipeline boundaries
- Achieves near-perfect NUMA-local execution

### Work Stealing
- Each thread has a local deque of morsels
- Idle threads steal from busy threads' deques
- Minimizes scheduling overhead while balancing load
- Cache-efficient: threads work on local data first

### Exchange Operators (Volcano parallelism)
- Gather: collect results from parallel workers
- Distribute: repartition data across workers
- Broadcast: send data to all workers
- Placed by optimizer, not scheduler

## Applicable to RA
- RA has execution-models/morsel-driven/ (13 rules) and parallelization/ (16 rules)
- Gap: No work-stealing scheduling cost model
- Gap: No morsel size selection rules
- Gap: No exchange operator placement optimization
- Gap: No thread pool sizing rules based on query complexity
- Gap: No concurrent query scheduling (resource allocation across queries)

## References
- Leis et al. "Morsel-Driven Parallelism: A NUMA-Aware Query Evaluation Framework for the Many-Core Age" (2014)
- Raasveldt & Muehleisen. "DuckDB" (implementation of morsel scheduling)
