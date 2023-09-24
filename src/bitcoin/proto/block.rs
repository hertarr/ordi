use bitcoin::hashes::sha256d;
use std::fmt;

use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::bitcoin::common::utils;
use crate::bitcoin::proto::header::BlockHeader;
use crate::bitcoin::proto::tx::{EvaluatedTx, RawTx};
use crate::bitcoin::proto::varuint::VarUint;
use crate::bitcoin::proto::{Hashed, MerkleBranch};

/// Basic block structure which holds all information
pub struct Block {
    pub size: u32,
    pub header: Hashed<BlockHeader>,
    pub aux_pow_extension: Option<AuxPowExtension>,
    pub tx_count: VarUint,
    pub txs: Vec<Hashed<EvaluatedTx>>,
}

impl From<bitcoin::Block> for Block {
    fn from(value: bitcoin::Block) -> Block {
        let tx_count = (value.txdata.len() as u32).into();
        let txs = value
            .txdata
            .into_par_iter()
            .map(|raw| Hashed::double_sha256(EvaluatedTx::from(raw)))
            .collect();

        Block {
            size: 0,
            header: Hashed::double_sha256(value.header.into()),
            aux_pow_extension: None,
            tx_count,
            txs,
        }
    }
}

impl Block {
    pub fn new(
        size: u32,
        header: BlockHeader,
        aux_pow_extension: Option<AuxPowExtension>,
        tx_count: VarUint,
        raw_txs: Vec<RawTx>,
    ) -> Block {
        let txs = raw_txs
            .into_par_iter()
            .map(|raw| Hashed::double_sha256(EvaluatedTx::from(raw)))
            .collect();
        Block {
            size,
            header: Hashed::double_sha256(header),
            aux_pow_extension,
            tx_count,
            txs,
        }
    }

    /// Computes merkle root for all containing transactions
    pub fn compute_merkle_root(&self) -> sha256d::Hash {
        let hashes = self
            .txs
            .iter()
            .map(|tx| tx.hash)
            .collect::<Vec<sha256d::Hash>>();
        utils::merkle_root(hashes)
    }

    /// Calculates merkle root and verifies it against the field in BlockHeader.
    /// panics if not valid.
    pub fn verify_merkle_root(&self) -> anyhow::Result<()> {
        let merkle_root = self.compute_merkle_root();

        if merkle_root == self.header.value.merkle_root {
            Ok(())
        } else {
            let msg = format!(
                "Invalid merkle_root!\n  -> expected: {}\n  -> got: {}\n",
                &self.header.value.merkle_root, &merkle_root
            );
            Err(anyhow::anyhow!("Validate error: {}", msg))
        }
    }
}

impl fmt::Debug for Block {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Block")
            .field("header", &self.header)
            .field("tx_count", &self.tx_count)
            .finish()
    }
}

/// This is used to prove work on the auxiliary blockchain,
/// see https://en.bitcoin.it/wiki/Merged_mining_specification
pub struct AuxPowExtension {
    pub coinbase_tx: RawTx,
    pub block_hash: sha256d::Hash,
    pub coinbase_branch: MerkleBranch,
    pub blockchain_branch: MerkleBranch,
    pub parent_block: BlockHeader,
}

/// Get block reward for given height
pub fn get_base_reward(block_height: u64) -> u64 {
    (50 * 100000000) >> (block_height / 210000)
}

#[cfg(test)]
mod tests {
    use super::get_base_reward;

    #[test]
    fn test_get_base_reward() {
        assert_eq!(get_base_reward(0), 5000000000);
        assert_eq!(get_base_reward(209999), 5000000000);
        assert_eq!(get_base_reward(210000), 2500000000);
        assert_eq!(get_base_reward(419999), 2500000000);
        assert_eq!(get_base_reward(420000), 1250000000);
        assert_eq!(get_base_reward(629999), 1250000000);
        assert_eq!(get_base_reward(630000), 0625000000);
    }
}
