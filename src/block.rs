use std::collections::{BTreeMap, HashMap};
use std::num::TryFromIntError;
use std::string::FromUtf8Error;

use bitcoincore_rpc::{Client, RpcApi};
use log::{debug, info, trace};
use rusty_leveldb::{Status, WriteBatch, DB};
use thiserror::Error;

use crate::{
    bitcoin::proto::{tx::EvaluatedTx, Hashed},
    height::Height,
    inscription::{Curse, Inscription},
    Flotsam, Origin,
};

pub type Tx = Hashed<EvaluatedTx>;
pub type ProtoBlock = crate::bitcoin::proto::block::Block;

const UNBOUND_INSCRIPTIONS: &str = "unbound_inscriptions";
const NEXT_CURSED_ID_NUMBER: &str = "next_cursed_id_number";
const NEXT_ID_NUMBER: &str = "next_id_number";
const LOST_SATS: &str = "lost_sats";
const INDEXED_HEIGHT: &str = "indexed_height";

pub struct InscribeEntry<'a> {
    pub id: i64,
    pub inscription_id: &'a String,
    pub inscription: &'a Inscription,
    // Not genesis_tx, first output_tx.
    pub txid: &'a String,
    pub vout: u32,
    pub to_address: &'a Option<String>,
    pub height: u64,
    pub timestamp: u32,
}

pub struct TransferEntry<'a> {
    pub inscription_id: &'a String,
    pub from_output: &'a String,
    pub from_offset: u64,
    pub to: &'a Option<String>,
    pub txid: &'a String,
    pub vout: u32,
    pub offset: u64,
    pub height: u64,
    pub timestamp: u32,
}

pub type InscribeUpdater = fn(InscribeEntry);
pub type TransferUpdater = fn(TransferEntry);

#[derive(Error, Debug)]
pub enum BlockUpdaterError {
    #[error("InscriptionUpdater error: `{0}`")]
    InscriptionUpdaterError(#[from] InscriptionUpdaterError),
}

pub struct BlockUpdater<'ordi> {
    pub height: u64,
    pub block: ProtoBlock,
    pub btc_rpc_client: &'ordi Client,
    pub status: &'ordi mut DB,
    pub output_value: &'ordi mut DB,
    pub id_inscription: &'ordi mut DB,
    pub inscription_output: &'ordi mut DB,
    pub output_inscription: &'ordi mut DB,
    inscribe_updaters: &'ordi Vec<InscribeUpdater>,
    transfer_updaters: &'ordi Vec<TransferUpdater>,
}

