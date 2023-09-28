use std::env::VarError;
use std::{path::PathBuf, thread, time::Duration};

use bitcoincore_rpc::{Client, Error, RpcApi};
use log::info;
use rusty_leveldb::{Options, Status, WriteBatch, DB};
use thiserror::Error;

use crate::bitcoin::index::IndexError;
use crate::block::{BlockUpdaterError, InscribeUpdater, TransferUpdater, Tx};
use crate::inscription::Inscription;
use crate::{
    bitcoin::index::{Index, FIRST_INSCRIPTION_HEIGHT},
    block::BlockUpdater,
};

pub mod bitcoin;

pub mod block;
pub mod epoch;
pub mod height;
pub mod inscription;

const ORDI_STATUS: &str = "status";
const ORDI_OUTPUT_VALUE: &str = "output_value";
const ORDI_ID_TO_INSCRIPTION: &str = "id_inscription";
const ORDI_INSCRIPTION_TO_OUTPUT: &str = "inscription_output";
const ORDI_OUTPUT_TO_INSCRIPTION: &str = "output_inscription";

#[derive(Error, Debug)]
pub enum OrdiError {
    #[error("Var error: `{0}`")]
    VarError(#[from] VarError),
    #[error("Open leveldb error: `{0}`")]
    OpenLevelDBError(#[from] Status),
    #[error("Bitcoin rpc errpr: `{0}`")]
    BitcoinRpcError(#[from] Error),
    #[error("Index error: `{0}`")]
    IndexError(#[from] IndexError),
    #[error("BlockUpdater error: `{0}`")]
    BlockUpdaterError(#[from] BlockUpdaterError),
}

pub struct Ordi {
    pub btc_rpc_client: Client,
    pub status: DB,
    pub output_value: DB,
    pub id_inscription: DB,
    pub inscription_output: DB,
    pub output_inscription: DB,
    pub index: Index,
    pub inscribe_updaters: Vec<InscribeUpdater>,
    pub transfer_updaters: Vec<TransferUpdater>,
}

impl Ordi {
    pub fn new(in_memory: bool) -> Result<Ordi, OrdiError> {
        let mut options = if in_memory {
            rusty_leveldb::in_memory()
        } else {
            Options::default()
        };
        options.max_file_size = 2 << 25;

        let base_path = PathBuf::from(std::env::var("ordi_data_dir")?.as_str());
        let status = DB::open(base_path.join(ORDI_STATUS), options.clone())?;
        let output_value = DB::open(base_path.join(ORDI_OUTPUT_VALUE), options.clone())?;
        let id_inscription = DB::open(base_path.join(ORDI_ID_TO_INSCRIPTION), options.clone())?;
        let inscription_output =
            DB::open(base_path.join(ORDI_INSCRIPTION_TO_OUTPUT), options.clone())?;
        let output_inscription = DB::open(
            base_path.join(ORDI_OUTPUT_TO_INSCRIPTION),
            rusty_leveldb::in_memory(),
        )?;

        let btc_data_dir = std::env::var("btc_data_dir")?;
        let btc_rpc_client = Client::new(
            std::env::var("btc_rpc_host")?.as_str(),
            bitcoincore_rpc::Auth::UserPass(
                std::env::var("btc_rpc_user")?,
                std::env::var("btc_rpc_pass")?,
            ),
        )?;
        let index = Index::new(PathBuf::from(btc_data_dir))?;

        Ok(Ordi {
            btc_rpc_client,
            status,
            output_value,
            id_inscription,
            inscription_output,
            output_inscription,
            index,
            inscribe_updaters: vec![],
            transfer_updaters: vec![],
        })
    }

    pub fn close(&mut self) {
        self.status.close().expect("Close status db.");
        self.output_value.close().expect("Close output_value db.");
        self.id_inscription
            .close()
            .expect("Close id_inscription db.");
        self.inscription_output
            .close()
            .expect("Close inscription_output db.");
        self.output_inscription
            .close()
            .expect("Close output_inscription db.");
    }

    pub fn start(&mut self) -> Result<(), OrdiError> {
        // Catch up latest block.
        let mut next_height = self.index.max_height + 1;
        for height in FIRST_INSCRIPTION_HEIGHT..next_height {
            let block = self.index.catch_block(height)?;
            let mut block_updater = BlockUpdater::new(
                height,
                block,
                &self.btc_rpc_client,
                &mut self.status,
                &mut self.output_value,
                &mut self.id_inscription,
                &mut self.inscription_output,
                &mut self.output_inscription,
                &self.inscribe_updaters,
                &self.transfer_updaters,
            );

            block_updater.index_transactions()?;
        }

        let client = &self.btc_rpc_client;
        loop {
            match client.get_block_hash(next_height) {
                Ok(block_hash) => {
                    let mut block_updater = BlockUpdater::new(
                        next_height,
                        client.get_block(&block_hash)?.into(),
                        &self.btc_rpc_client,
                        &mut self.status,
                        &mut self.output_value,
                        &mut self.id_inscription,
                        &mut self.inscription_output,
                        &mut self.output_inscription,
                        &self.inscribe_updaters,
                        &self.transfer_updaters,
                    );

                    block_updater.index_transactions()?;
                    next_height += 1;
                }
                Err(_) => {
                    thread::sleep(Duration::from_secs(10));
                }
            };
        }
    }

    pub fn index_output_value(&mut self) -> Result<(), OrdiError> {
        for height in 0..FIRST_INSCRIPTION_HEIGHT {
            let block = self.index.catch_block(height)?;
            for (_tx_index, tx) in block.txs.iter().enumerate() {
                self.index_output_value_in_transaction(&tx)?;
            }
        }

        Ok(())
    }

    fn index_output_value_in_transaction(&mut self, tx: &Tx) -> Result<(), OrdiError> {
        let mut wb = WriteBatch::new();
        for (output_index, output) in tx.value.outputs.iter().enumerate() {
            let k = format!("{}:{}", tx.hash.to_string(), output_index);
            wb.put(k.as_bytes(), output.out.value.to_le_bytes().as_slice());
        }

        for input in tx.value.inputs.iter() {
            if input.outpoint.is_null() {
                continue;
            }

            let k = format!(
                "{}:{}",
                input.outpoint.txid.to_string(),
                input.outpoint.index
            );
            wb.delete(k.as_bytes())
        }

        self.output_value.write(wb, false)?;
        Ok(())
    }

    pub fn when_inscribe(&mut self, f: InscribeUpdater) {
        self.inscribe_updaters.push(f);
    }

    pub fn when_transfer(&mut self, f: TransferUpdater) {
        self.transfer_updaters.push(f);
    }
}

impl Drop for Ordi {
    fn drop(&mut self) {
        info!("Start closing Ordi instance.");
        self.close();
        info!("Closed Ordi instance.");
    }
}

#[derive(Clone)]
pub enum Origin {
    New {
        cursed: bool,
        unbound: bool,
        inscription: Inscription,
    },
    Old {
        old_output: String,
        old_offset: u64,
    },
}

#[derive(Clone)]
pub struct Flotsam {
    pub inscription_id: String,
    pub offset: u64,
    pub origin: Origin,
}
