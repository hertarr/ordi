use std::{
    fs::File,
    io::{Seek, SeekFrom},
    path::PathBuf,
};

use byteorder::{LittleEndian, ReadBytesExt};
use seek_bufread::BufReader;
use thiserror::Error;

use crate::bitcoin::block_reader::BlockchainRead;
use crate::bitcoin::proto::block::Block;
use crate::bitcoin::Bitcoin;

#[derive(Error, Debug)]
pub enum BlkError {
    #[error("Anyhow error")]
    AnyhowError(#[from] anyhow::Error),
    #[error("IO error")]
    IOError(#[from] std::io::Error),
}

pub struct BLK {
    btc_data_dir: PathBuf,
    index: u64,
    reader: Option<BufReader<File>>,
}

impl BLK {
    pub fn new(btc_data_dir: PathBuf, index: u64) -> BLK {
        BLK {
            btc_data_dir,
            index,
            reader: None,
        }
    }

    pub fn open(&mut self) {
        if self.reader.is_none() {
            let blk_filename = format!("blk{:0>5}.dat", self.index);
            let blk_filepath = self.btc_data_dir.join("blocks").join(blk_filename);

            let file = File::open(blk_filepath.clone())
                .expect(format!("blk file: {:?} not found.", blk_filepath.as_os_str()).as_str());
            self.reader = Some(BufReader::new(file));
        }
    }

    pub fn close(&mut self) {
        if self.reader.is_some() {
            self.reader = None;
        }
    }

    pub fn read_block(&mut self, data_offset: u64) -> Result<Block, BlkError> {
        let reader = self.reader.as_mut().unwrap();
        reader.seek(SeekFrom::Start(data_offset - 4))?;
        let block_size = reader.read_u32::<LittleEndian>()?;
        let coin = Bitcoin.into();
        Ok(reader.read_block(block_size, &coin)?)
    }
}
