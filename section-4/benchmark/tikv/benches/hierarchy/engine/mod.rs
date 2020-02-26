// Copyright 2018 PingCAP, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// See the License for the specific language governing permissions and
// limitations under the License.

use criterion::{black_box, Bencher, Criterion};
use kvproto::kvrpcpb::Context;
use test_util::KvGenerator;
use tikv::storage::engine::{Engine, Snapshot};
use tikv::storage::{Key, Value};

use super::{BenchConfig, EngineFactory, DEFAULT_ITERATIONS, DEFAULT_KV_GENERATOR_SEED};

fn bench_engine_put<E: Engine, F: EngineFactory<E>>(
    bencher: &mut Bencher,
    config: &BenchConfig<F>,
) {
    let engine = config.engine_factory.build();
    let ctx = Context::new();
    bencher.iter_with_setup(
        || {
            let test_kvs: Vec<(Key, Value)> = KvGenerator::with_seed(
                config.key_length,
                config.value_length,
                DEFAULT_KV_GENERATOR_SEED,
            )
            .generate(DEFAULT_ITERATIONS)
            .iter()
            .map(|(key, value)| (Key::from_raw(&key), value.clone()))
            .collect();
            (test_kvs, &ctx)
        },
        |(test_kvs, ctx)| {
            for (key, value) in test_kvs {
                black_box(engine.put(ctx, key, value)).unwrap();
            }
        },
    );
}

fn bench_engine_snapshot<E: Engine, F: EngineFactory<E>>(
    bencher: &mut Bencher,
    config: &BenchConfig<F>,
) {
    let engine = config.engine_factory.build();
    let ctx = Context::new();
    bencher.iter(|| black_box(&engine).snapshot(black_box(&ctx)).unwrap());
}

//exclude snapshot
fn bench_engine_get<E: Engine, F: EngineFactory<E>>(
    bencher: &mut Bencher,
    config: &BenchConfig<F>,
) {
    let engine = config.engine_factory.build();
    let ctx = Context::new();
    let test_kvs: Vec<Key> = KvGenerator::with_seed(
        config.key_length,
        config.value_length,
        DEFAULT_KV_GENERATOR_SEED,
    )
    .generate(DEFAULT_ITERATIONS)
    .iter()
    .map(|(key, _)| Key::from_raw(&key))
    .collect();

    bencher.iter_with_setup(
        || {
            let snap = engine.snapshot(&ctx).unwrap();
            (snap, &test_kvs)
        },
        |(snap, test_kvs)| {
            for key in test_kvs {
                black_box(snap.get(key).unwrap());
            }
        },
    );
}

pub fn bench_engine<E: Engine, F: EngineFactory<E>>(c: &mut Criterion, configs: &[BenchConfig<F>]) {
    c.bench_function_over_inputs(
        "engine_get(exclude snapshot)",
        bench_engine_get,
        configs.to_vec(),
    );
    c.bench_function_over_inputs("engine_put", bench_engine_put, configs.to_owned());
    c.bench_function_over_inputs("engine_snapshot", bench_engine_snapshot, configs.to_owned());
}
