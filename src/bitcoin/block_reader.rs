use std::borrow::BorrowMut;
use std::io;

use bitcoin::hashes::{sha256d, Hash};
use bitcoin::Witness;
use byteorder::{LittleEndian, ReadBytesExt};

use crate::bitcoin::proto::block::{AuxPowExtension, Block};
use crate::bitcoin::proto::header::BlockHeader;
use crate::bitcoin::proto::tx::{RawTx, TxInput, TxOutpoint, TxOutput};
use crate::bitcoin::proto::varuint::VarUint;
use crate::bitcoin::proto::MerkleBranch;
use crate::bitcoin::CoinType;

pub trait BlockchainRead: io::Read {
    fn read_256hash(&mut self) -> anyhow::Result<[u8; 32]> {
        let mut arr = [0u8; 32];
        self.read_exact(arr.borrow_mut())?;
        Ok(arr)
    }

    fn read_u8_vec(&mut self, count: u32) -> anyhow::Result<Vec<u8>> {
        let mut arr = vec![0u8; count as usize];
        self.read_exact(arr.borrow_mut())?;
        Ok(arr)
    }

    /// Reads a block as specified here: https://en.bitcoin.it/wiki/Protocol_specification#block
    fn read_block(&mut self, size: u32, coin: &CoinType) -> anyhow::Result<Block> {
        let header = self.read_block_header()?;
        // Parse AuxPow data if present
        let aux_pow_extension = match coin.aux_pow_activation_version {
            Some(version) if header.version >= version => {
                Some(self.read_aux_pow_extension(coin.version_id)?)
            }
            _ => None,
        };
        let tx_count = VarUint::read_from(self)?;
        let txs = self.read_txs(tx_count.value, coin.version_id)?;
        Ok(Block::new(size, header, aux_pow_extension, tx_count, txs))
    }

    fn read_block_header(&mut self) -> anyhow::Result<BlockHeader> {
        let version = self.read_u32::<LittleEndian>()?;
        let prev_hash = sha256d::Hash::from_byte_array(self.read_256hash()?);
        let merkle_root = sha256d::Hash::from_byte_array(self.read_256hash()?);
        let timestamp = self.read_u32::<LittleEndian>()?;
        let bits = self.read_u32::<LittleEndian>()?;
        let nonce = self.read_u32::<LittleEndian>()?;

        Ok(BlockHeader {
            version,
            prev_hash,
            merkle_root,
            timestamp,
            bits,
            nonce,
        })
    }

    fn read_txs(&mut self, tx_count: u64, version_id: u8) -> anyhow::Result<Vec<RawTx>> {
        (0..tx_count).map(|_| self.read_tx(version_id)).collect()
    }

    /// Reads a transaction as specified here: https://en.bitcoin.it/wiki/Protocol_specification#tx
    fn read_tx(&mut self, version_id: u8) -> anyhow::Result<RawTx> {
        let mut flags = 0u8;
        let version = self.read_u32::<LittleEndian>()?;

        // Parse transaction inputs and check if this transaction contains segwit data
        let mut in_count = VarUint::read_from(self)?;
        if in_count.value == 0 {
            flags = self.read_u8()?;
            // TODO: handle segwit data
            in_count = VarUint::read_from(self)?
        }
        let mut inputs = self.read_tx_inputs(in_count.value)?;

        // Parse transaction outputs
        let out_count = VarUint::read_from(self)?;
        let outputs = self.read_tx_outputs(out_count.value)?;

        // Check if the witness flag is present
        if flags & 1 > 0 {
            for witness_index in 0..in_count.value {
                let item_count = VarUint::read_from(self)?;
                let mut witnesses = vec![];
                for _ in 0..item_count.value {
                    let witness_len = VarUint::read_from(self)?;
                    let witness = self.read_u8_vec(witness_len.value as u32)?;
                    witnesses.push(witness);
                }
                inputs[witness_index as usize].witness = Some(Witness::from_slice(&witnesses));
            }
        }
        let locktime = self.read_u32::<LittleEndian>()?;
        let tx = RawTx {
            version,
            in_count,
            inputs,
            out_count,
            outputs,
            locktime,
            version_id,
        };
        Ok(tx)
    }

    fn read_tx_outpoint(&mut self) -> anyhow::Result<TxOutpoint> {
        let txid = sha256d::Hash::from_byte_array(self.read_256hash()?);
        let index = self.read_u32::<LittleEndian>()?;

        Ok(TxOutpoint { txid, index })
    }

    fn read_tx_inputs(&mut self, input_count: u64) -> anyhow::Result<Vec<TxInput>> {
        let mut inputs = Vec::with_capacity(input_count as usize);
        for _ in 0..input_count {
            let outpoint = self.read_tx_outpoint()?;
            let script_len = VarUint::read_from(self)?;
            let script_sig = self.read_u8_vec(script_len.value as u32)?;
            let seq_no = self.read_u32::<LittleEndian>()?;
            inputs.push(TxInput {
                outpoint,
                script_len,
                script_sig,
                seq_no,
                witness: None,
            });
        }
        Ok(inputs)
    }

    fn read_tx_outputs(&mut self, output_count: u64) -> anyhow::Result<Vec<TxOutput>> {
        let mut outputs = Vec::with_capacity(output_count as usize);
        for _ in 0..output_count {
            let value = self.read_u64::<LittleEndian>()?;
            let script_len = VarUint::read_from(self)?;
            let script_pubkey = self.read_u8_vec(script_len.value as u32)?;
            outputs.push(TxOutput {
                value,
                script_len,
                script_pubkey,
            });
        }
        Ok(outputs)
    }

    /// Reads a merkle branch as specified here https://en.bitcoin.it/wiki/Merged_mining_specification#Merkle_Branch
    /// This is mainly used for merged mining (AuxPoW).
    fn read_merkle_branch(&mut self) -> anyhow::Result<MerkleBranch> {
        let branch_length = VarUint::read_from(self)?;
        let hashes = (0..branch_length.value)
            .map(|_| self.read_256hash())
            .collect::<anyhow::Result<Vec<[u8; 32]>>>()?;
        let side_mask = self.read_u32::<LittleEndian>()?;
        Ok(MerkleBranch::new(hashes, side_mask))
    }

    /// Reads the additional AuxPow fields as specified here https://en.bitcoin.it/wiki/Merged_mining_specification#Aux_proof-of-work_block
    fn read_aux_pow_extension(&mut self, version_id: u8) -> anyhow::Result<AuxPowExtension> {
        let coinbase_tx = self.read_tx(version_id)?;
        let block_hash = sha256d::Hash::from_byte_array(self.read_256hash()?);

        let coinbase_branch = self.read_merkle_branch()?;
        let blockchain_branch = self.read_merkle_branch()?;

        let parent_block = self.read_block_header()?;

        Ok(AuxPowExtension {
            coinbase_tx,
            block_hash,
            coinbase_branch,
            blockchain_branch,
            parent_block,
        })
    }
}

/// All types that implement `Read` get methods defined in `BlockchainRead`
/// for free.
impl<R: io::Read + ?Sized> BlockchainRead for R {}
