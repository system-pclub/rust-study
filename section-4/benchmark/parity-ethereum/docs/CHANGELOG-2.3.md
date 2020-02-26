## Parity-Ethereum [v2.3.8](https://github.com/paritytech/parity-ethereum/releases/tag/v2.3.8) (2019-03-22)

Parity-Ethereum 2.3.8-stable is a bugfix release that improves performance and stability. This patch release contains a critical bug fix where serving light clients previously led to client crashes. Upgrading is highly recommended.

The full list of included changes:
- 2.3.8 stable backports ([#10507](https://github.com/paritytech/parity-ethereum/pull/10507))
  - Version: bump stable
  - Add additional request tests ([#10503](https://github.com/paritytech/parity-ethereum/pull/10503))

## Parity-Ethereum [v2.3.7](https://github.com/paritytech/parity-ethereum/releases/tag/v2.3.7) (2019-03-20)

Parity-Ethereum 2.3.7-stable is a bugfix release that improves performance and stability.

The full list of included changes:
- 2.3.7 stable backports ([#10487](https://github.com/paritytech/parity-ethereum/pull/10487))
  - Version: bump stable
  - Сaching through docker volume ([#10477](https://github.com/paritytech/parity-ethereum/pull/10477))
  - fix win&mac build ([#10486](https://github.com/paritytech/parity-ethereum/pull/10486))
  - fix(extract `timestamp_checked_add` as lib) ([#10383](https://github.com/paritytech/parity-ethereum/pull/10383))

## Parity-Ethereum [v2.3.6](https://github.com/paritytech/parity-ethereum/releases/tag/v2.3.6) (2019-03-19)

Parity-Ethereum 2.3.6-stable is a bugfix release that improves performance and stability.

The full list of included changes:
- 2.3.6 stable backports ([#10470](https://github.com/paritytech/parity-ethereum/pull/10470))
  - Version: bump stable
  - CI publish to aws ([#10446](https://github.com/paritytech/parity-ethereum/pull/10446))
  - Ensure static validator set changes are recognized ([#10467](https://github.com/paritytech/parity-ethereum/pull/10467))
  - CI aws git checkout ([#10451](https://github.com/paritytech/parity-ethereum/pull/10451))
  - Revert "CI aws git checkout ([#10451](https://github.com/paritytech/parity-ethereum/pull/10451))" ([#10456](https://github.com/paritytech/parity-ethereum/pull/10456))
  - Tests parallelized ([#10452](https://github.com/paritytech/parity-ethereum/pull/10452))

## Parity-Ethereum [v2.3.5](https://github.com/paritytech/parity-ethereum/releases/tag/v2.3.5) (2019-02-25)

Parity-Ethereum 2.3.5-stable is a bugfix release that improves performance and stability.

Note, all 2.2 releases and older are now unsupported and upgrading is recommended.

The full list of included changes:

- More Backports for Stable 2.3.5 ([#10430](https://github.com/paritytech/parity-ethereum/pull/10430))
  - Revert some changes, could be buggy ([#10399](https://github.com/paritytech/parity-ethereum/pull/10399))
  - Ci: clean up gitlab-ci.yml leftovers from previous merge ([#10429](https://github.com/paritytech/parity-ethereum/pull/10429))
  - 10000 > 5000 ([#10422](https://github.com/paritytech/parity-ethereum/pull/10422))
  - Fix underflow in pip, closes [#10419](https://github.com/paritytech/parity-ethereum/pull/10419) ([#10423](https://github.com/paritytech/parity-ethereum/pull/10423))
  - Fix panic when logging directory does not exist, closes [#10420](https://github.com/paritytech/parity-ethereum/pull/10420) ([#10424](https://github.com/paritytech/parity-ethereum/pull/10424))
  - Update hardcoded headers for Foundation, Ropsten, Kovan and Classic ([#10417](https://github.com/paritytech/parity-ethereum/pull/10417))
- Backports for Stable 2.3.5 ([#10414](https://github.com/paritytech/parity-ethereum/pull/10414))
  - No-git for publish jobs, empty artifacts dir ([#10393](https://github.com/paritytech/parity-ethereum/pull/10393))
  - Snap: reenable i386, arm64, armhf architecture publishing ([#10386](https://github.com/paritytech/parity-ethereum/pull/10386))
  - Tx pool: always accept local transactions ([#10375](https://github.com/paritytech/parity-ethereum/pull/10375))
  - Fix to_pod storage trie value decoding ([#10368)](https://github.com/paritytech/parity-ethereum/pull/10368))
- Version: mark 2.3.5 as stable

## Parity-Ethereum [v2.3.4](https://github.com/paritytech/parity-ethereum/releases/tag/v2.3.4) (2019-02-21)

Parity-Ethereum 2.3.4-beta is a maintenance release that fixes snap and docker installations.

The full list of included changes:
- Beta: snap: release untagged versions from branches to the candidate ([#10357](https://github.com/paritytech/parity-ethereum/pull/10357)) ([#10373](https://github.com/paritytech/parity-ethereum/pull/10373))
  - Snap: release untagged versions from branches to the candidate snap channel ([#10357](https://github.com/paritytech/parity-ethereum/pull/10357))
  - Snap: add the removable-media plug ([#10377](https://github.com/paritytech/parity-ethereum/pull/10377))
  - Exchanged old(azure) bootnodes with new(ovh) ones ([#10309](https://github.com/paritytech/parity-ethereum/pull/10309))
- Beta Backports ([#10354](https://github.com/paritytech/parity-ethereum/pull/10354))
  - Version: bump beta to 2.3.4
  - Snap: prefix version and populate candidate channel ([#10343](https://github.com/paritytech/parity-ethereum/pull/10343))
  - Snap: populate candidate releases with beta snaps to avoid stale channel
  - Snap: prefix version with v*
  - No volumes are needed, just run -v volume:/path/in/the/container ([#10345](https://github.com/paritytech/parity-ethereum/pull/10345))

## Parity-Ethereum [v2.3.3](https://github.com/paritytech/parity-ethereum/releases/tag/v2.3.3) (2019-02-13)

Parity-Ethereum 2.3.3-beta is a security-relevant release. A bug in the JSONRPC-deserialization module can cause crashes of all versions of Parity Ethereum nodes if an attacker is able to submit a specially-crafted RPC to certain publicly available endpoints.

- https://www.parity.io/new-parity-ethereum-update-fixes-several-rpc-vulnerabilities/

The full list of included changes:

- Additional error for invalid gas ([#10327](https://github.com/paritytech/parity-ethereum/pull/10327)) ([#10328](https://github.com/paritytech/parity-ethereum/pull/10328))
- Backports for Beta 2.3.3 ([#10333](https://github.com/paritytech/parity-ethereum/pull/10333))
    - Properly handle check_epoch_end_signal errors ([#10015](https://github.com/paritytech/parity-ethereum/pull/10015))
    - import rpc transactions sequentially ([#10051](https://github.com/paritytech/parity-ethereum/pull/10051))
    - fix(docker): fix not receives SIGINT ([#10059](https://github.com/paritytech/parity-ethereum/pull/10059))
    - snap: official image / test ([#10168](https://github.com/paritytech/parity-ethereum/pull/10168))
    - Extract CallContract and RegistryInfo traits into their own crate ([#10178](https://github.com/paritytech/parity-ethereum/pull/10178))
    - perform stripping during build ([#10208](https://github.com/paritytech/parity-ethereum/pull/10208))
    - Remove CallContract and RegistryInfo re-exports from `ethcore/client` ([#10205](https://github.com/paritytech/parity-ethereum/pull/10205))
    - fixed: types::transaction::SignedTransaction; ([#10229](https://github.com/paritytech/parity-ethereum/pull/10229))
    - Additional tests for uint/hash/bytes deserialization. ([#10279](https://github.com/paritytech/parity-ethereum/pull/10279))
    - Fix Windows build ([#10284](https://github.com/paritytech/parity-ethereum/pull/10284))
    - Don't run the CPP example on CI ([#10285](https://github.com/paritytech/parity-ethereum/pull/10285))
    - CI optimizations ([#10297](https://github.com/paritytech/parity-ethereum/pull/10297))
    - fix publish job ([#10317](https://github.com/paritytech/parity-ethereum/pull/10317))
    - Add Statetest support for Constantinople Fix ([#10323](https://github.com/paritytech/parity-ethereum/pull/10323))
    - Add helper for Timestamp overflows ([#10330](https://github.com/paritytech/parity-ethereum/pull/10330))
    - Don't add discovery initiators to the node table ([#10305](https://github.com/paritytech/parity-ethereum/pull/10305))
    - change docker image based on debian instead of ubuntu due to the chan ([#10336](https://github.com/paritytech/parity-ethereum/pull/10336))
    - role back docker build image and docker deploy image to ubuntu:xenial based ([#10338](https://github.com/paritytech/parity-ethereum/pull/10338))

## Parity-Ethereum [v2.3.2](https://github.com/paritytech/parity-ethereum/releases/tag/v2.3.2) (2019-02-03)

Parity-Ethereum 2.3.2-stable is a security-relevant release. A bug in the JSONRPC-deserialization module can cause crashes of all versions of Parity Ethereum nodes if an attacker is able to submit a specially-crafted RPC to certain publicly available endpoints.

- https://www.parity.io/security-alert-parity-ethereum-03-02/

The full list of included changes:
- Version: bump beta to 2.3.2 ([#10283](https://github.com/paritytech/parity-ethereum/pull/10283))
- Additional tests for uint deserialization. ([#10279](https://github.com/paritytech/parity-ethereum/pull/10279)) ([#10280](https://github.com/paritytech/parity-ethereum/pull/10280))
- Backport [#10285](https://github.com/paritytech/parity-ethereum/pull/10285) to beta ([#10286](https://github.com/paritytech/parity-ethereum/pull/10286))

## Parity-Ethereum [v2.3.1](https://github.com/paritytech/parity-ethereum/releases/tag/v2.3.1) (2019-02-01)

Parity-Ethereum 2.3.1-beta is a consensus-relevant release that enables _St. Petersfork_ on:

- Ethereum Block `7280000` (along with Constantinople)
- Kovan Block `10255201`
- Ropsten Block `4939394`
- POA Sokol Block `7026400`

In addition to this, Constantinople is cancelled for the POA Core network. Upgrading is mandatory for clients on any of these chains.

The full list of included changes:

- Backports for beta 2.3.1 ([#10225](https://github.com/paritytech/parity-ethereum/pull/10225))
  - Fix _cannot recursively call into `Core`_ issue ([#10144](https://github.com/paritytech/parity-ethereum/pull/10144))
  - Update for Android cross-compilation. ([#10180](https://github.com/paritytech/parity-ethereum/pull/10180))
  - Fix _cannot recursively call into `Core`_ - Part 2 ([#10195](https://github.com/paritytech/parity-ethereum/pull/10195))
  - Cancel Constantinople HF on POA Core ([#10198](https://github.com/paritytech/parity-ethereum/pull/10198))
  - Add EIP-1283 disable transition ([#10214](https://github.com/paritytech/parity-ethereum/pull/10214))
  - Enable St-Peters-Fork ("Constantinople Fix") ([#10223](https://github.com/paritytech/parity-ethereum/pull/10223))
- Beta: Macos heapsize force jemalloc ([#10234](https://github.com/paritytech/parity-ethereum/pull/10234)) ([#10259](https://github.com/paritytech/parity-ethereum/pull/10259))

## Parity-Ethereum [v2.3.0](https://github.com/paritytech/parity-ethereum/releases/tag/v2.3.0) (2019-01-16)

Parity-Ethereum 2.3.0-beta is a consensus-relevant security release that reverts Constantinople on the Ethereum network. Upgrading is mandatory for Ethereum, and strongly recommended for other networks.

- **Consensus** - Ethereum Network: Pull Constantinople protocol upgrade on Ethereum ([#10189](https://github.com/paritytech/parity-ethereum/pull/10189))
  - Read more: [Security Alert: Ethereum Constantinople Postponement](https://blog.ethereum.org/2019/01/15/security-alert-ethereum-constantinople-postponement/)
- **Networking** - All networks: Ping nodes from discovery ([#10167](https://github.com/paritytech/parity-ethereum/pull/10167))
- **Wasm** - Kovan Network: Update pwasm-utils to 0.6.1 ([#10134](https://github.com/paritytech/parity-ethereum/pull/10134))

Other notable changes:

- Existing blocks in the database are now kept when restoring a Snapshot. ([#8643](https://github.com/paritytech/parity-ethereum/pull/8643))
- Block and transaction propagation is improved significantly. ([#9954](https://github.com/paritytech/parity-ethereum/pull/9954))
- The ERC-191 Signed Data Standard is now supported by `personal_sign191`. ([#9701](https://github.com/paritytech/parity-ethereum/pull/9701))
- Add support for ERC-191/712 `eth_signTypedData` as a standard for machine-verifiable and human-readable typed data signing with Ethereum keys. ([#9631](https://github.com/paritytech/parity-ethereum/pull/9631))
- Add support for ERC-1186 `eth_getProof` ([#9001](https://github.com/paritytech/parity-ethereum/pull/9001))
- Add experimental RPCs flag to enable ERC-191, ERC-712, and ERC-1186 APIs via `--jsonrpc-experimental` ([#9928](https://github.com/paritytech/parity-ethereum/pull/9928))
- Make `CALLCODE` to trace value to be the code address. ([#9881](https://github.com/paritytech/parity-ethereum/pull/9881))

Configuration changes:

- The EIP-98 transition is now disabled by default. If you previously had no `eip98transition` specified in your chain specification, you would enable this now manually on block `0x0`. ([#9955](https://github.com/paritytech/parity-ethereum/pull/9955))
- Also, unknown fields in chain specs are now rejected. ([#9972](https://github.com/paritytech/parity-ethereum/pull/9972))
- The Tendermint engine was removed from Parity Ethereum and is no longer available and maintained. ([#9980](https://github.com/paritytech/parity-ethereum/pull/9980))
- Ropsten testnet data and keys moved from `test/` to `ropsten/` subdir. To reuse your old keys and data either copy or symlink them to the new location.  ([#10123](https://github.com/paritytech/parity-ethereum/pull/10123))
- Strict empty steps validation ([#10041](https://github.com/paritytech/parity-ethereum/pull/10041))
  - If you have a chain with`empty_steps` already running, some blocks most likely contain non-strict entries (unordered or duplicated empty steps). In this release `strict_empty_steps_transition` is enabled by default at block `0x0` for any chain with `empty_steps`.
  - If your network uses `empty_steps` you **must** (A) plan a hard fork and change `strict_empty_steps_transition` to the desired fork block and (B) update the clients of the whole network to 2.2.7-stable / 2.3.0-beta. If for some reason you don't want to do this please set`strict_empty_steps_transition` to `0xfffffffff` to disable it.

_Note:_ This release marks Parity 2.3 as _beta_. All versions of Parity 2.2 are now considered _stable_.

The full list of included changes:

- Backports for 2.3.0 beta ([#10164](https://github.com/paritytech/parity-ethereum/pull/10164))
- Snap: fix path in script ([#10157](https://github.com/paritytech/parity-ethereum/pull/10157))
- Make sure parent block is not in importing queue when importing ancient blocks ([#10138](https://github.com/paritytech/parity-ethereum/pull/10138))
- Ci: re-enable snap publishing ([#10142](https://github.com/paritytech/parity-ethereum/pull/10142))
- Hf in POA Core (2019-01-18) - Constantinople ([#10155](https://github.com/paritytech/parity-ethereum/pull/10155))
- Update EWF's tobalaba chainspec ([#10152](https://github.com/paritytech/parity-ethereum/pull/10152))
- Replace ethcore-logger with env-logger. ([#10102](https://github.com/paritytech/parity-ethereum/pull/10102))
- Finality: dont require chain head to be in the chain ([#10054](https://github.com/paritytech/parity-ethereum/pull/10054))
- Remove caching for node connections ([#10143](https://github.com/paritytech/parity-ethereum/pull/10143))
- Blooms file iterator empty on out of range position. ([#10145](https://github.com/paritytech/parity-ethereum/pull/10145))
- Autogen docs for the "Configuring Parity Ethereum" wiki page. ([#10067](https://github.com/paritytech/parity-ethereum/pull/10067))
- Misc: bump license header to 2019 ([#10135](https://github.com/paritytech/parity-ethereum/pull/10135))
- Hide most of the logs from cpp example. ([#10139](https://github.com/paritytech/parity-ethereum/pull/10139))
- Don't try to send oversized packets ([#10042](https://github.com/paritytech/parity-ethereum/pull/10042))
- Private tx enabled flag added into STATUS packet ([#9999](https://github.com/paritytech/parity-ethereum/pull/9999))
- Update pwasm-utils to 0.6.1 ([#10134](https://github.com/paritytech/parity-ethereum/pull/10134))
- Extract blockchain from ethcore ([#10114](https://github.com/paritytech/parity-ethereum/pull/10114))
- Ethcore: update hardcoded headers ([#10123](https://github.com/paritytech/parity-ethereum/pull/10123))
- Identity fix ([#10128](https://github.com/paritytech/parity-ethereum/pull/10128))
- Use LenCachingMutex to optimize verification. ([#10117](https://github.com/paritytech/parity-ethereum/pull/10117))
- Pyethereum keystore support ([#9710](https://github.com/paritytech/parity-ethereum/pull/9710))
- Bump rocksdb-sys to 0.5.5 ([#10124](https://github.com/paritytech/parity-ethereum/pull/10124))
- Parity-clib: `async C bindings to RPC requests` + `subscribe/unsubscribe to websocket events` ([#9920](https://github.com/paritytech/parity-ethereum/pull/9920))
- Refactor (hardware wallet) : reduce the number of threads ([#9644](https://github.com/paritytech/parity-ethereum/pull/9644))
- Hf in POA Sokol (2019-01-04) ([#10077](https://github.com/paritytech/parity-ethereum/pull/10077))
- Fix broken links ([#10119](https://github.com/paritytech/parity-ethereum/pull/10119))
- Follow-up to [#10105](https://github.com/paritytech/parity-ethereum/issues/10105) ([#10107](https://github.com/paritytech/parity-ethereum/pull/10107))
- Move EIP-712 crate back to parity-ethereum ([#10106](https://github.com/paritytech/parity-ethereum/pull/10106))
- Move a bunch of stuff around ([#10101](https://github.com/paritytech/parity-ethereum/pull/10101))
- Revert "Add --frozen when running cargo ([#10081](https://github.com/paritytech/parity-ethereum/pull/10081))" ([#10105](https://github.com/paritytech/parity-ethereum/pull/10105))
- Fix left over small grumbles on whitespaces ([#10084](https://github.com/paritytech/parity-ethereum/pull/10084))
- Add --frozen when running cargo ([#10081](https://github.com/paritytech/parity-ethereum/pull/10081))
- Fix pubsub new_blocks notifications to include all blocks ([#9987](https://github.com/paritytech/parity-ethereum/pull/9987))
- Update some dependencies for compilation with pc-windows-gnu ([#10082](https://github.com/paritytech/parity-ethereum/pull/10082))
- Fill transaction hash on ethGetLog of light client. ([#9938](https://github.com/paritytech/parity-ethereum/pull/9938))
- Update changelog update for 2.2.5-beta and 2.1.10-stable ([#10064](https://github.com/paritytech/parity-ethereum/pull/10064))
- Implement len caching for parking_lot RwLock ([#10032](https://github.com/paritytech/parity-ethereum/pull/10032))
- Update parking_lot to 0.7 ([#10050](https://github.com/paritytech/parity-ethereum/pull/10050))
- Bump crossbeam. ([#10048](https://github.com/paritytech/parity-ethereum/pull/10048))
- Ethcore: enable constantinople on ethereum ([#10031](https://github.com/paritytech/parity-ethereum/pull/10031))
- Strict empty steps validation ([#10041](https://github.com/paritytech/parity-ethereum/pull/10041))
- Center the Subtitle, use some CAPS ([#10034](https://github.com/paritytech/parity-ethereum/pull/10034))
- Change test miner max memory to malloc reports. ([#10024](https://github.com/paritytech/parity-ethereum/pull/10024))
- Sort the storage for private state ([#10018](https://github.com/paritytech/parity-ethereum/pull/10018))
- Fix: test corpus_inaccessible panic ([#10019](https://github.com/paritytech/parity-ethereum/pull/10019))
- Ci: move future releases to ethereum subdir on s3 ([#10017](https://github.com/paritytech/parity-ethereum/pull/10017))
- Light(on_demand): decrease default time window to 10 secs ([#10016](https://github.com/paritytech/parity-ethereum/pull/10016))
- Light client : failsafe crate (circuit breaker) ([#9790](https://github.com/paritytech/parity-ethereum/pull/9790))
- Lencachingmutex ([#9988](https://github.com/paritytech/parity-ethereum/pull/9988))
- Version and notification for private contract wrapper added ([#9761](https://github.com/paritytech/parity-ethereum/pull/9761))
- Handle failing case for update account cache in require ([#9989](https://github.com/paritytech/parity-ethereum/pull/9989))
- Add tokio runtime to ethcore io worker ([#9979](https://github.com/paritytech/parity-ethereum/pull/9979))
- Move daemonize before creating account provider ([#10003](https://github.com/paritytech/parity-ethereum/pull/10003))
- Docs: update changelogs ([#9990](https://github.com/paritytech/parity-ethereum/pull/9990))
- Fix daemonize ([#10000](https://github.com/paritytech/parity-ethereum/pull/10000))
- Fix Bloom migration ([#9992](https://github.com/paritytech/parity-ethereum/pull/9992))
- Remove tendermint engine support ([#9980](https://github.com/paritytech/parity-ethereum/pull/9980))
- Calculate gas for deployment transaction ([#9840](https://github.com/paritytech/parity-ethereum/pull/9840))
- Fix unstable peers and slowness in sync ([#9967](https://github.com/paritytech/parity-ethereum/pull/9967))
- Adds parity_verifySignature RPC method ([#9507](https://github.com/paritytech/parity-ethereum/pull/9507))
- Improve block and transaction propagation ([#9954](https://github.com/paritytech/parity-ethereum/pull/9954))
- Deny unknown fields for chainspec ([#9972](https://github.com/paritytech/parity-ethereum/pull/9972))
- Fix docker build ([#9971](https://github.com/paritytech/parity-ethereum/pull/9971))
- Ci: rearrange pipeline by logic ([#9970](https://github.com/paritytech/parity-ethereum/pull/9970))
- Add changelogs for 2.0.9, 2.1.4, 2.1.6, and 2.2.1 ([#9963](https://github.com/paritytech/parity-ethereum/pull/9963))
- Add Error message when sync is still in progress. ([#9475](https://github.com/paritytech/parity-ethereum/pull/9475))
- Make CALLCODE to trace value to be the code address ([#9881](https://github.com/paritytech/parity-ethereum/pull/9881))
- Fix light client informant while syncing ([#9932](https://github.com/paritytech/parity-ethereum/pull/9932))
- Add a optional json dump state to evm-bin ([#9706](https://github.com/paritytech/parity-ethereum/pull/9706))
- Disable EIP-98 transition by default ([#9955](https://github.com/paritytech/parity-ethereum/pull/9955))
- Remove secret_store runtimes. ([#9888](https://github.com/paritytech/parity-ethereum/pull/9888))
- Fix a deadlock ([#9952](https://github.com/paritytech/parity-ethereum/pull/9952))
- Chore(eip712): remove unused `failure-derive` ([#9958](https://github.com/paritytech/parity-ethereum/pull/9958))
- Do not use the home directory as the working dir in docker ([#9834](https://github.com/paritytech/parity-ethereum/pull/9834))
- Prevent silent errors in daemon mode, closes [#9367](https://github.com/paritytech/parity-ethereum/issues/9367) ([#9946](https://github.com/paritytech/parity-ethereum/pull/9946))
- Fix empty steps ([#9939](https://github.com/paritytech/parity-ethereum/pull/9939))
- Adjust requests costs for light client ([#9925](https://github.com/paritytech/parity-ethereum/pull/9925))
- Eip-1186: add `eth_getProof` RPC-Method ([#9001](https://github.com/paritytech/parity-ethereum/pull/9001))
- Missing blocks in filter_changes RPC ([#9947](https://github.com/paritytech/parity-ethereum/pull/9947))
- Allow rust-nightly builds fail in nightly builds ([#9944](https://github.com/paritytech/parity-ethereum/pull/9944))
- Update eth-secp256k1 to include fix for BSDs ([#9935](https://github.com/paritytech/parity-ethereum/pull/9935))
- Unbreak build on rust -stable ([#9934](https://github.com/paritytech/parity-ethereum/pull/9934))
- Keep existing blocks when restoring a Snapshot ([#8643](https://github.com/paritytech/parity-ethereum/pull/8643))
- Add experimental RPCs flag ([#9928](https://github.com/paritytech/parity-ethereum/pull/9928))
- Clarify poll lifetime ([#9922](https://github.com/paritytech/parity-ethereum/pull/9922))
- Docs(require rust 1.30) ([#9923](https://github.com/paritytech/parity-ethereum/pull/9923))
- Use block header for building finality ([#9914](https://github.com/paritytech/parity-ethereum/pull/9914))
- Simplify cargo audit ([#9918](https://github.com/paritytech/parity-ethereum/pull/9918))
- Light-fetch: Differentiate between out-of-gas/manual throw and use required gas from response on failure ([#9824](https://github.com/paritytech/parity-ethereum/pull/9824))
- Eip 191 ([#9701](https://github.com/paritytech/parity-ethereum/pull/9701))
- Fix(logger): `reqwest` no longer a dependency ([#9908](https://github.com/paritytech/parity-ethereum/pull/9908))
- Remove rust-toolchain file ([#9906](https://github.com/paritytech/parity-ethereum/pull/9906))
- Foundation: 6692865, ropsten: 4417537, kovan: 9363457 ([#9907](https://github.com/paritytech/parity-ethereum/pull/9907))
- Ethcore: use Machine::verify_transaction on parent block ([#9900](https://github.com/paritytech/parity-ethereum/pull/9900))
- Chore(rpc-tests): remove unused rand ([#9896](https://github.com/paritytech/parity-ethereum/pull/9896))
- Fix: Intermittent failing CI due to addr in use ([#9885](https://github.com/paritytech/parity-ethereum/pull/9885))
- Chore(bump docopt): 0.8 -> 1.0 ([#9889](https://github.com/paritytech/parity-ethereum/pull/9889))
- Use expect ([#9883](https://github.com/paritytech/parity-ethereum/pull/9883))
- Use Weak reference in PubSubClient ([#9886](https://github.com/paritytech/parity-ethereum/pull/9886))
- Ci: nuke the gitlab caches ([#9855](https://github.com/paritytech/parity-ethereum/pull/9855))
- Remove unused code ([#9884](https://github.com/paritytech/parity-ethereum/pull/9884))
- Fix json tracer overflow ([#9873](https://github.com/paritytech/parity-ethereum/pull/9873))
- Allow to seal work on latest block ([#9876](https://github.com/paritytech/parity-ethereum/pull/9876))
- Fix docker script ([#9854](https://github.com/paritytech/parity-ethereum/pull/9854))
- Health endpoint ([#9847](https://github.com/paritytech/parity-ethereum/pull/9847))
- Gitlab-ci: make android release build succeed ([#9743](https://github.com/paritytech/parity-ethereum/pull/9743))
- Clean up existing benchmarks ([#9839](https://github.com/paritytech/parity-ethereum/pull/9839))
- Update Callisto block reward code to support HF1 ([#9811](https://github.com/paritytech/parity-ethereum/pull/9811))
- Option to disable keep alive for JSON-RPC http transport ([#9848](https://github.com/paritytech/parity-ethereum/pull/9848))
- Classic.json Bootnode Update ([#9828](https://github.com/paritytech/parity-ethereum/pull/9828))
- Support MIX. ([#9767](https://github.com/paritytech/parity-ethereum/pull/9767))
- Ci: remove failing tests for android, windows, and macos ([#9788](https://github.com/paritytech/parity-ethereum/pull/9788))
- Implement NoProof for json tests and update tests reference (replaces [#9744](https://github.com/paritytech/parity-ethereum/issues/9744)) ([#9814](https://github.com/paritytech/parity-ethereum/pull/9814))
- Chore(bump regex) ([#9842](https://github.com/paritytech/parity-ethereum/pull/9842))
- Ignore global cache for patched accounts ([#9752](https://github.com/paritytech/parity-ethereum/pull/9752))
- Move state root verification before gas used ([#9841](https://github.com/paritytech/parity-ethereum/pull/9841))
- Fix(docker-aarch64) : cross-compile config ([#9798](https://github.com/paritytech/parity-ethereum/pull/9798))
- Version: bump nightly to 2.3.0 ([#9819](https://github.com/paritytech/parity-ethereum/pull/9819))
- Tests modification for windows CI ([#9671](https://github.com/paritytech/parity-ethereum/pull/9671))
- Eip-712 implementation ([#9631](https://github.com/paritytech/parity-ethereum/pull/9631))
- Fix typo ([#9826](https://github.com/paritytech/parity-ethereum/pull/9826))
- Clean up serde rename and use rename_all = camelCase when possible ([#9823](https://github.com/paritytech/parity-ethereum/pull/9823))
