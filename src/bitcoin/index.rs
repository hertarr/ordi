use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;

use bitcoin::hashes::{sha256d, Hash};
use byteorder::ReadBytesExt;
use log::info;
use rusty_leveldb::{LdbIterator, Options, Status, DB};
use thiserror::Error;

use crate::bitcoin::blk::{BlkError, BLK};
use crate::bitcoin::proto::block::Block;

const INDEX_PATH: &str = "blocks/index";
pub const FIRST_INSCRIPTION_HEIGHT: u64 = 767430;
const DEFAULT_INSCRIPTION_HEIGHT: usize = 100000;
const _DEFAULT_BLK_NUM: usize = 10000;

const BLOCK_VALID_CHAIN: u64 = 4;
const BLOCK_HAVE_DATA: u64 = 8;

#[derive(Error, Debug)]
pub enum IndexError {
    #[error("Blk error: `{0}`")]
    BlkError(#[from] BlkError),
    #[error("Database index not found: `{0}`")]
    DatabaseNotFound(String),
    #[error("Invalid height `{0}` in entry, expect: `{1}`")]
    InvalidHeight(u64, u64),
    #[error("Entry not found")]
    EntryNotFound,
    #[error("Open database error: `{0}`")]
    OpenDatabase(#[from] Status),
    #[error("Read varint")]
    IOError(#[from] std::io::Error),
}

fn parse_index_for_ordinals(
    btc_data_dir: &PathBuf,
) -> Result<
    (
        HashMap<u64, IndexEntry>,
        u64,
        HashMap<u64, u64>,
        HashMap<u64, BLK>,
    ),
    IndexError,
> {
    let index_path = btc_data_dir.join(INDEX_PATH);
    if !index_path.exists() {
        return Err(IndexError::DatabaseNotFound(
            index_path.to_str().unwrap().to_string(),
        ));
    }

    let mut index = HashMap::with_capacity(DEFAULT_INSCRIPTION_HEIGHT * 10);
    let mut max_height: u64 = 0;
    let mut max_height_in_blk = HashMap::new();
    let mut blks = HashMap::new();
    let mut iter = DB::open(index_path, Options::default())?.new_iter()?;
    let (mut key, mut value) = (vec![], vec![]);

    while iter.advance() {
        iter.current(&mut key, &mut value);
        if is_block_index_entry(&key) {
            let record = IndexEntry::from_leveldb_kv(&key[1..], &value)?;
            if record.status & (BLOCK_VALID_CHAIN | BLOCK_HAVE_DATA | BLOCK_VALID_CHAIN) > 0 {
                let height_in_blk = max_height_in_blk
                    .entry(record.blk_index)
                    .or_insert(record.height);
                if record.height > *height_in_blk {
                    *height_in_blk = record.height;
                }

                blks.entry(record.blk_index)
                    .or_insert(BLK::new(btc_data_dir.clone(), record.blk_index));

                if record.height > max_height {
                    max_height = record.height;
                }

                index.insert(record.height, record);
            }
        }
    }

    for (height, entry) in index.iter() {
        if entry.height != *height {
            return Err(IndexError::InvalidHeight(entry.height, *height));
        }
    }

    info!("All index entries are valid until height: {}.", max_height);
    Ok((index, max_height, max_height_in_blk, blks))
}

pub struct Index {
    pub btc_data_dir: PathBuf,
    pub entries: HashMap<u64, IndexEntry>,
    pub max_height: u64,
    pub max_height_in_blk: HashMap<u64, u64>,
    pub blks: HashMap<u64, BLK>,
}

impl Index {
    pub fn new(btc_data_dir: PathBuf) -> Result<Index, IndexError> {
        let start = std::time::Instant::now();

        let (entries, max_height, max_height_in_blk, blks) =
            parse_index_for_ordinals(&btc_data_dir)?;
        info!("Parsed bitcoin index, {}s.", start.elapsed().as_secs());

        Ok(Index {
            btc_data_dir,
            entries,
            max_height,
            max_height_in_blk,
            blks,
        })
    }

    pub fn catch_block(&mut self, height: u64) -> Result<Block, IndexError> {
        let (blk_index, data_offset) = self
            .get_index_entry(height)
            .map(|entry| (entry.blk_index, entry.data_offset))
            .expect("Invalid height.");
        let blk = self.blks.get_mut(&blk_index).unwrap();
        blk.open();

        let block = blk.read_block(data_offset);

        if *self.max_height_in_blk.get(&blk_index).unwrap() == height {
            blk.close();
            // todo: remove relevant variables
        }

        Ok(block?)
    }

    pub fn get_index_entry(&self, height: u64) -> Option<&IndexEntry> {
        self.entries.get(&height)
    }

    pub fn get_block_entry_by_block_hash(
        &mut self,
        block_hash: &[u8],
    ) -> Result<IndexEntry, IndexError> {
        let index_path = self.btc_data_dir.join(INDEX_PATH);
        if !index_path.exists() {
            return Err(IndexError::DatabaseNotFound(
                index_path.to_str().unwrap().to_string(),
            ));
        }

        let mut iter = DB::open(index_path, Options::default())?.new_iter()?;
        let (mut key, mut value) = (vec![], vec![]);
        iter.seek(block_hash);
        iter.current(&mut key, &mut value);

        if is_block_index_entry(&key) {
            return Ok(IndexEntry::from_leveldb_kv(&key[1..], &value)?);
        }

        return Err(IndexError::EntryNotFound);
    }
}

pub struct IndexEntry {
    pub block_hash: sha256d::Hash,
    pub blk_index: u64,
    pub data_offset: u64,
    pub version: u64,
    pub height: u64,
    pub status: u64,
    pub tx_count: u64,
}

impl IndexEntry {
    fn from_leveldb_kv(key: &[u8], value: &[u8]) -> Result<IndexEntry, IndexError> {
        let mut reader = Cursor::new(value);

        let block_hash: [u8; 32] = key.try_into().expect("malformed blockhash");
        let version = read_varint(&mut reader)?;
        let height = read_varint(&mut reader)?;
        let status = read_varint(&mut reader)?;
        let tx_count = read_varint(&mut reader)?;
        let blk_index = read_varint(&mut reader)?;
        let data_offset = read_varint(&mut reader)?;

        Ok(IndexEntry {
            block_hash: sha256d::Hash::from_byte_array(block_hash),
            blk_index,
            data_offset,
            version,
            height,
            status,
            tx_count,
        })
    }
}

#[inline]
fn is_block_index_entry(data: &[u8]) -> bool {
    *data.first().unwrap() == b'b'
}

/// TODO: this is a wonky 1:1 translation from https://github.com/bitcoin/bitcoin
/// It is NOT the same as CompactSize.
fn read_varint(reader: &mut Cursor<&[u8]>) -> Result<u64, IndexError> {
    let mut n = 0;
    loop {
        let ch_data = reader.read_u8()?;
        if n > u64::MAX >> 7 {
            panic!("size too large");
        }
        n = (n << 7) | (ch_data & 0x7F) as u64;
        if ch_data & 0x80 > 0 {
            if n == u64::MAX {
                panic!("size too large");
            }
            n += 1;
        } else {
            break;
        }
    }
    Ok(n)
}
