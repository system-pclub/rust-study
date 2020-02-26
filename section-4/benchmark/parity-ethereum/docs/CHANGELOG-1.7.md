Note: Parity 1.7 reached End-of-Life on 2018-01-25 (EOL).

### Parity [v1.7.13](https://github.com/paritytech/parity/releases/tag/v1.7.13) (2018-01-23)

Parity 1.7.13 is a bug-fix release to improve stability of PoA-networks. Users on Kovan or other Aura-based networks are advised to upgrade as this release fixes an issue introduced with 1.7.12 that causes Proof-of-Authority nodes to stop synchronizing the chain.

The full list of included changes:

- AuRa fix for 1.7.x series ([#7666](https://github.com/paritytech/parity/pull/7666))
  - Fix Temporarily Invalid blocks handling ([#7613](https://github.com/paritytech/parity/pull/7613))
    - Handle temporarily invalid blocks in sync.
    - Fix tests.
    - Bump rustc-serialize
    - Bump version.
    - Update .gitlab-ci.yml
    - Fix lint
    - Remove slash from gitlab ci script to fix builds
    - Start build.

### Parity [v1.7.12](https://github.com/paritytech/parity/releases/tag/v1.7.12) (2018-01-09)

Parity 1.7.12 is a bug-fix release to improve performance and stability.

The full list of included changes:

- Fix stable builds for rustc 1.23.0 ([#7504](https://github.com/paritytech/parity/pull/7504))
- Missing AuRa backports ([#7499](https://github.com/paritytech/parity/pull/7499)
  - Wait for future blocks in AuRa ([#7368](https://github.com/paritytech/parity/pull/7368))
    - Mark future blocks as temporarily invalid.
    - Don't check max.
  - Advance AuRa step as far as we can and prevent invalid blocks. ([#7451](https://github.com/paritytech/parity/pull/7451))
    - Advance AuRa step as far as we can.
    - Wait for future blocks.
  -  Problem: AuRa's unsafeties around step duration ([#7282](https://github.com/paritytech/parity/pull/7282))
  - Fix tests.
  - Detect different node, same-key signing in aura ([#7245](https://github.com/paritytech/parity/pull/7245))
    - Detect different node, same-key signing in aura
    - Reduce scope of warning
- Backports ([#7496](https://github.com/paritytech/parity/pull/7496))
  - Advance AuRa step as far as we can. ([#7451](https://github.com/paritytech/parity/pull/7451))
    - Advance AuRa step as far as we can.
    - Wait for future blocks.
  - Fixed panic when io is not available for export block, closes [#7486](https://github.com/paritytech/parity/issue/7486) ([#7495](https://github.com/paritytech/parity/pull/7495))
  - Update Parity Mainnet Bootnodes ([#7476](https://github.com/paritytech/parity/pull/7476))
    - Replace the Azure HDD bootnodes with the new ones :)
  - Bump version to 1.7.12

### Parity [v1.7.11](https://github.com/paritytech/parity/releases/tag/v1.7.11) (2017-12-29)

Parity 1.7.11 changes the default behavior of JSON-RPC CORS setting, and updates bootnodes for the Kovan and Foundation networks.

Note: The default value of `--jsonrpc-cors` option has been altered to disallow (potentially malicious) websites from accessing the low-sensitivity RPCs (viewing exposed accounts, proposing transactions for signing). Currently domains need to be whitelisted manually. To bring back previous behaviour run with `--jsonrpc-cors all` or `--jsonrpc-cors http://example.com`.

The full list of included changes:

- Stable Bootnodes and Warpnodes ([#7298](https://github.com/paritytech/parity/pull/7298))
  - New warp enodes ([#7287](https://github.com/paritytech/parity/pull/7287))
    - New warp enodes
    - Added one more warp enode; replaced spaces with tabs
    - Bump stable to 1.7.11
  - Update kovan boot nodes ([#7296](https://github.com/paritytech/parity/pull/7296))
  - Fix Cargo.lock
  - Updating mainnet bootnodes.
  - Update bootnodes ([#7363](https://github.com/paritytech/parity/pull/7363))
    - Updating mainnet bootnodes.
    - Add additional parity-beta bootnodes.
    - Restore old parity bootnodes and update foudation bootnodes
- Ethstore optimizations ([#6827](https://github.com/paritytech/parity/pull/6827)) ([#6844](https://github.com/paritytech/parity/pull/6844)) ([#7347](https://github.com/paritytech/parity/pull/7347))
- Fix default CORS. ([#7389](https://github.com/paritytech/parity/pull/7389))

### Parity [v1.7.10](https://github.com/paritytech/parity/releases/tag/v1.7.10) (2017-12-11)

Parity 1.7.10 applies fixes for Proof-of-Authority networks and schedules the Kovan-Byzantium hard-fork.

- The Kovan testnet will fork on block `5067000` at `Thu Dec 14 2017 05:40:03 UTC`.
  - This enables Byzantium features on Kovan.
  - This disables uncles on Kovan for stability reasons.
- Proof-of-Authority networks are advised to set `maximumUncleCount` to 0 in a future `maximumUncleCountTransition` for stability reasons. See the [Kovan chain spec](https://github.com/paritytech/parity/blob/master/ethcore/res/ethereum/kovan.json) for an example. New PoA networks created with Parity will have this feature enabled by default.

The full list of included changes:

- Backports and HF block update ([#7243](https://github.com/paritytech/parity/pull/7243))
  - Reduce max block timestamp drift to 15 seconds ([#7240](https://github.com/paritytech/parity/pull/7240))
  - Add test for block timestamp validation within allowed drift
  - Update kovan HF block number. ([#7259](https://github.com/paritytech/parity/pull/7259))
- [stable] Backports and Kovan HF ([#7235](https://github.com/paritytech/parity/pull/7235))
  - Escape inifinite loop in estimte_gas ([#7075](https://github.com/paritytech/parity/pull/7075))
  - Disable uncles by default ([#7006](https://github.com/paritytech/parity/pull/7006))
  - Maximum uncle count transition ([#7196](https://github.com/paritytech/parity/pull/7196))
    - Enable delayed maximum_uncle_count activation.
    - Fix tests.
    - Defer kovan HF.
  - Bump version.
  - Kovan HF.
  - Update Kovan HF block.
  - Fix compilation issues.
  - Fix aura test.
  - Add missing byzantium builtins.
  - Fix tests.
  - Bump version for installers.
  - Increase allowed time drift to 10s. ([#7238](https://github.com/paritytech/parity/pull/7238))

### Parity [v1.7.9](https://github.com/paritytech/parity/releases/tag/v1.7.9) (2017-11-14)

Parity 1.7.9 removes the ability to deploy built-in multi-signature wallets.

The full list of included changes:

- Bump to v1.7.9 ([#7047](https://github.com/paritytech/parity/pull/7047))
- Disallow built-in multi-sig deploy (only watch) ([#7017](https://github.com/paritytech/parity/pull/7017))

### Parity [v1.7.8](https://github.com/paritytech/parity/releases/tag/v1.7.8) (2017-10-26)

Parity 1.7.8 fixes a critical Byzantium consensus issue. Update is highly recommended.

The full list of included changes:

- Refactor static context check in CREATE ([#6889](https://github.com/paritytech/parity/pull/6889))
- Bump to v1.7.8 ([#6890](https://github.com/paritytech/parity/pull/6890))

## Parity [v1.7.7](https://github.com/paritytech/parity/releases/tag/v1.7.7) (2017-10-15)

Parity 1.7.7 fixes an issue with auto-update system. Updating is recommended, but not required for Byzantium.

The full list of included changes:

- Fix auto-update ([#6769](https://github.com/paritytech/parity/pull/6759))
  - Bump to v1.7.7
  - Updated ethabi to fix auto-update
- Bumped fork block number for auto-update ([#6754](https://github.com/paritytech/parity/pull/6754))

## Parity [v1.7.6](https://github.com/paritytech/parity/releases/tag/v1.7.6) (2017-10-13)

Parity 1.7.6 includes a critical consensus-relevant fix for the Byzantium hard-fork. Please upgrade your Ethereum client before block number `4_370_000`.

The full list of included changes:

- Fixed modexp gas calculation overflow ([#6746](https://github.com/paritytech/parity/pull/6746))
 - Fixed modexp gas calculation overflow ([#6741](https://github.com/paritytech/parity/pull/6741))
 - Bump to v1.7.6

## Parity [v1.7.5](https://github.com/paritytech/parity/releases/tag/v1.7.5) (2017-10-12)

Parity 1.7.5 includes a critical consensus-relevant fix for the Byzantium hard-fork. Please upgrade your Ethereum client before block number `4_370_000`.

Parity 1.7.5 is the first stable release of the 1.7 branch. With this release the support for 1.6 releases ends. Please upgrade your stable nodes to 1.7.5.

The full list of included changes:

- Backport stable - Fixes Badges ([#6731](https://github.com/paritytech/parity/pull/6731))
  - Fix badges not showing up ([#6730](https://github.com/paritytech/parity/pull/6730))
  - Always fetch meta data first [badges]
- Backport ([#6726](https://github.com/paritytech/parity/pull/6726))
  - Check vouch status on appId in addition to contentHash ([#6719](https://github.com/paritytech/parity/pull/6719))
    - Check vouch status on appId in addition to contentHash
    - Simplify var expansion
  - Merge [#6725](https://github.com/paritytech/parity/pull/6725)
    - Update new token fetching
    - Working Certifications Monitoring
    - Update on Certification / Revoke
    - Fix none-fetched tokens value display
    - Fix tests
  - Add updated MethodDecoding from master
- v1.7.5 stabilized
- Backport ([#6724](https://github.com/paritytech/parity/pull/6724))
  - Fixed RETURNDATA out of bounds check ([#6718](https://github.com/paritytech/parity/pull/6718))
  - Prevent going offline when restoring or taking snapshot ([#6694](https://github.com/paritytech/parity/pull/6694))
- Bump to v1.7.5
- Trigger beta js build & release ([#6721](https://github.com/paritytech/parity/pull/6721))

## Parity [v1.7.4](https://github.com/paritytech/parity/releases/tag/v1.7.4) (2017-10-11)

Parity 1.7.4 includes a critical consensus-relevant fix for the Byzantium hard-fork. Please upgrade your Ethereum client before block number `4_370_000`.

The full list of included changes:

- Backport ([#6715](https://github.com/paritytech/parity/pull/6715))
  - Fix estimate gas if from is not provided. ([#6714](https://github.com/paritytech/parity/pull/6714))
  - Display vouched overlay on dapps ([#6710](https://github.com/paritytech/parity/pull/6710))
    - Add vouch overlays to dapps
    - Cleanup address
    - Only run where we have a contentHash
- Backporting ([#6712](https://github.com/paritytech/parity/pull/6712))
  - Bump to v1.7.4
  - Fixed potential exp len overflow ([#6686](https://github.com/paritytech/parity/pull/6686))
  - Fix warp sync blockers detection ([#6691](https://github.com/paritytech/parity/pull/6691))
- Backport ([#6713](https://github.com/paritytech/parity/pull/6713))
  - Allow signer signing display of markdown ([#6707](https://github.com/paritytech/parity/pull/6707))
  - Fix default values for address input ([#6701](https://github.com/paritytech/parity/pull/6701))
  - Fix asciiToHex for characters < 0x10 ([#6702](https://github.com/paritytech/parity/pull/6702))

## Parity [v1.7.3](https://github.com/paritytech/parity/releases/tag/v1.7.3) (2017-10-09)

Parity 1.7.3 enables the Byzantium fork for Ethereum main network on Block 4_370_000 and offers a variety of bug fixes and stability improvements. Among them:

- Fixed network protocol version negotiation with Geth nodes v1.7.1+.
- Fixed `RETURNDATA` size for built-ins. (Built-ins in some cases overwrite only a portion of the output memory slice.)
- Multisig Wallet View now loads if multiple transactions happened within one block.
- Improved stability of snapshot-sycns (warp).
- Revised timeout and batch size constants for bigger blocks.
- Renamed RPC receipt `statusCode` field to `status`.

The full list of included changes:

- Backporting ([#6676](https://github.com/paritytech/parity/pull/6676))
  - Fix wallet view ([#6597](https://github.com/paritytech/parity/pull/6597))
    - Add safe fail for empty logs
    - Filter transactions
    - Add more logging
    - Fix Wallet Creation and wallet tx list
    - Remove logs
    - Prevent selecting twice same wallet owner
    - Fix tests
    - Remove unused props
  - Disallow pasting recovery phrases on first run ([#6602](https://github.com/paritytech/parity/pull/6602))
    - Fix disallowing paste of recovery phrase on first run, ref [#6581](https://github.com/paritytech/parity/issues/6581)
    - Allow the leader of CATS pasting recovery phrases.
  - Updated systemd files for linux ([#6592](https://github.com/paritytech/parity/pull/6592))
    - Previous version put $BASE directory in root directory.
    - This version clearly explains how to run as root or as specific user.
    - Additional configuration:
      - send SIGHUP for clean exit,
      - restart on fail.
    - Tested on Ubuntu 16.04.3 LTS with 4.10.0-33-generic x86_64 kernel
  - Don't expose port 80 for parity anymore ([#6633](https://github.com/paritytech/parity/pull/6633))
- Backporting ([#6675](https://github.com/paritytech/parity/pull/6675))
  - Required validators >= num owners ([#6551](https://github.com/paritytech/parity/pull/6551))
  - Debounce sync status. ([#6572](https://github.com/paritytech/parity/pull/6572))
  - Fixed network protocol version negotiation ([#6649](https://github.com/paritytech/parity/pull/6649))
  - Renamed RPC receipt statusCode field to status ([#6650](https://github.com/paritytech/parity/pull/6650))
  - Fixed RETURNDATA size for built-ins ([#6652](https://github.com/paritytech/parity/pull/6652))
- Byzantium fork block number ([#6661](https://github.com/paritytech/parity/pull/6661))
- Refreshing block number on status view ([#6610](https://github.com/paritytech/parity/pull/6610))
- Tweaked block download timeouts ([#6595](https://github.com/paritytech/parity/pull/6595))
- Backports ([#6563](https://github.com/paritytech/parity/pull/6563))
  - Sync progress and error handling fixes ([#6560](https://github.com/paritytech/parity/pull/6560))
  - Fixed receipt serialization and RPC ([#6555](https://github.com/paritytech/parity/pull/6555))
- Bump to v1.7.3

## Parity [v1.7.2](https://github.com/paritytech/parity/releases/tag/v1.7.2) (2017-09-18)

Parity 1.7.2 is a bug-fix release to improve performance and stability. Among others, it addresses the following:

- Byzantium fork support for the Ropsten and Foundation networks.
- Added support for the ConsenSys and Gnosis multi-signature wallets.
- Significantly increased token registry and token balance lookup performance.
- Fixed issues with the health status indicator in the wallet.
- Tweaked warp-sync to quickly catch up with chains fallen back more than 10,000 blocks.
- Fixes to the Chrome extension and macOS installer upgrades.

The full list of included changes:

- Fix output from eth_call. ([#6538](https://github.com/paritytech/parity/pull/6538))
- Ropsten fork ([#6532](https://github.com/paritytech/parity/pull/6532))
- Byzantium updates ([#6529](https://github.com/paritytech/parity/pull/6529))
  - Fix modexp bug: return 0 if base=0 ([#6424](https://github.com/paritytech/parity/pull/6424))
  - Running state test using parity-evm ([#6355](https://github.com/paritytech/parity/pull/6355))
    - Initial version of state tests.
    - Refactor state to support tracing.
    - Unify TransactResult.
    - Add test.
  - Byzantium updates ([#5855](https://github.com/paritytech/parity/pull/5855))
    - EIP-211 updates
    - Benchmarks
    - Blockhash instruction gas cost updated
    - More benches
    - EIP-684
    - EIP-649
    - EIP-658
    - Updated some tests
    - Modexp fixes
    - STATICCALL fixes
    - Pairing fixes
    - More STATICALL fixes
    - Use paritytech/bn
    - Fixed REVERTing of contract creation
    - Fixed more tests
    - Fixed more tests
    - Blockchain tests
    - Enable previously broken tests
    - Transition test
    - Updated tests
    - Fixed modexp reading huge numbers
    - Enabled max_code_size test
    - Review fixes
    - Updated pairing pricing
    - Missing commas (style)
    - Update test.rs
    - Small improvements
    - Eip161abc
- Fix extension detection ([#6452](https://github.com/paritytech/parity/pull/6452)) ([#6524](https://github.com/paritytech/parity/pull/6524))
  - Fix extension detection.
  - Fix mobx quirks.
  - Update submodule.
- Fix detecting hardware wallets. ([#6509](https://github.com/paritytech/parity/pull/6509))
- Allow hardware device reads without lock. ([#6517](https://github.com/paritytech/parity/pull/6517))
- Backports [#6497](https://github.com/paritytech/parity/pull/6497)
  - Fix slow balances ([#6471](https://github.com/paritytech/parity/pull/6471))
    - Update token updates
    - Update token info fetching
    - Update logger
    - Minor fixes to updates and notifications for balances
    - Use Pubsub
    - Fix timeout.
    - Use pubsub for status.
    - Fix signer subscription.
    - Process tokens in chunks.
    - Fix tokens loaded by chunks
    - Dispatch tokens asap
    - Fix chunks processing.
    - Better filter options
    - Parallel log fetching.
    - Fix signer polling.
    - Fix initial block query.
    - Token balances updates : the right(er) way
    - Better tokens info fetching
    - Fixes in token data fetching
    - Only fetch what's needed (tokens)
    - Fix linting issues
    - Update wasm-tests.
    - Fixing balances fetching
    - Fix requests tracking in UI
    - Fix request watching
    - Update the Logger
    - PR Grumbles Fixes
  - Eth_call returns output of contract creations ([#6420](https://github.com/paritytech/parity/pull/6420))
    - Eth_call returns output of contract creations
    - Fix parameters order.
    - Save outputs for light client as well.
  - Don't accept transactions above block gas limit.
  - Expose health status over RPC ([#6274](https://github.com/paritytech/parity/pull/6274))
     - Node-health to a separate crate.
     - Initialize node_health outside of dapps.
     - Expose health over RPC.
     - Bring back 412 and fix JS.
     - Add health to workspace and tests.
     - Fix compilation without default features.
     - Fix borked merge.
     - Revert to generics to avoid virtual calls.
     - Fix node-health tests.
     - Add missing trailing comma.
  - Fixing/removing failing JS tests.
  - Do not activate genesis epoch in immediate transition validator contract ([#6349](https://github.com/paritytech/parity/pull/6349))
  - Fix memory tracing.
  - Add test to cover that.
  - Ensure balances of constructor accounts are kept
  - Test balance of spec-constructed account is kept
- Fix warning spam. [#6369](https://github.com/paritytech/parity/pull/6369)
- Bump to 1.7.2
- Fix eth_call [#6366](https://github.com/paritytech/parity/pull/6366)
- Backporting [#6352](https://github.com/paritytech/parity/pull/6352)
  - Better check the created accounts before showing Startup Wizard [#6331](https://github.com/paritytech/parity/pull/6331)
  - Tweaked snapshot params [#6344](https://github.com/paritytech/parity/pull/6344)
- Increase default gas limit for eth_call [#6337](https://github.com/paritytech/parity/pull/6337)
  - Fix balance increase.
  - Cap gas limit for dapp-originating requests.
- Backports [#6333](https://github.com/paritytech/parity/pull/6333)
  - Overflow check in addition
  - Unexpose methods on UI RPC. [#6295](https://github.com/paritytech/parity/pull/6295)
  - Add more descriptive error when signing/decrypting using hw wallet.
  - Format instant change proofs correctly
  - Propagate stratum submit share error upstream [#6260](https://github.com/paritytech/parity/pull/6260)
  - Updated jsonrpc [#6264](https://github.com/paritytech/parity/pull/6264)
  - Using multiple NTP servers [#6173](https://github.com/paritytech/parity/pull/6173)
    - Small improvements to time estimation.
    - Allow multiple NTP servers to be used.
    - Removing boxing.
    - Update list of servers and add reference.
  - Fix dapps CSP when UI is exposed externally [#6178](https://github.com/paritytech/parity/pull/6178)
    - Allow embeding on any page when ui-hosts=all and fix dev_ui
  - Fix cache path when using --base-path [#6212](https://github.com/paritytech/parity/pull/6212)
  - Bump to v1.7.1
- UI backports [#6332](https://github.com/paritytech/parity/pull/6332)
  - Time should not contribue to overall status. [#6276](https://github.com/paritytech/parity/pull/6276)
  - Add warning to web browser and fix links. [#6232](https://github.com/paritytech/parity/pull/6232)
  - Extension fixes [#6284](https://github.com/paritytech/parity/pull/6284)
    - Fix token symbols in extension.
    - Allow connections from firefox extension.
  - Add support for ConsenSys multisig wallet [#6153](https://github.com/paritytech/parity/pull/6153)
    - First draft of ConsenSys wallet
    - Fix transfer store // WIP Consensys Wallet
    - Rename walletABI JSON file
    - Fix wrong daylimit in wallet modal
    - Confirm/Revoke ConsensysWallet txs
    - Change of settings for the Multisig Wallet
- Update README for beta [#6270](https://github.com/paritytech/parity/pull/6270)
- Fixed macOS installer upgrade [#6221](https://github.com/paritytech/parity/pull/6221)

## Parity [v1.7.0](https://github.com/paritytech/parity/releases/tag/v1.7.0) (2017-07-28)

Parity 1.7.0 is a major release introducing several important features:

- **Experimental [Light client](https://github.com/paritytech/parity/wiki/The-Parity-Light-Protocol-(PIP)) support**. Start Parity with `--light` to enable light mode. Please, note: The wallet UI integration for the light client is not included, yet.
- **Experimental web wallet**. A hosted version of Parity that keeps the keys and signs transactions using your browser storage. Try it at https://wallet.parity.io or run your own with `--public-node`.
- **WASM contract support**. Private networks can run contracts compiled into WASM bytecode. _More information and documentation to follow_.
- **DApps and RPC server merge**. DApp and RPC are now available through a single API endpoint. DApp server related settings are deprecated.
- **Export accounts from the wallet**. Backing up your keys can now simply be managed through the wallet interface.
- **PoA/Kovan validator set contract**. The PoA network validator-set management via smart contract is now supported by warp and, in the near future, light sync.
- **PubSub API**. https://github.com/paritytech/parity/wiki/JSONRPC-Parity-Pub-Sub-module
- **Signer apps for IOS and Android**.

The full list of included changes:

- Backports [#6163](https://github.com/paritytech/parity/pull/6163)
  - Light client improvements ([#6156](https://github.com/paritytech/parity/pull/6156))
    - No seal checking
    - Import command and --no-seal-check for light client
    - Fix eth_call
    - Tweak registry dapps lookup
    - Ignore failed requests to non-server peers
  - Fix connecting to wildcard addresses. ([#6167](https://github.com/paritytech/parity/pull/6167))
  - Don't display an overlay in case the time sync check fails. ([#6164](https://github.com/paritytech/parity/pull/6164))
    - Small improvements to time estimation.
    - Temporarily disable NTP time check by default.
- Light client fixes ([#6148](https://github.com/paritytech/parity/pull/6148)) [#6151](https://github.com/paritytech/parity/pull/6151)
  - Light client fixes
  - Fix memory-lru-cache
  - Clear pending reqs on disconnect
- Filter tokens logs from current block, not genesis ([#6128](https://github.com/paritytech/parity/pull/6128)) [#6141](https://github.com/paritytech/parity/pull/6141)
- Fix QR scanner returning null on confirm [#6122](https://github.com/paritytech/parity/pull/6122)
- Check QR before lowercase ([#6119](https://github.com/paritytech/parity/pull/6119)) [#6120](https://github.com/paritytech/parity/pull/6120)
- Remove chunk to restore from pending set only upon successful import [#6117](https://github.com/paritytech/parity/pull/6117)
- Fixed node address detection on incoming connection [#6094](https://github.com/paritytech/parity/pull/6094)
- Place RETURNDATA behind block number gate [#6095](https://github.com/paritytech/parity/pull/6095)
- Update wallet library binaries [#6108](https://github.com/paritytech/parity/pull/6108)
- Backported wallet fix [#6105](https://github.com/paritytech/parity/pull/6105)
  - Fix initialisation bug. ([#6102](https://github.com/paritytech/parity/pull/6102))
  - Update wallet library modifiers ([#6103](https://github.com/paritytech/parity/pull/6103))
- Place RETURNDATA behind block number gate [#6095](https://github.com/paritytech/parity/pull/6095)
- Fixed node address detection on incoming connection [#6094](https://github.com/paritytech/parity/pull/6094)
- Bump snap version and tweak importing detection logic ([#6079](https://github.com/paritytech/parity/pull/6079)) [#6081](https://github.com/paritytech/parity/pull/6081)
  - bump last tick just before printing info and restore sync detection
  - bump kovan snapshot version
  - Fixed sync tests
  - Fixed rpc tests
- Acquire client report under lock in informant [#6071](https://github.com/paritytech/parity/pull/6071)
- Show busy indicator on Address forget [#6069](https://github.com/paritytech/parity/pull/6069)
- Add CSP for worker-src ([#6059](https://github.com/paritytech/parity/pull/6059)) [#6064](https://github.com/paritytech/parity/pull/6064)
  - Specify worker-src seperately, add blob
  - Upgrade react-qr-scan to latest version
- Set release channel to beta
- Limit transaction queue memory & limit future queue [#6038](https://github.com/paritytech/parity/pull/6038)
- Fix CI build issue [#6050](https://github.com/paritytech/parity/pull/6050)
- New contract PoA sync fixes [#5991](https://github.com/paritytech/parity/pull/5991)
- Fixed link to Multisig Contract Wallet on master [#5984](https://github.com/paritytech/parity/pull/5984)
- Ethcore crate split part 1 [#6041](https://github.com/paritytech/parity/pull/6041)
- Fix status icon [#6039](https://github.com/paritytech/parity/pull/6039)
- Errors & warnings for inappropriate RPCs [#6029](https://github.com/paritytech/parity/pull/6029)
- Add missing CSP for web3.site [#5992](https://github.com/paritytech/parity/pull/5992)
- Remove cargo install --git from README.md [#6037](https://github.com/paritytech/parity/pull/6037)
- Node Health warnings [#5951](https://github.com/paritytech/parity/pull/5951)
- RPC cpu pool [#6023](https://github.com/paritytech/parity/pull/6023)
- Use crates.io dependencies for parity-wasm [#6036](https://github.com/paritytech/parity/pull/6036)
- Add test for loading the chain specs [#6028](https://github.com/paritytech/parity/pull/6028)
- Whitelist APIs for generic Pub-Sub [#5840](https://github.com/paritytech/parity/pull/5840)
- WASM contracts MVP [#5679](https://github.com/paritytech/parity/pull/5679)
- Fix valid QR scan not advancing [#6033](https://github.com/paritytech/parity/pull/6033)
- --reseal-on-uncle [#5940](https://github.com/paritytech/parity/pull/5940)
- Support comments in reserved peers file ([#6004](https://github.com/paritytech/parity/pull/6004)) [#6012](https://github.com/paritytech/parity/pull/6012)
- Add new md tnc [#5937](https://github.com/paritytech/parity/pull/5937)
- Fix output of parity-evm in case of bad instruction [#5955](https://github.com/paritytech/parity/pull/5955)
- Don't send notifications to unsubscribed clients of PubSub [#5960](https://github.com/paritytech/parity/pull/5960)
- Proper light client informant and more verification of imported headers [#5897](https://github.com/paritytech/parity/pull/5897)
- New Kovan bootnodes [#6017](https://github.com/paritytech/parity/pull/6017)
- Use standard paths for Ethash cache [#5881](https://github.com/paritytech/parity/pull/5881)
- Defer code hash calculation. [#5959](https://github.com/paritytech/parity/pull/5959)
- Fix first run wizard. [#6000](https://github.com/paritytech/parity/pull/6000)
- migration to serde 1.0 [#5996](https://github.com/paritytech/parity/pull/5996)
- SecretStore: generating signatures [#5764](https://github.com/paritytech/parity/pull/5764)
- bigint upgraded to version 3.0 [#5986](https://github.com/paritytech/parity/pull/5986)
- config: don't allow dev chain with force sealing option [#5965](https://github.com/paritytech/parity/pull/5965)
- Update lockfile for miniz-sys and gcc [#5969](https://github.com/paritytech/parity/pull/5969)
- Clean up function naming in RPC error module [#5995](https://github.com/paritytech/parity/pull/5995)
- Fix underflow in gas calculation [#5975](https://github.com/paritytech/parity/pull/5975)
- PubSub for parity-js [#5830](https://github.com/paritytech/parity/pull/5830)
- Report whether a peer was kept from `Handler::on_connect` [#5958](https://github.com/paritytech/parity/pull/5958)
- Implement skeleton for transaction index and epoch transition proof PIP messages [#5908](https://github.com/paritytech/parity/pull/5908)
- TransactionQueue improvements [#5917](https://github.com/paritytech/parity/pull/5917)
- constant time HMAC comparison and clarify docs in ethkey [#5952](https://github.com/paritytech/parity/pull/5952)
- Avoid pre-computing jump destinations [#5954](https://github.com/paritytech/parity/pull/5954)
- Upgrade elastic array [#5949](https://github.com/paritytech/parity/pull/5949)
- PoA: Wait for transition finality before applying [#5774](https://github.com/paritytech/parity/pull/5774)
- Logs Pub-Sub [#5705](https://github.com/paritytech/parity/pull/5705)
- Add the command to install the parity snap [#5945](https://github.com/paritytech/parity/pull/5945)
- Reduce unnecessary allocations [#5944](https://github.com/paritytech/parity/pull/5944)
- Clarify confusing messages. [#5935](https://github.com/paritytech/parity/pull/5935)
- Content Security Policy [#5790](https://github.com/paritytech/parity/pull/5790)
- CLI: Export error message and less verbose peer counter. [#5870](https://github.com/paritytech/parity/pull/5870)
- network: make it more explicit about StreamToken and TimerToken [#5939](https://github.com/paritytech/parity/pull/5939)
- sync: make it more idiomatic rust [#5938](https://github.com/paritytech/parity/pull/5938)
- Prioritize accounts over address book [#5909](https://github.com/paritytech/parity/pull/5909)
- Fixing failing compilation of RPC test on master. [#5916](https://github.com/paritytech/parity/pull/5916)
- Empty local middleware, until explicitly requested [#5912](https://github.com/paritytech/parity/pull/5912)
- Cancel propagated TX [#5899](https://github.com/paritytech/parity/pull/5899)
- fix minor race condition in aura seal generation [#5910](https://github.com/paritytech/parity/pull/5910)
- Docs for Pub-Sub, optional parameter for parity_subscribe [#5833](https://github.com/paritytech/parity/pull/5833)
- Fix gas editor doubling-up on gas [#5820](https://github.com/paritytech/parity/pull/5820)
- Information about used paths added to general output block [#5904](https://github.com/paritytech/parity/pull/5904)
- Domain-locked web tokens. [#5894](https://github.com/paritytech/parity/pull/5894)
- Removed panic handlers [#5895](https://github.com/paritytech/parity/pull/5895)
- Latest changes from Rust RocksDB binding merged [#5905](https://github.com/paritytech/parity/pull/5905)
- Adjust keyethereum/secp256 aliasses [#5903](https://github.com/paritytech/parity/pull/5903)
- Keyethereum fs dependency [#5902](https://github.com/paritytech/parity/pull/5902)
- Ethereum Classic Monetary Policy [#5741](https://github.com/paritytech/parity/pull/5741)
- Initial token should allow full access. [#5873](https://github.com/paritytech/parity/pull/5873)
- Fixed account selection for Dapps on public node [#5856](https://github.com/paritytech/parity/pull/5856)
- blacklist bad snapshot manifest hashes upon failure [#5874](https://github.com/paritytech/parity/pull/5874)
- Fix wrongly called timeouts [#5838](https://github.com/paritytech/parity/pull/5838)
- ArchiveDB and other small fixes [#5867](https://github.com/paritytech/parity/pull/5867)
- convert try!() to ? [#5866](https://github.com/paritytech/parity/pull/5866)
- Make config file optional in systemd [#5847](https://github.com/paritytech/parity/pull/5847)
- EIP-116 (214), [#4833](https://github.com/paritytech/parity/issues/4833) [#4851](https://github.com/paritytech/parity/pull/4851)
- all executables are workspace members [#5865](https://github.com/paritytech/parity/pull/5865)
- minor optimizations of the modexp builtin [#5860](https://github.com/paritytech/parity/pull/5860)
- three small commits for HashDB and MemoryDB [#5766](https://github.com/paritytech/parity/pull/5766)
- use rust 1.18's retain to boost the purge performance [#5801](https://github.com/paritytech/parity/pull/5801)
- Allow IPFS server to accept POST requests [#5858](https://github.com/paritytech/parity/pull/5858)
- Dutch i18n from [#5802](https://github.com/paritytech/parity/issues/5802) for master [#5836](https://github.com/paritytech/parity/pull/5836)
- Typos in token deploy dapp ui [#5851](https://github.com/paritytech/parity/pull/5851)
- A CLI flag to allow fast transaction signing when account is unlocked. [#5778](https://github.com/paritytech/parity/pull/5778)
- Removing `additional` field from EVM instructions [#5821](https://github.com/paritytech/parity/pull/5821)
- Don't fail on wrong log decoding [#5813](https://github.com/paritytech/parity/pull/5813)
- Use randomized subscription ids for PubSub [#5756](https://github.com/paritytech/parity/pull/5756)
- Fixed mem write for empty slice [#5827](https://github.com/paritytech/parity/pull/5827)
- Fix party technologies [#5810](https://github.com/paritytech/parity/pull/5810)
- Revert "Fixed mem write for empty slice" [#5826](https://github.com/paritytech/parity/pull/5826)
- Fixed mem write for empty slice [#5825](https://github.com/paritytech/parity/pull/5825)
- Fix JS tests [#5822](https://github.com/paritytech/parity/pull/5822)
- Bump native-tls and openssl crates. [#5817](https://github.com/paritytech/parity/pull/5817)
- Public node using WASM [#5734](https://github.com/paritytech/parity/pull/5734)
- enforce block signer == author field in PoA [#5808](https://github.com/paritytech/parity/pull/5808)
- Fix stack display in evmbin. [#5733](https://github.com/paritytech/parity/pull/5733)
- Disable UI if it's not compiled in. [#5773](https://github.com/paritytech/parity/pull/5773)
- Require phrase confirmation. [#5731](https://github.com/paritytech/parity/pull/5731)
- Duration limit made optional for EthashParams [#5777](https://github.com/paritytech/parity/pull/5777)
- Update Changelog for 1.6.8 [#5798](https://github.com/paritytech/parity/pull/5798)
- Replace Ethcore comany name in T&C and some other places [#5796](https://github.com/paritytech/parity/pull/5796)
- PubSub for IPC. [#5800](https://github.com/paritytech/parity/pull/5800)
- Fix terminology distributed -> decentralized applications [#5797](https://github.com/paritytech/parity/pull/5797)
- Disable compression for RLP strings [#5786](https://github.com/paritytech/parity/pull/5786)
- update the source for the snapcraft package [#5781](https://github.com/paritytech/parity/pull/5781)
- Fixed default UI port for mac installer [#5782](https://github.com/paritytech/parity/pull/5782)
- Block invalid account name creation [#5784](https://github.com/paritytech/parity/pull/5784)
- Update Cid/multihash/ring/tinykeccak [#5785](https://github.com/paritytech/parity/pull/5785)
- use NULL_RLP, remove NULL_RLP_STATIC [#5742](https://github.com/paritytech/parity/pull/5742)
- Blacklist empty phrase account. [#5730](https://github.com/paritytech/parity/pull/5730)
- EIP-211 RETURNDATACOPY and RETURNDATASIZE [#5678](https://github.com/paritytech/parity/pull/5678)
- Bump mio [#5763](https://github.com/paritytech/parity/pull/5763)
- Fixing UI issues after UI server refactor [#5710](https://github.com/paritytech/parity/pull/5710)
- Fix WS server expose issue. [#5728](https://github.com/paritytech/parity/pull/5728)
- Fix local transactions without condition. [#5716](https://github.com/paritytech/parity/pull/5716)
- Bump parity-wordlist. [#5748](https://github.com/paritytech/parity/pull/5748)
- two small changes in evm [#5700](https://github.com/paritytech/parity/pull/5700)
- Evmbin: JSON format printing pre-state. [#5712](https://github.com/paritytech/parity/pull/5712)
- Recover from empty phrase in dev mode [#5698](https://github.com/paritytech/parity/pull/5698)
- EIP-210 BLOCKHASH changes [#5505](https://github.com/paritytech/parity/pull/5505)
- fixes typo [#5708](https://github.com/paritytech/parity/pull/5708)
- Bump rocksdb [#5707](https://github.com/paritytech/parity/pull/5707)
- Fixed --datadir option [#5697](https://github.com/paritytech/parity/pull/5697)
- rpc -> weak to arc [#5688](https://github.com/paritytech/parity/pull/5688)
- typo fix [#5699](https://github.com/paritytech/parity/pull/5699)
- Revamping parity-evmbin [#5696](https://github.com/paritytech/parity/pull/5696)
- Update dependencies and bigint api [#5685](https://github.com/paritytech/parity/pull/5685)
- UI server refactoring [#5580](https://github.com/paritytech/parity/pull/5580)
- Fix from/into electrum in ethkey [#5686](https://github.com/paritytech/parity/pull/5686)
- Add unit tests [#5668](https://github.com/paritytech/parity/pull/5668)
- Guanqun add unit tests [#5671](https://github.com/paritytech/parity/pull/5671)
- Parity-PubSub as a separate API. [#5676](https://github.com/paritytech/parity/pull/5676)
- EIP-140 REVERT opcode [#5477](https://github.com/paritytech/parity/pull/5477)
- Update CHANGELOG for 1.6.7 [#5683](https://github.com/paritytech/parity/pull/5683)
- Updated docs slightly. [#5674](https://github.com/paritytech/parity/pull/5674)
- Fix build [#5684](https://github.com/paritytech/parity/pull/5684)
- Back-references for the on-demand service [#5573](https://github.com/paritytech/parity/pull/5573)
- Dynamically adjust PIP request costs based on gathered data [#5603](https://github.com/paritytech/parity/pull/5603)
- use cargo workspace [#5601](https://github.com/paritytech/parity/pull/5601)
- Latest headers Pub-Sub [#5655](https://github.com/paritytech/parity/pull/5655)
- improved dockerfile builds [#5659](https://github.com/paritytech/parity/pull/5659)
- Adding CLI options: port shift and unsafe expose. [#5677](https://github.com/paritytech/parity/pull/5677)
- Report missing author in Aura [#5583](https://github.com/paritytech/parity/pull/5583)
- typo fix [#5669](https://github.com/paritytech/parity/pull/5669)
- Remove public middleware (temporary) [#5665](https://github.com/paritytech/parity/pull/5665)
- Remove additional polyfill [#5663](https://github.com/paritytech/parity/pull/5663)
- Importing accounts from files. [#5644](https://github.com/paritytech/parity/pull/5644)
- remove the deprecated options in rustfmt.toml [#5616](https://github.com/paritytech/parity/pull/5616)
- Update the Console dapp [#5602](https://github.com/paritytech/parity/pull/5602)
- Create an account for chain=dev [#5612](https://github.com/paritytech/parity/pull/5612)
- Use babel-runtime as opposed to babel-polyfill [#5662](https://github.com/paritytech/parity/pull/5662)
- Connection dialog timestamp info [#5554](https://github.com/paritytech/parity/pull/5554)
- use copy_from_slice instead of for loop [#5647](https://github.com/paritytech/parity/pull/5647)
- Light friendly dapps [#5634](https://github.com/paritytech/parity/pull/5634)
- Add Recover button to Accounts and warnings [#5645](https://github.com/paritytech/parity/pull/5645)
- Update eth_sign docs. [#5631](https://github.com/paritytech/parity/pull/5631)
- Proper signer Pub-Sub for pending requests. [#5594](https://github.com/paritytech/parity/pull/5594)
- Bump bigint to 1.0.5 [#5641](https://github.com/paritytech/parity/pull/5641)
- PoA warp implementation [#5488](https://github.com/paritytech/parity/pull/5488)
- Improve on-demand dispatch and add support for batch requests [#5419](https://github.com/paritytech/parity/pull/5419)
- Use default account for sending transactions [#5588](https://github.com/paritytech/parity/pull/5588)
- Add peer management to the Status tab [#5566](https://github.com/paritytech/parity/pull/5566)
- Add monotonic step transition [#5587](https://github.com/paritytech/parity/pull/5587)
- Decrypting for external accounts. [#5581](https://github.com/paritytech/parity/pull/5581)
- only enable warp sync when engine supports it [#5595](https://github.com/paritytech/parity/pull/5595)
- fix the doc of installing rust [#5586](https://github.com/paritytech/parity/pull/5586)
- Small fixes [#5584](https://github.com/paritytech/parity/pull/5584)
- SecretStore: remove session on master node [#5545](https://github.com/paritytech/parity/pull/5545)
- run-clean [#5607](https://github.com/paritytech/parity/pull/5607)
- relicense RLP to MIT/Apache2 [#5591](https://github.com/paritytech/parity/pull/5591)
- Fix eth_sign signature encoding. [#5597](https://github.com/paritytech/parity/pull/5597)
- Check pending request on Node local transactions [#5564](https://github.com/paritytech/parity/pull/5564)
- Add tooltips on ActionBar [#5562](https://github.com/paritytech/parity/pull/5562)
- Can't deploy without compiling Contract [#5593](https://github.com/paritytech/parity/pull/5593)
- Add a warning when node is syncing [#5565](https://github.com/paritytech/parity/pull/5565)
- Update registry middleware [#5585](https://github.com/paritytech/parity/pull/5585)
- Set block condition to BigNumber in MethodDecoding [#5592](https://github.com/paritytech/parity/pull/5592)
- Load the sources immediately in Contract Dev [#5575](https://github.com/paritytech/parity/pull/5575)
- Remove formal verification messages in Dev Contract [#5574](https://github.com/paritytech/parity/pull/5574)
- Fix event params decoding when no names for parameters [#5567](https://github.com/paritytech/parity/pull/5567)
- Do not convert to Dates twice [#5563](https://github.com/paritytech/parity/pull/5563)
- Fix Multisig wallet settings [#5560](https://github.com/paritytech/parity/pull/5560)
- Typo [#5547](https://github.com/paritytech/parity/pull/5547)
- Generic PubSub implementation [#5456](https://github.com/paritytech/parity/pull/5456)
- Fix CI paths. [#5570](https://github.com/paritytech/parity/pull/5570)
- reorg into blocks before minimum history [#5558](https://github.com/paritytech/parity/pull/5558)
- EIP-86 update [#5506](https://github.com/paritytech/parity/pull/5506)
- Secretstore RPCs + integration [#5439](https://github.com/paritytech/parity/pull/5439)
- Fixes Parity Bar position [#5557](https://github.com/paritytech/parity/pull/5557)
- Fixes invalid log in BadgeReg events [#5556](https://github.com/paritytech/parity/pull/5556)
- Fix issues in Contract Development view [#5555](https://github.com/paritytech/parity/pull/5555)
- Added missing methods [#5542](https://github.com/paritytech/parity/pull/5542)
- option to disable persistent txqueue [#5544](https://github.com/paritytech/parity/pull/5544)
- Bump jsonrpc [#5552](https://github.com/paritytech/parity/pull/5552)
- Retrieve block headers only for header-only info [#5480](https://github.com/paritytech/parity/pull/5480)
- add snap to CI [#5519](https://github.com/paritytech/parity/pull/5519)
- Pass additional data when reporting [#5527](https://github.com/paritytech/parity/pull/5527)
- Calculate post-constructors state root in spec at load time [#5523](https://github.com/paritytech/parity/pull/5523)
- Fix utf8 decoding [#5533](https://github.com/paritytech/parity/pull/5533)
- Add CHANGELOG.md [#5513](https://github.com/paritytech/parity/pull/5513)
- Change all occurrences of ethcore.io into parity.io [#5528](https://github.com/paritytech/parity/pull/5528)
- Memory usage optimization [#5526](https://github.com/paritytech/parity/pull/5526)
- Compose transaction RPC. [#5524](https://github.com/paritytech/parity/pull/5524)
- Support external eth_sign  [#5481](https://github.com/paritytech/parity/pull/5481)
- Treat block numbers as strings, not BigNums. [#5449](https://github.com/paritytech/parity/pull/5449)
- npm cleanups [#5512](https://github.com/paritytech/parity/pull/5512)
- Export acc js [#4973](https://github.com/paritytech/parity/pull/4973)
- YARN [#5395](https://github.com/paritytech/parity/pull/5395)
- Fix linting issues [#5511](https://github.com/paritytech/parity/pull/5511)
- Chinese Translation [#5460](https://github.com/paritytech/parity/pull/5460)
- Fixing secretstore TODOs - part 2 [#5416](https://github.com/paritytech/parity/pull/5416)
- fix json format of state snapshot [#5504](https://github.com/paritytech/parity/pull/5504)
- Bump jsonrpc version [#5489](https://github.com/paritytech/parity/pull/5489)
- Groundwork for generalized warp sync [#5454](https://github.com/paritytech/parity/pull/5454)
- Add the packaging metadata to build the parity snap [#5496](https://github.com/paritytech/parity/pull/5496)
- Cancel tx JS [#4958](https://github.com/paritytech/parity/pull/4958)
- EIP-212 (bn128 curve pairing) [#5307](https://github.com/paritytech/parity/pull/5307)
- fix panickers in tree-route [#5479](https://github.com/paritytech/parity/pull/5479)
- Update links to etherscan.io [#5455](https://github.com/paritytech/parity/pull/5455)
- Refresh UI on nodeKind changes, e.g. personal -> public [#5312](https://github.com/paritytech/parity/pull/5312)
- Correct contract address for EIP-86 [#5473](https://github.com/paritytech/parity/pull/5473)
- Force two decimals for USD conversion rate [#5471](https://github.com/paritytech/parity/pull/5471)
- Refactoring of Tokens & Balances [#5372](https://github.com/paritytech/parity/pull/5372)
- Background-repeat round [#5475](https://github.com/paritytech/parity/pull/5475)
- nl i18n updated [#5461](https://github.com/paritytech/parity/pull/5461)
- Show ETH value (even 0) if ETH transfer in transaction list [#5406](https://github.com/paritytech/parity/pull/5406)
- Store the pending requests per network version [#5405](https://github.com/paritytech/parity/pull/5405)
- Use in-memory database for tests [#5451](https://github.com/paritytech/parity/pull/5451)
- WebSockets RPC server [#5425](https://github.com/paritytech/parity/pull/5425)
- Added missing docs [#5452](https://github.com/paritytech/parity/pull/5452)
- Tests and tweaks for public node middleware [#5417](https://github.com/paritytech/parity/pull/5417)
- Fix removal of hash-mismatched files. [#5440](https://github.com/paritytech/parity/pull/5440)
- parity_getBlockHeaderByNumber and LightFetch utility [#5383](https://github.com/paritytech/parity/pull/5383)
- New state tests [#5418](https://github.com/paritytech/parity/pull/5418)
- Fix buffer length for QR code gen. [#5447](https://github.com/paritytech/parity/pull/5447)
- Add raw hash signing [#5423](https://github.com/paritytech/parity/pull/5423)
- Filters and block RPCs for the light client [#5320](https://github.com/paritytech/parity/pull/5320)
- Work around mismatch for QR checksum [#5374](https://github.com/paritytech/parity/pull/5374)
- easy to use conversion from and to string for ethstore::Crypto [#5437](https://github.com/paritytech/parity/pull/5437)
- Tendermint fixes [#5415](https://github.com/paritytech/parity/pull/5415)
- Adrianbrink lightclientcache branch. [#5428](https://github.com/paritytech/parity/pull/5428)
- Add caching to HeaderChain struct [#5403](https://github.com/paritytech/parity/pull/5403)
- Add decryption to the UI (in the Signer) [#5422](https://github.com/paritytech/parity/pull/5422)
- Add CIDv0 RPC [#5414](https://github.com/paritytech/parity/pull/5414)
- Updating documentation for RPCs [#5392](https://github.com/paritytech/parity/pull/5392)
- Fixing secretstore TODOs - part 1 [#5386](https://github.com/paritytech/parity/pull/5386)
- Fixing disappearing content. [#5399](https://github.com/paritytech/parity/pull/5399)
- Snapshot chunks packed by size [#5318](https://github.com/paritytech/parity/pull/5318)
- APIs wildcards and simple arithmetic. [#5402](https://github.com/paritytech/parity/pull/5402)
- Fixing compilation without dapps. [#5410](https://github.com/paritytech/parity/pull/5410)
- Don't use port 8080 anymore [#5397](https://github.com/paritytech/parity/pull/5397)
- Quick'n'dirty CLI for the light client [#5002](https://github.com/paritytech/parity/pull/5002)
- set gas limit before proving transactions [#5401](https://github.com/paritytech/parity/pull/5401)
- Public node: perf and fixes [#5390](https://github.com/paritytech/parity/pull/5390)
- Straight download path in the readme [#5393](https://github.com/paritytech/parity/pull/5393)
- On-chain ACL checker for secretstore [#5015](https://github.com/paritytech/parity/pull/5015)
- Allow empty-encoded values from QR encoding [#5385](https://github.com/paritytech/parity/pull/5385)
- Update npm build for new inclusions [#5381](https://github.com/paritytech/parity/pull/5381)
- Fix for Ubuntu Dockerfile [#5356](https://github.com/paritytech/parity/pull/5356)
- Secretstore over network [#4974](https://github.com/paritytech/parity/pull/4974)
- Dapps and RPC server merge [#5365](https://github.com/paritytech/parity/pull/5365)
- trigger js build release [#5379](https://github.com/paritytech/parity/pull/5379)
- Update expanse json with fork at block 600000 [#5351](https://github.com/paritytech/parity/pull/5351)
- Futures-based native wrappers for contract ABIs [#5341](https://github.com/paritytech/parity/pull/5341)
- Kovan warp sync fixed [#5337](https://github.com/paritytech/parity/pull/5337)
- Aura eip155 validation transition [#5362](https://github.com/paritytech/parity/pull/5362)
- Shared wordlist for brain wallets [#5331](https://github.com/paritytech/parity/pull/5331)
- Allow signing via Qr [#4881](https://github.com/paritytech/parity/pull/4881)
- Allow entry of url or hash for DappReg meta [#5360](https://github.com/paritytech/parity/pull/5360)
- Adjust tx overlay colours [#5353](https://github.com/paritytech/parity/pull/5353)
- Add ability to disallow API subscriptions [#5366](https://github.com/paritytech/parity/pull/5366)
- EIP-213 (bn128 curve operations) [#4999](https://github.com/paritytech/parity/pull/4999)
- Fix analize output file name [#5357](https://github.com/paritytech/parity/pull/5357)
- Add default eip155 validation [#5346](https://github.com/paritytech/parity/pull/5346)
- Add new seed nodes for Classic chain [#5345](https://github.com/paritytech/parity/pull/5345)
- Shared wordlist for frontend [#5336](https://github.com/paritytech/parity/pull/5336)
- fix rpc tests [#5338](https://github.com/paritytech/parity/pull/5338)
- Public node with accounts and signing in Frontend [#5304](https://github.com/paritytech/parity/pull/5304)
- Rename Status/Status -> Status/NodeStatus [#5332](https://github.com/paritytech/parity/pull/5332)
- Updating paths to repos. [#5330](https://github.com/paritytech/parity/pull/5330)
- Separate status for canceled local transactions. [#5319](https://github.com/paritytech/parity/pull/5319)
- Cleanup the Status View [#5317](https://github.com/paritytech/parity/pull/5317)
- Update UI minimised requests [#5324](https://github.com/paritytech/parity/pull/5324)
- Order signer transactions FIFO [#5321](https://github.com/paritytech/parity/pull/5321)
- updating dependencies [#5028](https://github.com/paritytech/parity/pull/5028)
- Minimise transactions progress [#4942](https://github.com/paritytech/parity/pull/4942)
- Fix eth_sign showing as wallet account [#5309](https://github.com/paritytech/parity/pull/5309)
- Ropsten revival [#5302](https://github.com/paritytech/parity/pull/5302)
- Strict validation transitions [#4988](https://github.com/paritytech/parity/pull/4988)
- Fix default list sorting [#5303](https://github.com/paritytech/parity/pull/5303)
- Use unique owners for multisig wallets [#5298](https://github.com/paritytech/parity/pull/5298)
- Copy all existing i18n strings into zh (as-is translation aid) [#5305](https://github.com/paritytech/parity/pull/5305)
- Fix booleans in Typedinput [#5295](https://github.com/paritytech/parity/pull/5295)
- node kind RPC [#5025](https://github.com/paritytech/parity/pull/5025)
- Fix the use of MobX in playground [#5294](https://github.com/paritytech/parity/pull/5294)
- Fine grained snapshot chunking [#5019](https://github.com/paritytech/parity/pull/5019)
- Add lint:i18n to find missing & extra keys [#5290](https://github.com/paritytech/parity/pull/5290)
- Scaffolding for zh translations, including first-round by @btceth [#5289](https://github.com/paritytech/parity/pull/5289)
- JS package bumps [#5287](https://github.com/paritytech/parity/pull/5287)
- Auto-extract new i18n strings (update) [#5288](https://github.com/paritytech/parity/pull/5288)
- eip100b [#5027](https://github.com/paritytech/parity/pull/5027)
- Set earliest era in snapshot restoration [#5021](https://github.com/paritytech/parity/pull/5021)
- Avoid clogging up tmp when updater dir has bad permissions. [#5024](https://github.com/paritytech/parity/pull/5024)
- Resilient warp sync [#5018](https://github.com/paritytech/parity/pull/5018)
- Create webpack analysis files (size) [#5009](https://github.com/paritytech/parity/pull/5009)
- Dispatch an open event on drag of Parity Bar [#4987](https://github.com/paritytech/parity/pull/4987)
- Various installer and tray apps fixes [#4970](https://github.com/paritytech/parity/pull/4970)
- Export account RPC [#4967](https://github.com/paritytech/parity/pull/4967)
- Switching ValidatorSet [#4961](https://github.com/paritytech/parity/pull/4961)
- Implement PIP messages, request builder, and handlers [#4945](https://github.com/paritytech/parity/pull/4945)
- auto lint [#5003](https://github.com/paritytech/parity/pull/5003)
- Fix FireFox overflows [#5000](https://github.com/paritytech/parity/pull/5000)
- Show busy indicator, focus first field in password change [#4997](https://github.com/paritytech/parity/pull/4997)
- Consistent store naming in the Signer components [#4996](https://github.com/paritytech/parity/pull/4996)
- second (and last) part of rlp refactor [#4901](https://github.com/paritytech/parity/pull/4901)
- Double click to select account creation type [#4986](https://github.com/paritytech/parity/pull/4986)
- Fixes to the Registry dapp [#4984](https://github.com/paritytech/parity/pull/4984)
- Extend api.util [#4979](https://github.com/paritytech/parity/pull/4979)
- Updating JSON-RPC crates [#4934](https://github.com/paritytech/parity/pull/4934)
- splitting part of util into smaller crates [#4956](https://github.com/paritytech/parity/pull/4956)
- Updating syntex et al [#4983](https://github.com/paritytech/parity/pull/4983)
- EIP198 and built-in activation [#4926](https://github.com/paritytech/parity/pull/4926)
- Fix MethodDecoding for Arrays [#4977](https://github.com/paritytech/parity/pull/4977)
- Try to fix WS race condition connection [#4976](https://github.com/paritytech/parity/pull/4976)
- eth_sign where account === undefined [#4964](https://github.com/paritytech/parity/pull/4964)
- Fix references to api outside of `parity.js` [#4981](https://github.com/paritytech/parity/pull/4981)
- Fix Password Dialog form overflow [#4968](https://github.com/paritytech/parity/pull/4968)
- Changing Mutex into RwLock for transaction queue [#4951](https://github.com/paritytech/parity/pull/4951)
- Disable max seal period for external sealing [#4927](https://github.com/paritytech/parity/pull/4927)
- Attach hardware wallets already in addressbook [#4912](https://github.com/paritytech/parity/pull/4912)
- rlp serialization refactor [#4873](https://github.com/paritytech/parity/pull/4873)
- Bump nanomsg [#4965](https://github.com/paritytech/parity/pull/4965)
- Fixed multi-chunk ledger transactions on windows [#4960](https://github.com/paritytech/parity/pull/4960)
- Fix outputs in Contract Constant Queries [#4953](https://github.com/paritytech/parity/pull/4953)
- systemd: Start parity after network.target [#4952](https://github.com/paritytech/parity/pull/4952)
- Remove transaction RPC [#4949](https://github.com/paritytech/parity/pull/4949)
- Swap out ethcore.io url for parity.io [#4947](https://github.com/paritytech/parity/pull/4947)
- Don't remove confirmed requests to early. [#4933](https://github.com/paritytech/parity/pull/4933)
- Ensure sealing work enabled in miner once subscribers added [#4930](https://github.com/paritytech/parity/pull/4930)
- Add z-index to small modals as well [#4923](https://github.com/paritytech/parity/pull/4923)
- Bump nanomsg [#4946](https://github.com/paritytech/parity/pull/4946)
- Bumping multihash and libc [#4943](https://github.com/paritytech/parity/pull/4943)
- Edit ETH value, gas and gas price in Contract Deployment [#4919](https://github.com/paritytech/parity/pull/4919)
- Add ability to configure Secure API [#4922](https://github.com/paritytech/parity/pull/4922)
- Add Token image from URL [#4916](https://github.com/paritytech/parity/pull/4916)
- Use the registry fee in Token Deployment dapp [#4915](https://github.com/paritytech/parity/pull/4915)
- Add reseal max period [#4903](https://github.com/paritytech/parity/pull/4903)
- Detect rust compiler version in Parity build script, closes 4742 [#4907](https://github.com/paritytech/parity/pull/4907)
- Add Vaults logic to First Run [#4914](https://github.com/paritytech/parity/pull/4914)
- Updated gcc and rayon crates to remove outdated num_cpus dependency [#4909](https://github.com/paritytech/parity/pull/4909)
- Renaming evm binary to avoid conflicts. [#4899](https://github.com/paritytech/parity/pull/4899)
- Better error handling for traces RPC [#4849](https://github.com/paritytech/parity/pull/4849)
- Safari SectionList fix [#4895](https://github.com/paritytech/parity/pull/4895)
- Safari Dialog scrolling fix [#4893](https://github.com/paritytech/parity/pull/4893)
- Spelling :) [#4900](https://github.com/paritytech/parity/pull/4900)
- Additional kovan params [#4892](https://github.com/paritytech/parity/pull/4892)
- trigger js-precompiled build [#4898](https://github.com/paritytech/parity/pull/4898)
- Recalculate receipt roots in close_and_lock [#4884](https://github.com/paritytech/parity/pull/4884)
- Reload UI on network switch [#4864](https://github.com/paritytech/parity/pull/4864)
- Update parity-ui-precompiled with branch [#4850](https://github.com/paritytech/parity/pull/4850)
- OSX Installer is no longer experimental [#4882](https://github.com/paritytech/parity/pull/4882)
- Chain-selection from UI [#4859](https://github.com/paritytech/parity/pull/4859)
- removed redundant (and unused) FromJson trait [#4871](https://github.com/paritytech/parity/pull/4871)
- fix typos and grammar [#4880](https://github.com/paritytech/parity/pull/4880)
- Remove old experimental remote-db code [#4872](https://github.com/paritytech/parity/pull/4872)
- removed redundant FixedHash trait, fixes [#4029](https://github.com/paritytech/parity/issues/4029) [#4866](https://github.com/paritytech/parity/pull/4866)
- Reference JSON-RPC more changes-friendly [#4870](https://github.com/paritytech/parity/pull/4870)
- Better handling of Solidity compliation [#4860](https://github.com/paritytech/parity/pull/4860)
- Go through contract links in Transaction List display [#4863](https://github.com/paritytech/parity/pull/4863)
- Fix Gas Price Selector Tooltips [#4865](https://github.com/paritytech/parity/pull/4865)
- Fix auto-updater [#4867](https://github.com/paritytech/parity/pull/4867)
- Make the UI work offline [#4861](https://github.com/paritytech/parity/pull/4861)
- Subscribe to accounts info in Signer / ParityBar [#4856](https://github.com/paritytech/parity/pull/4856)
- Don't link libsnappy explicitly [#4841](https://github.com/paritytech/parity/pull/4841)
- Fix paste in Inputs [#4854](https://github.com/paritytech/parity/pull/4854)
- Extract i18n from shared UI components [#4834](https://github.com/paritytech/parity/pull/4834)
- Fix paste in Inputs [#4844](https://github.com/paritytech/parity/pull/4844)
- Pull contract deployment title from available steps [#4848](https://github.com/paritytech/parity/pull/4848)
- Supress USB error message [#4839](https://github.com/paritytech/parity/pull/4839)
- Fix getTransactionCount in --geth mode [#4837](https://github.com/paritytech/parity/pull/4837)
- CI: test coverage (for core and js) [#4832](https://github.com/paritytech/parity/pull/4832)
- Lowering threshold for transactions above gas limit [#4831](https://github.com/paritytech/parity/pull/4831)
- Fix TxViewer when no `to` (contract deployment) [#4847](https://github.com/paritytech/parity/pull/4847)
- Fix method decoding [#4845](https://github.com/paritytech/parity/pull/4845)
- Add React Hot Reload to dapps + TokenDeploy fix [#4846](https://github.com/paritytech/parity/pull/4846)
- Dapps show multiple times in some cases [#4843](https://github.com/paritytech/parity/pull/4843)
- Fixes to the Registry dapp [#4838](https://github.com/paritytech/parity/pull/4838)
- Show token icons on list summary pages [#4826](https://github.com/paritytech/parity/pull/4826)
- Calibrate step before rejection [#4800](https://github.com/paritytech/parity/pull/4800)
- Add replay protection [#4808](https://github.com/paritytech/parity/pull/4808)
- Better icon on windows [#4804](https://github.com/paritytech/parity/pull/4804)
- Better logic for contract deployments detection [#4821](https://github.com/paritytech/parity/pull/4821)
- Fix wrong default values for contract queries inputs [#4819](https://github.com/paritytech/parity/pull/4819)
- Adjust selection colours/display [#4811](https://github.com/paritytech/parity/pull/4811)
- Update the Wallet Library Registry key [#4817](https://github.com/paritytech/parity/pull/4817)
- Update Wallet to new Wallet Code [#4805](https://github.com/paritytech/parity/pull/4805)
