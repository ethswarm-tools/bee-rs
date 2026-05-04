//! Side-by-side perf comparison: current `sha3` keccak vs `tiny-keccak`,
//! plus baselines for the BMT chunk-address path and ECDSA signing.
//!
//! Run with: `cargo bench --bench hashing`
//!
//! The point is to decide whether swapping `sha3` → `tiny-keccak` is
//! worth doing. Per-call numbers matter less than the ratio, since
//! the chunk-address path runs keccak ~129 times per 4 KiB chunk
//! (128 BMT-leaf pairs + 1 outer keccak).

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use sha3::{Digest, Keccak256};
use tiny_keccak::{Hasher, Keccak};

use bee::swarm::bmt::{calculate_chunk_address, keccak256};
use bee::swarm::keys::PrivateKey;

fn sha3_keccak(input: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(input);
    let out = h.finalize();
    let mut a = [0u8; 32];
    a.copy_from_slice(&out);
    a
}

fn tiny_keccak(input: &[u8]) -> [u8; 32] {
    let mut h = Keccak::v256();
    h.update(input);
    let mut out = [0u8; 32];
    h.finalize(&mut out);
    out
}

fn bench_keccak_small(c: &mut Criterion) {
    // 32-byte input — representative of address derivation, signing
    // prehash, BMT pair-reduce step.
    let input = [0xab_u8; 32];
    let mut g = c.benchmark_group("keccak256_32B");
    g.throughput(Throughput::Bytes(32));
    g.bench_function("sha3", |b| b.iter(|| sha3_keccak(black_box(&input))));
    g.bench_function("tiny-keccak", |b| b.iter(|| tiny_keccak(black_box(&input))));
    g.finish();
}

fn bench_keccak_chunk(c: &mut Criterion) {
    // 4 KiB input — chunk-sized payload.
    let input = vec![0xcd_u8; 4096];
    let mut g = c.benchmark_group("keccak256_4KiB");
    g.throughput(Throughput::Bytes(4096));
    g.bench_function("sha3", |b| b.iter(|| sha3_keccak(black_box(&input))));
    g.bench_function("tiny-keccak", |b| b.iter(|| tiny_keccak(black_box(&input))));
    g.finish();
}

fn bench_chunk_address(c: &mut Criterion) {
    // Full BMT chunk-address path: 8-byte span + 4 KiB payload.
    // ~129 keccak256 invocations per call (128 leaf pairs + 1 outer).
    let mut data = vec![0u8; 8 + 4096];
    data[0..8].copy_from_slice(&(4096u64).to_le_bytes());
    for (i, b) in data[8..].iter_mut().enumerate() {
        *b = (i & 0xff) as u8;
    }

    let mut g = c.benchmark_group("chunk_address");
    g.throughput(Throughput::Bytes(4096));
    g.bench_function("calculate_chunk_address_4KiB", |b| {
        b.iter(|| calculate_chunk_address(black_box(&data)).unwrap())
    });

    g.finish();

    // Sanity: keccak256 of a 64-byte pair — the inner-loop op the BMT
    // performs ~127 times per chunk address.
    let mut g = c.benchmark_group("bmt_keccak256_pair_64B");
    g.throughput(Throughput::Bytes(64));
    g.bench_function("via_keccak256_helper", |b| {
        let pair = [0xee_u8; 64];
        b.iter(|| keccak256(black_box(&pair)))
    });
    g.finish();
}

fn bench_ecdsa_sign(c: &mut Criterion) {
    // Eth-signed-message scheme: two keccak256 calls + ECDSA sign over
    // the 32-byte prehash via the active secp256k1 backend.
    let pk = PrivateKey::new(&[0x42; 32]).unwrap();

    let msg = vec![0u8; 32];
    let mut g = c.benchmark_group("ecdsa_sign_eth_msg");
    g.bench_function("sign_32B_msg", |b| {
        b.iter(|| pk.sign(black_box(&msg)).unwrap())
    });
    let big_msg = vec![0u8; 4096];
    g.bench_function("sign_4KiB_msg", |b| {
        b.iter(|| pk.sign(black_box(&big_msg)).unwrap())
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_keccak_small,
    bench_keccak_chunk,
    bench_chunk_address,
    bench_ecdsa_sign,
);
criterion_main!(benches);