impl<'ordi> BlockUpdater<'ordi> {
    pub fn new(
        height: u64,
        block: ProtoBlock,
        btc_rpc_client: &'ordi Client,
        status: &'ordi mut DB,
        output_value: &'ordi mut DB,
        id_inscription: &'ordi mut DB,
        inscription_output: &'ordi mut DB,
        output_inscription: &'ordi mut DB,
        inscribe_updaters: &'ordi Vec<InscribeUpdater>,
        transfer_updaters: &'ordi Vec<TransferUpdater>,
    ) -> BlockUpdater<'ordi> {
        BlockUpdater {
            height,
            block,
            btc_rpc_client,
            status,
            output_value,
            id_inscription,
            inscription_output,
            output_inscription,
            inscribe_updaters,
            transfer_updaters,
        }
    }

    pub fn index_transactions(&mut self) -> Result<(), BlockUpdaterError> {
        let start = std::time::Instant::now();
        let mut inscription_updater = InscriptionUpdater::new(
            self.height,
            self.block.header.value.timestamp,
            &self.block,
            &self.btc_rpc_client,
            &mut self.status,
            &mut self.output_value,
            &mut self.id_inscription,
            &mut self.inscription_output,
            &mut self.output_inscription,
            self.inscribe_updaters,
            self.transfer_updaters,
        );

        for (_, tx) in self
            .block
            .txs
            .iter()
            .skip(1)
            .chain(self.block.txs.first())
            .enumerate()
        {
            inscription_updater.index_inscriptions_in_transaction(tx)?;
        }

        inscription_updater.flush_update()?;
        info!(
            "Indexed block: {}, used {}s.",
            self.height,
            start.elapsed().as_secs()
        );
        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum InscriptionUpdaterError {
    #[error("String from utf-8 error: `{0}`")]
    StringFromUtf8Error(#[from] FromUtf8Error),
    #[error("Bitcoin rpc error: `{0}`")]
    BitcoinRpcError(#[from] bitcoincore_rpc::Error),
    #[error("Try from int error: `{0}`")]
    TryFromIntError(#[from] TryFromIntError),
    #[error("Write leveldb error: `{0}`")]
    WriteLevelDBError(#[from] Status),
}

pub struct InscriptionUpdater<'block> {
    pub height: u64,
    pub timestamp: u32,
    pub block: &'block ProtoBlock,
    pub btc_rpc_client: &'block Client,
    pub status: &'block mut DB,
    pub output_value: &'block mut DB,
    pub id_inscription: &'block mut DB,
    pub inscription_output: &'block mut DB,
    pub output_inscription: &'block mut DB,
    status_wb: WriteBatch,
    output_value_wb: WriteBatch,
    id_inscription_wb: WriteBatch,
    inscription_output_wb: WriteBatch,
    output_inscription_wb: WriteBatch,
    pub flotsam: Vec<Flotsam>,
    pub reward: u64,
    pub unbound_inscriptions: u64,
    pub next_number: i64,
    pub next_cursed_number: i64,
    pub lost_sats: u64,
    output_inscription_cache: HashMap<String, String>,
    inscribe_updaters: &'block Vec<InscribeUpdater>,
    transfer_updaters: &'block Vec<TransferUpdater>,
}

impl<'block> InscriptionUpdater<'block> {
    pub fn new(
        height: u64,
        timestamp: u32,
        block: &'block ProtoBlock,
        btc_rpc_client: &'block Client,
        status: &'block mut DB,
        output_value: &'block mut DB,
        id_inscription: &'block mut DB,
        inscription_output: &'block mut DB,
        output_inscription: &'block mut DB,
        inscribe_updaters: &'block Vec<InscribeUpdater>,
        transfer_updaters: &'block Vec<TransferUpdater>,
    ) -> InscriptionUpdater<'block> {
        let mut iu = InscriptionUpdater {
            height,
            timestamp,
            block,
            btc_rpc_client,
            status,
            output_value,
            id_inscription,
            inscription_output,
            output_inscription,
            status_wb: WriteBatch::new(),
            output_value_wb: WriteBatch::new(),
            id_inscription_wb: WriteBatch::new(),
            inscription_output_wb: WriteBatch::new(),
            output_inscription_wb: WriteBatch::new(),
            flotsam: vec![],
            reward: Height(height).subsidy(),
            unbound_inscriptions: 0,
            next_number: 0,
            next_cursed_number: 0,
            lost_sats: 0,
            output_inscription_cache: HashMap::new(),
            inscribe_updaters,
            transfer_updaters,
        };

        iu.unbound_inscriptions = iu.status_value_u64(UNBOUND_INSCRIPTIONS);
        let mut next_cursed_number = iu.status_value_i64(NEXT_CURSED_ID_NUMBER);
        if next_cursed_number == 0 {
            next_cursed_number -= 1;
        }
        iu.next_cursed_number = next_cursed_number;
        iu.next_number = iu.status_value_i64(NEXT_ID_NUMBER);
        iu.lost_sats = iu.status_value_u64(LOST_SATS);

        iu
    }

    fn index_inscriptions_in_transaction(
        &mut self,
        tx: &Hashed<EvaluatedTx>,
    ) -> Result<(), InscriptionUpdaterError> {
        debug!("Handle Tx: {}", tx.hash.to_string());
        let mut new_inscriptions = Inscription::from_transaction(tx).into_iter().peekable();
        let mut floating_inscriptions = vec![];
        let mut inscribed_offsets = BTreeMap::new();
        let mut input_value = 0;
        let mut id_counter = 0;

        let mut wb = WriteBatch::new();
        for (input_index, tx_in) in tx.value.inputs.iter().enumerate() {
            if tx_in.outpoint.is_null() {
                input_value += Height(self.height).subsidy();
                continue;
            }

            let previous_output = format!("{}:{}", tx_in.outpoint.txid, tx_in.outpoint.index);
            let inscriptions_str = match self.output_inscription_cache.get(&previous_output) {
                Some(inscriptions) => inscriptions.clone(),
                None => {
                    let value = String::from_utf8(
                        self.output_inscription
                            .get(previous_output.as_bytes())
                            .unwrap_or_default(),
                    )?;
                    if value != "" {
                        self.output_inscription_cache
                            .insert(previous_output.clone(), value.clone());
                    }

                    value
                }
            };
            if inscriptions_str != "" {
                for (inscription_id, inscription_offset) in
                    inscriptions_str
                        .split("/")
                        .skip(1)
                        .map(|inscription_offset| {
                            let inscription_offset =
                                inscription_offset.split(":").collect::<Vec<&str>>();
                            let inscription_id = inscription_offset[0];
                            let offset = inscription_offset[1].parse::<u64>().unwrap();

                            (inscription_id, offset)
                        })
                {
                    let offset = input_value + inscription_offset;
                    floating_inscriptions.push(Flotsam {
                        inscription_id: inscription_id.to_string(),
                        offset,
                        origin: Origin::Old {
                            old_output: previous_output.clone(),
                            old_offset: inscription_offset,
                        },
                    });

                    inscribed_offsets
                        .entry(offset)
                        .and_modify(|(_id, count)| *count += 1)
                        .or_insert((inscription_id.to_string(), 0));
                }
            }
            let offset = input_value;

            input_value += {
                let k = format!(
                    "{}:{}",
                    tx_in.outpoint.txid.to_string(),
                    tx_in.outpoint.index
                );
                match self.output_value.get(k.as_bytes()) {
                    Some(value_vec) => {
                        let value = u64::from_le_bytes(value_vec.try_into().unwrap());
                        trace!(
                            "Retrieve output_value:{}, output: {}. Raw is from leveldb.",
                            value,
                            k
                        );
                        value
                    }
                    None => {
                        let previous_tx = self.btc_rpc_client.get_raw_transaction(
                            &bitcoin::Txid::from_raw_hash(tx_in.outpoint.txid.clone()),
                            None,
                        )?;
                        let value = previous_tx.output[tx_in.outpoint.index as usize].value;
                        trace!(
                            "Retrieve output_value:{}, output: {}. Raw is from bitcoin node.",
                            value,
                            k
                        );
                        value
                    }
                }
            };

            while let Some(new_inscription) = new_inscriptions.peek_mut() {
                if new_inscription.tx_in_index != u32::try_from(input_index)? {
                    break;
                }

                let inscription_id = format!("{}i{}", tx.hash.to_string(), id_counter);

                let curse = if new_inscription.tx_in_index != 0 {
                    Some(Curse::NotInFirstInput)
                } else if new_inscription.tx_in_offset != 0 {
                    Some(Curse::NotAtOffsetZero)
                } else if inscribed_offsets.contains_key(&offset) {
                    // todo, not necessary: insert (re-inscription_id, seq num)
                    Some(Curse::Reinscription)
                } else {
                    None
                };

                let cursed = if let Some(Curse::Reinscription) = curse {
                    let first_reinscription = inscribed_offsets
                        .get(&offset)
                        .map(|(_id, count)| *count == 0)
                        .unwrap_or(false);

                    let initial_inscription_is_cursed = inscribed_offsets
                        .get(&offset)
                        .and_then(|(inscription_id, _count)| {
                            Some(self.status_value_i64(inscription_id.as_str()) != 0)
                        })
                        .unwrap();

                    let cursed = !(first_reinscription && initial_inscription_is_cursed);
                    info!(
                        "new_inscription: {}, reinscription: {}, first_reinscription: {}, initial_inscription_is_cursed: {}",
                        &inscription_id,
                        cursed, first_reinscription,
                        initial_inscription_is_cursed
                    );
                    cursed
                } else {
                    curse.is_some()
                };

                let unbound = input_value == 0 || new_inscription.tx_in_offset != 0;

                debug!(
                    "Found inscription: {}, offset: {}, input_value: {}.",
                    &inscription_id, offset, input_value
                );
                floating_inscriptions.push(Flotsam {
                    inscription_id,
                    offset,
                    origin: Origin::New {
                        cursed,
                        unbound,
                        inscription: new_inscription.inscription.clone(),
                    },
                });

                new_inscriptions.next();
                id_counter += 1;
            }

            let k = format!(
                "{}:{}",
                tx_in.outpoint.txid.to_string(),
                tx_in.outpoint.index
            );
            wb.delete(k.as_bytes())
        }

        //let total_output_value = tx.value.outputs.iter().map(|txout| txout.out.value).sum::<u64>();
        // todo, not necessary: calculate fee

        let is_coinbase = tx
            .value
            .inputs
            .first()
            .map(|tx_in| tx_in.outpoint.is_null())
            .unwrap_or_default();

        if is_coinbase {
            floating_inscriptions.append(&mut self.flotsam);
        }

        floating_inscriptions.sort_by_key(|float| float.offset);
        let mut inscriptions = floating_inscriptions.into_iter().peekable();

        let mut output_value = 0;
        for (vout, tx_out) in tx.value.outputs.iter().enumerate() {
            let k = format!("{}:{}", tx.hash.to_string(), vout);
            wb.put(k.as_bytes(), tx_out.out.value.to_le_bytes().as_slice());

            let end = output_value + tx_out.out.value;

            while let Some(flotsam) = inscriptions.peek() {
                if flotsam.offset >= end {
                    break;
                }

                let offset = flotsam.offset - output_value;
                let vout = vout as u32;
                let flotsam = inscriptions.next().unwrap();
                self.update_inscription_state(
                    flotsam,
                    tx.hash.to_string(),
                    vout,
                    offset,
                    &tx_out.script.address,
                )?;
            }

            output_value = end;
        }

        self.output_value.write(wb, false)?;

        if is_coinbase {
            for flotsam in inscriptions {
                let new_txid = null_hash();
                let new_offset = self.lost_sats + flotsam.offset - output_value;

                self.update_inscription_state(flotsam, new_txid, u32::MAX, new_offset, &None)?;
            }

            self.lost_sats += self.reward - output_value;
        } else {
            self.flotsam.extend(inscriptions.map(|flotsam| Flotsam {
                offset: self.reward + flotsam.offset - output_value,
                ..flotsam
            }));
            self.reward += input_value - output_value;
        }

        Ok(())
    }

    pub fn update_inscription_state(
        &mut self,
        flotsam: Flotsam,
        new_txid: String,
        vout: u32,
        offset: u64,
        address: &Option<String>,
    ) -> Result<(), InscriptionUpdaterError> {
        let unbound = match flotsam.origin {
            Origin::Old {
                old_output,
                old_offset,
            } => {
                let inscription_value = self
                    .output_inscription_cache
                    .entry(old_output.clone())
                    .or_insert_with(|| {
                        String::from_utf8(
                            self.output_inscription
                                .get(old_output.as_bytes())
                                .unwrap_or_default(),
                        )
                        .unwrap()
                    });

                let inscription_in_output_inscription =
                    format!("/{}:{}", &flotsam.inscription_id, old_offset);
                *inscription_value =
                    inscription_value.replace(inscription_in_output_inscription.as_str(), "");

                for transfer_updater in self.transfer_updaters.iter() {
                    transfer_updater(TransferEntry {
                        inscription_id: &flotsam.inscription_id,
                        from_output: &old_output,
                        from_offset: old_offset,
                        to: address,
                        txid: &new_txid,
                        vout,
                        offset,
                        height: self.height,
                        timestamp: self.timestamp,
                    })
                }

                false
            }
            Origin::New {
                cursed,
                unbound,
                inscription,
            } => {
                let number: i64 = if cursed {
                    let next_cursed_number = self.next_cursed_number;
                    self.next_cursed_number -= 1;

                    self.status.put(
                        flotsam.inscription_id.as_bytes(),
                        next_cursed_number.to_le_bytes().as_slice(),
                    )?;

                    next_cursed_number
                } else {
                    let next_number = self.next_number;
                    self.next_number += 1;

                    next_number
                };

                self.id_inscription_wb.put(
                    number.to_le_bytes().as_slice(),
                    flotsam.inscription_id.as_bytes(),
                );

                // todo, not necessary: sat

                // todo, not necessary: map inscription_id to entry(height, number, timestamp[, sat])

                for inscribe_updater in self.inscribe_updaters.iter() {
                    inscribe_updater(InscribeEntry {
                        id: number,
                        inscription_id: &flotsam.inscription_id,
                        inscription: &inscription,
                        txid: &new_txid,
                        vout,
                        to_address: address,
                        height: self.height,
                        timestamp: self.timestamp,
                    });
                }

                unbound
            }
        };

        let real_new_txid = if unbound {
            let new_unbound_satpoint =
                format!("{}:{}", unbound_outpoint(), self.unbound_inscriptions);
            self.unbound_inscriptions += 1;

            new_unbound_satpoint
        } else {
            format!("{}:{}", new_txid, vout)
        };

        let previous_data = self
            .output_inscription_cache
            .entry(real_new_txid.clone())
            .or_insert_with(|| {
                String::from_utf8(
                    self.output_inscription
                        .get(real_new_txid.as_bytes())
                        .unwrap_or_default(),
                )
                .unwrap()
            });
        *previous_data = format!(
            "{}/{}:{}",
            previous_data,
            flotsam.inscription_id.as_str(),
            offset
        );

        self.inscription_output_wb
            .put(flotsam.inscription_id.as_bytes(), real_new_txid.as_bytes());

        Ok(())
    }

    pub fn flush_update(mut self) -> Result<(), InscriptionUpdaterError> {
        self.write_status_wb_str_to_u64(UNBOUND_INSCRIPTIONS, self.unbound_inscriptions);
        self.write_status_wb_str_to_i64(NEXT_ID_NUMBER, self.next_number);
        self.write_status_wb_str_to_i64(NEXT_CURSED_ID_NUMBER, self.next_cursed_number);
        self.write_status_wb_str_to_u64(LOST_SATS, self.lost_sats);
        self.write_status_wb_str_to_u64(INDEXED_HEIGHT, self.height);

        if self.output_value_wb.count() > 0 {
            self.output_value.write(self.output_value_wb, false)?;
        }

        if self.id_inscription_wb.count() > 0 {
            self.id_inscription.write(self.id_inscription_wb, false)?;
        }

        if self.inscription_output_wb.count() > 0 {
            self.inscription_output
                .write(self.inscription_output_wb, false)?;
        }

        for (output, inscriptions) in self.output_inscription_cache {
            if inscriptions != "" {
                self.output_inscription_wb
                    .put(output.as_bytes(), inscriptions.as_bytes());
            } else {
                self.output_inscription_wb.delete(output.as_bytes());
            }
        }

        if self.output_inscription_wb.count() > 0 {
            self.output_inscription
                .write(self.output_inscription_wb, false)?;
        }

        if self.status_wb.count() > 0 {
            self.status.write(self.status_wb, false)?;
        }

        Ok(())
    }

    #[inline]
    fn write_status_wb_str_to_u64(&mut self, k: &str, v: u64) {
        self.status_wb.put(k.as_bytes(), v.to_le_bytes().as_slice());
    }

    #[inline]
    fn write_status_wb_str_to_i64(&mut self, k: &str, v: i64) {
        self.status_wb.put(k.as_bytes(), v.to_le_bytes().as_slice());
    }

    #[inline]
    fn status_value_u64(&mut self, k: &str) -> u64 {
        u64::from_le_bytes(
            self.status
                .get(k.as_bytes())
                .unwrap_or(vec![0; 8])
                .try_into()
                .unwrap(),
        )
    }

    #[inline]
    fn status_value_i64(&mut self, k: &str) -> i64 {
        i64::from_le_bytes(
            self.status
                .get(k.as_bytes())
                .unwrap_or(vec![0; 8])
                .try_into()
                .unwrap(),
        )
    }
}

#[inline]
fn unbound_outpoint() -> String {
    "0000000000000000000000000000000000000000000000000000000000000000:0".to_string()
}

#[inline]
#[allow(dead_code)]
fn null_outpoint() -> String {
    "0000000000000000000000000000000000000000000000000000000000000000:4294967295".to_string()
}

#[inline]
fn null_hash() -> String {
    "0000000000000000000000000000000000000000000000000000000000000000".to_string()
}
