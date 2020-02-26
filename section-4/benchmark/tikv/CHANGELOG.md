# TiKV Change Log
All notable changes to this project are documented in this file.
See also [TiDB Changelog](https://github.com/pingcap/tidb/blob/master/CHANGELOG.md) and [PD Changelog](https://github.com/pingcap/pd/blob/master/CHANGELOG.md).

## [3.0.0-beta.1]
- Optimize the Coprocessor calculation execution framework and implement the TableScan section, with the Single TableScan performance improved by 5% ~ 30%
    - Implement the definition of the `BatchRows` row and the `BatchColumn` column [#3660](https://github.com/tikv/tikv/pull/3660)
    - Implement `VectorLike` to support accessing encoded and decoded data in the same way [#4242](https://github.com/tikv/tikv/pull/4242)
    - Define the `BatchExecutor` to interface and implement the way of converting requests to `BatchExecutor` [#4243](https://github.com/tikv/tikv/pull/4243)
    - Implement transforming the expression tree into the RPN format [#4329](https://github.com/tikv/tikv/pull/4329)
    - Implement the `BatchTableScanExecutor` vectorization calculation operator [#4351](https://github.com/tikv/tikv/pull/4351)
- Unify the log format for easy collection and analysis by tools
- Support using the Local Reader to read in the Raw Read interface [#4222](https://github.com/tikv/tikv/pull/4222)
- Add metrics about configuration information [#4206](https://github.com/tikv/tikv/pull/4206)
- Add metrics about key exceeding bound [#4255](https://github.com/tikv/tikv/pull/4255)
- Add an option to control panic or return an error when encountering the key exceeding bound error [#4254](https://github.com/tikv/tikv/pull/4254)
- Add support for the `INSERT` operation, make prewrite succeed only when keys do not exist, and eliminate `Batch Get` [#4085](https://github.com/tikv/tikv/pull/4085)
- Use more fair batch strategy in the Batch System [#4200](https://github.com/tikv/tikv/pull/4200)
- Support Raw scan in tikv-ctl [#3825](https://github.com/tikv/tikv/pull/3825)

## [3.0.0-beta]
- Support distributed GC [#3179](https://github.com/tikv/tikv/pull/3179)
- Check RocksDB Level 0 files before applying snapshots to avoid Write Stall [#3606](https://github.com/tikv/tikv/pull/3606)
- Support reverse `raw_scan` and `raw_batch_scan` [#3724](https://github.com/tikv/tikv/pull/3724)
- Support using HTTP to obtain monitoring information [#3855](https://github.com/tikv/tikv/pull/3855)
- Support DST better [#3786](https://github.com/tikv/tikv/pull/3786)
- Support receiving and sending Raft messages in batch [#3913](https://github.com/tikv/tikv/pull/3913)
- Introduce a new storage engine Titan [#3985](https://github.com/tikv/tikv/pull/3985)
- Upgrade gRPC to v1.17.2 [#4023](https://github.com/tikv/tikv/pull/4023)
- Support receiving the client requests and sending replies in batch [#4043](https://github.com/tikv/tikv/pull/4043)
- Support multi-thread Apply [#4044](https://github.com/tikv/tikv/pull/4044)
- Support multi-thread Raftstore [#4066](https://github.com/tikv/tikv/pull/4066)

## [2.1.2]
- Support the configuration format in the unit of `DAY` (`d`) and fix the configuration compatibility issue [#3931](https://github.com/tikv/tikv/pull/3931)
- Fix the possible panic issue caused by `Approximate Size Split` [#3942](https://github.com/tikv/tikv/pull/3942)
- Fix two issues about Region merge [#3822](https://github.com/tikv/tikv/pull/3822), [#3873](https://github.com/tikv/tikv/pull/3873)

## [2.1.1]
- Avoid transferring the leader to a newly created peer, to optimize the possible delay [#3878](https://github.com/tikv/tikv/pull/3878)

## [2.1.0]
+ Coprocessor
    - Add more built-in functions
    - [Add Coprocessor `ReadPool` to improve the concurrency in processing the requests](https://github.com/tikv/rfcs/blob/master/text/2017-12-22-read-pool.md)
    - Fix the time function parsing issue and the time zone related issues
    - Optimize the memory usage for pushdown aggregation computing

+ Transaction
    - Optimize the read logic and memory usage of MVCC to improve the performance of the scan operation and the performance of full table scan is 1 time better than that in TiDB 2.0
    - Fold the continuous Rollback records to ensure the read performance
    - [Add the `UnsafeDestroyRange` API to support to collecting space for the dropping table/index](https://github.com/tikv/rfcs/blob/master/text/2018-08-29-unsafe-destroy-range.md)
    - Separate the GC module to reduce the impact on write
    - Add the`upper bound` support in the `kv_scan` command

+ Raftstore
    - Improve the snapshot writing process to avoid RocksDB stall
    - [Add the `LocalReader` thread to process read requests and reduce the delay for read requests](https://github.com/tikv/rfcs/pull/17)
    - [Support `BatchSplit` to avoid large Region brought by large amounts of write](https://github.com/tikv/rfcs/pull/6)
    - Support `Region Split` according to statistics to reduce the I/O overhead
    - Support `Region Split` according to the number of keys to improve the concurrency of index scan
    - Improve the Raft message process to avoid unnecessary delay brought by `Region Split`
    - Enable the `PreVote` feature by default to reduce the impact of network isolation on services

+ Storage Engine
    - Fix the `CompactFiles` bug in RocksDB and reduce the impact on importing data using Lightning
    - Upgrade RocksDB to v5.15 to fix the possible issue of snapshot file corruption
    - Improve `IngestExternalFile` to avoid the issue that flush could block write

+ tikv-ctl
    - [Add the `ldb` command to diagnose RocksDB related issues](https://github.com/tikv/tikv/blob/master/docs/tools/tikv-control.md#ldb-command)
    - The `compact` command supports specifying whether to compact data in the bottommost level

+ Tools
    - Fast full import of large amounts of data: [TiDB-Lightning](https://pingcap.com/docs/tools/lightning/overview-architecture/)
    - Support new [TiDB-Binlog](https://pingcap.com/docs/tools/tidb-binlog-cluster/)

## [2.1.0-rc.5]
- Improve the error message of `WriteConflict` [#3750](https://github.com/tikv/tikv/pull/3750)
- Add the panic mark file [#3746](https://github.com/tikv/tikv/pull/3746)
- Downgrade grpcio to avoid the segment fault issue caused by the new version of gRPC [#3650](https://github.com/tikv/tikv/pull/3650)
- Add the upper limit to the `kv_scan` interface [#3749](https://github.com/tikv/tikv/pull/3749)

## [2.1.0-rc.4]
- Optimize the RocksDB Write stall issue caused by applying snapshots [#3606](https://github.com/tikv/tikv/pull/3606)
- Add raftstore `tick` metrics [#3657](https://github.com/tikv/tikv/pull/3657)
- Upgrade RocksDB and fix the Write block issue and that the source file might be damaged by the Write operation when performing `IngestExternalFile` [#3661](https://github.com/tikv/tikv/pull/3661)
- Upgrade grpcio and fix the issue that “too many pings” is wrongly reported [#3650](https://github.com/tikv/tikv/pull/3650)

## [2.1.0-rc.3]
### Performance
- Optimize the concurrency for coprocessor requests [#3515](https://github.com/tikv/tikv/pull/3515)
### New features
- Add the support for Log functions [#3603](https://github.com/tikv/tikv/pull/3603)
- Add the support for the `sha1` function [#3612](https://github.com/tikv/tikv/pull/3612)
- Add the support for the `truncate_int` function [#3532](https://github.com/tikv/tikv/pull/3532)
- Add the support for the `year` function [#3622](https://github.com/tikv/tikv/pull/3622)
- Add the support for the `truncate_real` function [#3633](https://github.com/tikv/tikv/pull/3633)
### Bug fixes
- Fix the reporting error behavior related to time functions [#3487](https://github.com/tikv/tikv/pull/3487), [#3615](https://github.com/tikv/tikv/pull/3615)
- Fix the issue that the time parsed from string is inconsistent with that in TiDB [#3589](https://github.com/tikv/tikv/pull/3589)

## [2.1.0-rc.2]
### Performance
* Support splitting Regions based on statistics estimation to reduce the I/O cost [#3511](https://github.com/tikv/tikv/pull/3511)
* Reduce clone in the transaction scheduler [#3530](https://github.com/tikv/tikv/pull/3530)
### Improvements
* Add the pushdown support for a large number of built-in functions
* Add the `leader-transfer-max-log-lag` configuration to fix the failure issue of leader scheduling in specific scenarios [#3507](https://github.com/tikv/tikv/pull/3507)
* Add the `max-open-engines` configuration to limit the number of engines opened by `tikv-importer` simultaneously [#3496](https://github.com/tikv/tikv/pull/3496)
* Limit the cleanup speed of garbage data to reduce the impact on `snapshot apply` [#3547](https://github.com/tikv/tikv/pull/3547)
* Broadcast the commit message for crucial Raft messages to avoid unnecessary delay [#3592](https://github.com/tikv/tikv/pull/3592)
### Bug fixes
* Fix the leader election issue caused by discarding the `PreVote` message of the newly split Region [#3557](https://github.com/tikv/tikv/pull/3557)
* Fix follower related statistics after merging Regions [#3573](https://github.com/tikv/tikv/pull/3573)
* Fix the issue that the local reader uses obsolete Region information [#3565](https://github.com/tikv/tikv/pull/3565)
* Support UnsafeDestroyRange API to speedup garbage data cleaning after table/index has been truncated/dropped [#3560](https://github.com/tikv/tikv/pull/3560)

## [2.1.0-rc.1]
### Features
* Support `batch split` to avoid too large Regions caused by the Write operation on hot Regions
* Support splitting Regions based on the number of rows to improve the index scan efficiency
### Performance
* Use `LocalReader` to separate the Read operation from the raftstore thread to lower the Read latency
* Refactor the MVCC framework, optimize the memory usage and improve the scan Read performance
* Support splitting Regions based on statistics estimation to reduce the I/O usage
* Optimize the issue that the Read performance is affected by continuous Write operations on the rollback record
* Reduce the memory usage of pushdown aggregation computing
### Improvements
* Add the pushdown support for a large number of built-in functions and better charset support
* Optimize the GC workflow, improve the GC speed and decrease the impact of GC on the system
* Enable `prevote` to speed up service recovery when the network is abnormal
* Add the related configuration items of RocksDB log files
* Adjust the default configuration of `scheduler_latch`
* Support setting whether to compact the data in the bottom layer of RocksDB when using tikv-ctl to compact data manually
* Add the check for environment variables when starting TiKV
* Support dynamically configuring the `dynamic_level_bytes` parameter based on the existing data
* Support customizing the log format
* Integrate tikv-fail in tikv-ctl
* Add I/O metrics of threads
### Bug fixes
* Fix decimal related issues
* Fix the issue that `gRPC max_send_message_len` is set mistakenly
* Fix the issue caused by misconfiguration of `region_size`

## [2.1.0-beta]
### Features
* Upgrade Rust to the `nightly-2018-06-14` version
* Provide a `Raft PreVote` configuration to avoid leader reelection generated when network recovers after network isolation
* Add a metric to display the number of files and `ingest` related information in each layer of RocksDB
* Print `key` with too many versions when GC works
### Performance
* Use `static metric` to optimize multi-label metric performance (YCSB `raw get` is improved by 3%)
* Remove `box` in multiple modules and use patterns to improve the operating performance (YCSB `raw get` is improved by 3%)
* Use `asynchronous log` to improve the performance of writing logs
* Add a metric to collect the thread status
* Decease memory copy times by decreasing `box` used in the application to improve the performance

## [2.0.4]
### Features
* Add the RocksDB `PerfContext` interface for debugging
* Add the `region-properties` command for `tikv-ctl`
### Improvements
* Make GC record the log when GC encounters many versions of data
* Remove the `import-mode` parameter
### Bug Fixes
* Fix the issue that `reverse-seek` is slow when many RocksDB tombstones exist
* Fix the crash issue caused by `do_sub`

## [2.0.3]
### Bug Fixes
* Correct wrong peer meta for learners
* Report an error instead of getting a result if divisor/dividend is 0 in do_div_mod

## [2.0.2]
### Improvements
* Support configuring more gRPC related parameters
* Support configuring the timeout range of leader election
### Bug Fixes
* Fix the issue that the Raft log is not printed
* Fix the issue that obsolete learner is not deleted
* Fix the issue that the snapshot intermediate file is mistakenly deleted

## [2.0.1]
### Performance
* Reduced number of `thread_yield` calls
* Fix the issue that `SELECT FOR UPDATE` prevents others from reading
### Improvements
* More verbose logs for slow query
* Speed up delete range
### Bug Fixes
* Fix the bug that raftstore is accidentally blocked when generating the snapshot
* Fix the issue that Learner cannot be successfully elected in special conditions
* Fix the issue that split might cause dirty read in extreme conditions
* Correct the default value of the read thread pool configuration

## [2.0.0] - 2018-04-27
### Features
* Protect critical configuration from incorrect modification
* Support `Region Merge` [experimental]
* Add the `Raw DeleteRange` API
* Add the `GetMetric` API
* Add `Raw Batch Put`, `Raw Batch Get`, `Raw Batch Delete` and `Raw Batch Scan`
* Add Column Family options for the RawKV API and support executing operation on a specific Column Family
* Support Streaming and Streaming Aggregation in Coprocessor
* Support configuring the request timeout of Coprocessor
* Carry timestamps with Region heartbeats
* Support modifying some RocksDB parameters online, such as `block-cache-size`
* Support configuring the behavior of Coprocessor when it encounters some warnings or errors
* Support starting in the importing data mode to reduce write amplification during the data importing process
* Support manually splitting Region in halves
* Improve the data recovery tool `tikv-ctl`
* Return more statistics in Coprocessor to guide the behavior of TiDB
* Support the `ImportSST` API to import SST files [experimental]
* Add the TiKV Importer binary to integrate with TiDB Lightning to import data quickly [experimental]
### Performance
* Optimize read performance using `ReadPool` and increase the `raw_get/get/batch_get` by 30%
* Improve metrics performance
* Inform PD immediately once the Raft snapshot process is completed to speed up balancing
* Solve performance jitter caused by RocksDB flushing
* Optimize the space reclaiming mechanism after deleting data
* Speed up garbage cleaning while starting the server
* Reduce the I/O overhead during replica migration using `DeleteFilesInRanges`
### Stability
* Fix the issue that gRPC call does not returned when the PD leader switches
* Fix the issue that it is slow to offline nodes caused by snapshots
* Limit the temporary space usage consumed by migrating replicas
* Report the Regions that cannot elect a leader for a long time
* Update the Region size information in time according to compaction events
* Limit the size of scan lock to avoid request timeout
* Limit the memory usage when receiving snapshots to avoid OOM
* Increase the speed of CI test
* Fix the OOM issue caused by too many snapshots
* Configure `keepalive` of gRPC
* Fix the OOM issue caused by an increase of the Region number

## [2.0.0-rc6] - 2018-04-19
### Improvements
* Reduce lock contention in Worker
* Add metrics to the FuturePool
### Bug Fixes
* Fix misused metrics in Coprocessor

## [2.0.0-rc.5] - 2018-04-17
### New Features
* Support compacting Regions in `tikv-ctl`
* Add raw batch put/get/delete/scan API for TiKV service
* Add ImportKV service
* Support eval error in Coprocessor
* Support dynamic adjustment of RocksDB cache size by `tikv-ctl`
* Collect number of rows scanned for each range in Coprocessor
* Support treating overflow as warning in Coprocessor
* Support learner in raftstore
### Improvements
* Increase snap GC timeout

## [2.0.0-rc.4] - 2018-04-01
### New Features
* Limit the memory usage during receiving snapshots, to avoid OOM in extreme conditions
* Support configuring the behavior of Coprocessor when it encounters warnings
* Support importing the data pattern in TiKV
* Support splitting Region in the middle
### Improvements
* Fix the issue that too many logs are output caused by leader missing when TiKV is isolated
* Use crossbeam channel in worker

## [2.0.0-rc.3] - 2018-03-23
### New Features
* Support Region Merge
* Add the Raw DeleteRange API
* Add the GetMetric API
* Support streaming in Coprocessor
* Support modifying RocksDB parameters online
### Improvements
* Inform PD immediately once the Raft snapshot process is completed, to speed up balancing
* Reduce the I/O fluctuation caused by RocksDB sync files
* Optimize the space reclaiming mechanism after deleting data
* Improve the data recovery tool `tikv-ctl`
* Fix the issue that it is slow to make nodes down caused by snapshot
* Increase the raw_get/get/batch_get by 30% with ReadPool
* Support configuring the request timeout of Coprocessor
* Carry time information in Region heartbeats
* Limit the space usage of snapshot files to avoid consuming too much disk space
* Record and report the Regions that cannot elect a leader for a long time
* Speed up garbage cleaning when starting the server
* Update the size information about the corresponding Region according to compaction events
* Limit the size of scan lock to avoid request timeout
* Use DeleteRange to speed up Region deletion

## [2.0.0-rc.2] - 2018-03-15
### New Features
* Implement IngestSST API
* `tikv-ctl` now can send consistency-check requests to TiKV
* Support dumping stats of RocksDB and malloc in `tikv-ctl`
### Improvements
* Reclaim disk space after data have been deleted

## [2.0.0-rc.1] - 2018-03-09
### New Features
* Protect important configuration which cannot be changed after initial configuration
* Check whether SSD is used when you start the cluster
### Improvements
* Fix the issue that gRPC call is not cancelled when PD leaders switch
* Optimize the read performance using ReadPool, and improve the performance by 30% for raw get
* Improve metrics and optimize the usage of metrics

## [1.1.0-beta] - 2018-02-24
### Improvements
* Traverse locks using offset + limit to avoid potential GC problems
* Support resolving locks in batches to improve GC speed
* Support GC concurrency to improve GC speed
* Update the Region size using the RocksDB compaction listener for more accurate PD scheduling
* Delete the outdated data in batches using DeleteFilesInRanges, to make TiKV start faster
* Configure the Raft snapshot max size to avoid the retained files taking up too much space
* Support more recovery operations in tikv-ctl
* Optimize the ordered flow aggregation operation

## [1.0.8] - 2018-02-11
### Improvements
* Use DeleteFilesInRanges to clear stale data and improve the TiKV starting speed
* Sync the metadata of the received Snapshot compulsorily to ensure its safety
### Bug Fixes
* Use Decimal in Coprocessor sum

## [1.0.7] - 2018-01-22
### Improvements
* Support key-only option in Table Scan executor
* Support the remote mode in tikv-ctl
* Fix the loss of scheduling command from PD
### Bug Fixes
* Fix the format compatibility issue of tikv-ctl proto
* Add timeout in Push metric


## [1.1.0-alpha] - 2018-01-19
### New Features
* Support Raft learner
* Support TLS
### Improvements
* Optimize Raft Snapshot and reduce the I/O overhead
* Optimize the RocksDB configuration to improve performance
* Optimize count (*) and query performance of unique index in Coprocessor
* Solve the reconnection issue between PD and TiKV
* Enhance the features of the data recovery tool `tikv-ctl`
* Support the Delete Range feature
* Support splitting according to table in Regions
* Support setting the I/O limit caused by snapshot
* Improve the flow control mechanism
