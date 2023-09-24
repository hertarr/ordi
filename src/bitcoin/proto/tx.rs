use std::fmt;

use bitcoin::hashes::{sha256d, Hash};
use bitcoin::{Transaction, Witness};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::bitcoin::common::utils;
use crate::bitcoin::proto::script;
use crate::bitcoin::proto::varuint::VarUint;
use crate::bitcoin::proto::ToRaw;

#[derive(Clone)]
pub struct RawTx {
    pub version: u32,
    pub in_count: VarUint,
    pub inputs: Vec<TxInput>,
    pub out_count: VarUint,
    pub outputs: Vec<TxOutput>,
    pub locktime: u32,
    pub version_id: u8,
}

/// Simple transaction struct
/// Please note: The txid is not stored here. See Hashed.
#[derive(Clone)]
pub struct EvaluatedTx {
    pub version: u32,
    pub in_count: VarUint,
    pub inputs: Vec<TxInput>,
    pub out_count: VarUint,
    pub outputs: Vec<EvaluatedTxOut>,
    pub locktime: u32,
}

impl EvaluatedTx {
    pub fn new(
        version: u32,
        in_count: VarUint,
        inputs: Vec<TxInput>,
        out_count: VarUint,
        outputs: Vec<TxOutput>,
        locktime: u32,
        version_id: u8,
    ) -> Self {
        // Evaluate and wrap all outputs to process them later
        let outputs = outputs
            .into_par_iter()
            .map(|o| EvaluatedTxOut::eval_script(o, version_id))
            .collect();
        EvaluatedTx {
            version,
            in_count,
            inputs,
            out_count,
            outputs,
            locktime,
        }
    }

    pub fn is_coinbase(&self) -> bool {
        if self.in_count.value == 1 {
            let input = self.inputs.first().unwrap();
            return input.outpoint.txid.as_ref() == [0u8; 32] && input.outpoint.index == 0xFFFFFFFF;
        }
        false
    }
}

impl fmt::Debug for EvaluatedTx {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Tx")
            .field("version", &self.version)
            .field("in_count", &self.in_count)
            .field("out_count", &self.out_count)
            .field("locktime", &self.locktime)
            .finish()
    }
}

impl From<RawTx> for EvaluatedTx {
    fn from(tx: RawTx) -> Self {
        Self::new(
            tx.version,
            tx.in_count,
            tx.inputs,
            tx.out_count,
            tx.outputs,
            tx.locktime,
            tx.version_id,
        )
    }
}

impl From<Transaction> for EvaluatedTx {
    fn from(tx: Transaction) -> EvaluatedTx {
        let inputs = tx
            .input
            .into_par_iter()
            .map(|input| TxInput {
                outpoint: TxOutpoint {
                    txid: input.previous_output.txid.to_raw_hash(),
                    index: input.previous_output.vout,
                },
                script_len: (input.script_sig.len() as u64).into(),
                script_sig: input.script_sig.into_bytes(),
                seq_no: input.sequence.to_consensus_u32(),
                witness: if input.witness.len() != 0 {
                    Some(input.witness)
                } else {
                    None
                },
            })
            .collect::<Vec<TxInput>>();

        let outputs = tx
            .output
            .into_par_iter()
            .map(|output| TxOutput {
                value: output.value,
                script_len: (output.script_pubkey.len() as u64).into(),
                script_pubkey: output.script_pubkey.into_bytes(),
            })
            .collect::<Vec<TxOutput>>();
        EvaluatedTx::new(
            tx.version as u32,
            (inputs.len() as u32).into(),
            inputs,
            (outputs.len() as u32).into(),
            outputs,
            tx.lock_time.to_consensus_u32(),
            0,
        )
    }
}

impl ToRaw for EvaluatedTx {
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes =
            Vec::with_capacity((4 + self.in_count.value + self.out_count.value + 4) as usize);

        // Serialize version
        bytes.extend_from_slice(&self.version.to_le_bytes());
        // Serialize all TxInputs
        bytes.extend_from_slice(&self.in_count.to_bytes());
        for i in &self.inputs {
            bytes.extend_from_slice(&i.to_bytes());
        }
        // Serialize all TxOutputs
        bytes.extend_from_slice(&self.out_count.to_bytes());
        for o in &self.outputs {
            bytes.extend_from_slice(&o.out.to_bytes());
        }
        // Serialize locktime
        bytes.extend_from_slice(&self.locktime.to_le_bytes());
        bytes
    }
}

/// TxOutpoint references an existing transaction output
#[derive(PartialEq, Eq, Hash, Clone)]
pub struct TxOutpoint {
    pub txid: sha256d::Hash,
    pub index: u32, // 0-based offset within tx
}

impl TxOutpoint {
    pub fn new(txid: sha256d::Hash, index: u32) -> Self {
        Self { txid, index }
    }

    pub fn is_null(&self) -> bool {
        self.txid == Hash::all_zeros() && self.index == u32::max_value()
    }
}

impl ToRaw for TxOutpoint {
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32 + 4);
        bytes.extend_from_slice(self.txid.as_byte_array());
        bytes.extend_from_slice(&self.index.to_le_bytes());
        bytes
    }
}

impl fmt::Debug for TxOutpoint {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("TxOutpoint")
            .field("txid", &self.txid)
            .field("index", &self.index)
            .finish()
    }
}

/// Holds TxInput informations
#[derive(Clone)]
pub struct TxInput {
    pub outpoint: TxOutpoint,
    pub script_len: VarUint,
    pub script_sig: Vec<u8>,
    pub seq_no: u32,
    pub witness: Option<Witness>,
}

impl TxInput {
    pub fn is_null(&self) -> bool {
        self.outpoint.is_null()
    }
}

impl ToRaw for TxInput {
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(36 + 5 + self.script_len.value as usize + 4);
        bytes.extend_from_slice(&self.outpoint.to_bytes());
        bytes.extend_from_slice(&self.script_len.to_bytes());
        bytes.extend_from_slice(&self.script_sig);
        bytes.extend_from_slice(&self.seq_no.to_le_bytes());
        bytes
    }
}

impl fmt::Debug for TxInput {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("TxInput")
            .field("outpoint", &self.outpoint)
            .field("script_len", &self.script_len)
            .field("script_sig", &self.script_sig)
            .field("seq_no", &self.seq_no)
            .finish()
    }
}

/// Evaluates script_pubkey and wraps TxOutput
#[derive(Clone)]
pub struct EvaluatedTxOut {
    pub script: script::EvaluatedScript,
    pub out: TxOutput,
}

impl EvaluatedTxOut {
    pub fn eval_script(out: TxOutput, version_id: u8) -> EvaluatedTxOut {
        EvaluatedTxOut {
            script: script::eval_from_bytes(&out.script_pubkey, version_id),
            out,
        }
    }
}

/// Holds TxOutput informations
#[derive(Clone)]
pub struct TxOutput {
    pub value: u64,
    pub script_len: VarUint,
    pub script_pubkey: Vec<u8>,
}

impl ToRaw for TxOutput {
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(8 + 5 + self.script_len.value as usize);
        bytes.extend_from_slice(&self.value.to_le_bytes());
        bytes.extend_from_slice(&self.script_len.to_bytes());
        bytes.extend_from_slice(&self.script_pubkey);
        bytes
    }
}

impl fmt::Debug for TxOutput {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("TxOutput")
            .field("value", &self.value)
            .field("script_len", &self.script_len)
            .field("script_pubkey", &utils::arr_to_hex(&self.script_pubkey))
            .finish()
    }
}
