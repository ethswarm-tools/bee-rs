//! Pure stamp math. Mirrors bee-go's free functions in
//! `pkg/postage/postage.go` and bee-js `stamps.ts`. No I/O.

use num_bigint::BigInt;

/// Effective-utilisation breakpoints for encrypted, medium-erasure
/// batches at each depth. Values mirror bee-js `stamps.ts` exactly.
pub const EFFECTIVE_SIZE_BREAKPOINTS: &[(i32, f64)] = &[
    (17, 0.00004089),
    (18, 0.00609),
    (19, 0.10249),
    (20, 0.62891),
    (21, 2.38),
    (22, 7.07),
    (23, 18.24),
    (24, 43.04),
    (25, 96.5),
    (26, 208.52),
    (27, 435.98),
    (28, 908.81),
    (29, 1870.0),
    (30, 3810.0),
    (31, 7730.0),
    (32, 15610.0),
    (33, 31430.0),
    (34, 63150.0),
];

/// Fractional usage `[0, 1]` of a postage batch.
pub fn get_stamp_usage(utilization: u32, depth: u8, bucket_depth: u8) -> f64 {
    let denom = 1u64 << (depth - bucket_depth);
    f64::from(utilization) / (denom as f64)
}

/// Theoretical capacity of a batch: `4096 * 2^depth` bytes.
pub fn get_stamp_theoretical_bytes(depth: i32) -> i64 {
    4096i64 * (1i64 << depth)
}

/// Cost of buying a batch: `2^depth * amount` PLUR.
pub fn get_stamp_cost(depth: i32, amount: &BigInt) -> BigInt {
    BigInt::from(2u32).pow(depth as u32) * amount
}

/// Practical capacity for the given depth using the encrypted-medium
/// erasure breakpoints (17..=34). Outside that range falls back to
/// `0.9 * theoretical`.
pub fn get_stamp_effective_bytes(depth: i32) -> i64 {
    if depth < 17 {
        return 0;
    }
    for (d, gb) in EFFECTIVE_SIZE_BREAKPOINTS {
        if *d == depth {
            return (gb * 1_000_000_000.0) as i64;
        }
    }
    let theoretical = get_stamp_theoretical_bytes(depth);
    ((theoretical as f64) * 0.9) as i64
}

/// Smallest depth whose effective capacity covers `bytes`. Falls back
/// to `35` for sizes the table doesn't cover.
pub fn get_depth_for_size(bytes: i64) -> i32 {
    for (d, gb) in EFFECTIVE_SIZE_BREAKPOINTS {
        if (bytes as f64) <= gb * 1_000_000_000.0 {
            return *d;
        }
    }
    35
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theoretical_bytes_at_depth_17() {
        // 4096 * 2^17 = 536_870_912
        assert_eq!(get_stamp_theoretical_bytes(17), 536_870_912);
    }

    #[test]
    fn cost_at_depth_zero_is_amount() {
        let amt = BigInt::from(100);
        assert_eq!(get_stamp_cost(0, &amt), amt);
    }

    #[test]
    fn cost_at_depth_three_is_eight_amount() {
        let amt = BigInt::from(100);
        assert_eq!(get_stamp_cost(3, &amt), BigInt::from(800));
    }

    #[test]
    fn effective_bytes_uses_table_at_depth_17() {
        // 0.00004089 * 1e9 = 40_890
        assert_eq!(get_stamp_effective_bytes(17), 40_890);
    }

    #[test]
    fn effective_bytes_below_17_is_zero() {
        assert_eq!(get_stamp_effective_bytes(16), 0);
    }

    #[test]
    fn depth_for_size_smallest_covering() {
        // 1 MB = 1e6 bytes — fits in depth-19 (0.10249 GB ≈ 102 MB).
        // Smaller depths: depth-18 = 6.09 MB, also fits. Should pick 18.
        // Smaller still: depth-17 = 0.04 MB, doesn't fit.
        assert_eq!(get_depth_for_size(1_000_000), 18);
    }

    #[test]
    fn usage_fraction() {
        // utilization = 8, depth = 20, bucket_depth = 16 → 8 / (1 << 4) = 0.5.
        assert!((get_stamp_usage(8, 20, 16) - 0.5).abs() < 1e-9);
    }
}
