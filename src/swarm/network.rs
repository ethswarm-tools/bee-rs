//! Block-time aware network selector. Used by stamp duration math —
//! Bee mainnet is on Ethereum (15-second blocks); Gnosis Chain is the
//! production Swarm settlement chain (~5-second blocks).
//!
//! Mirrors the bee-js `Network` enum (`mainnet` / `gnosis`).

/// Settlement chain selector for stamp duration calculations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Network {
    /// Ethereum mainnet — 15-second average block time.
    Mainnet,
    /// Gnosis Chain — 5-second average block time. Default for Swarm.
    #[default]
    Gnosis,
}

impl Network {
    /// Average block time in seconds.
    pub const fn block_time_seconds(self) -> u64 {
        match self {
            Network::Mainnet => 15,
            Network::Gnosis => 5,
        }
    }

    /// Number of blocks in `seconds` rounded up to the next block.
    pub const fn seconds_to_blocks(self, seconds: u64) -> u64 {
        let bt = self.block_time_seconds();
        seconds.div_ceil(bt)
    }

    /// Wall-clock seconds covered by `blocks` blocks.
    pub const fn blocks_to_seconds(self, blocks: u64) -> u64 {
        blocks * self.block_time_seconds()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_times_match_chain() {
        assert_eq!(Network::Mainnet.block_time_seconds(), 15);
        assert_eq!(Network::Gnosis.block_time_seconds(), 5);
    }

    #[test]
    fn seconds_to_blocks_rounds_up() {
        assert_eq!(Network::Gnosis.seconds_to_blocks(0), 0);
        assert_eq!(Network::Gnosis.seconds_to_blocks(1), 1);
        assert_eq!(Network::Gnosis.seconds_to_blocks(5), 1);
        assert_eq!(Network::Gnosis.seconds_to_blocks(6), 2);
        assert_eq!(Network::Mainnet.seconds_to_blocks(60), 4);
        assert_eq!(Network::Mainnet.seconds_to_blocks(61), 5);
    }

    #[test]
    fn round_trip_blocks_to_seconds() {
        assert_eq!(Network::Gnosis.blocks_to_seconds(12), 60);
        assert_eq!(Network::Mainnet.blocks_to_seconds(4), 60);
    }

    #[test]
    fn default_is_gnosis() {
        assert_eq!(Network::default(), Network::Gnosis);
    }
}
