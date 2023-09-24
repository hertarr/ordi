use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use bitcoin::hashes::sha256d;

pub mod blk;
pub mod index;
pub mod proto;

mod block_reader;
mod common;

/// Trait to specify the underlying coin of a blockchain
/// Needs a proper magic value and a network id for address prefixes
pub trait Coin {
    // Human readable coin name
    fn name(&self) -> String;
    // Magic value to identify blocks
    fn magic(&self) -> u32;
    // https://en.bitcoin.it/wiki/List_of_address_prefixes
    fn version_id(&self) -> u8;
    // Returns genesis hash
    fn genesis(&self) -> sha256d::Hash;
    // Activates AuxPow for the returned version and above
    fn aux_pow_activation_version(&self) -> Option<u32> {
        None
    }
    // Default working directory to look for datadir, for example .bitcoin
    fn default_folder(&self) -> PathBuf;
}

pub struct Bitcoin;

#[derive(Clone)]
// Holds the selected coin type information
pub struct CoinType {
    pub name: String,
    pub magic: u32,
    pub version_id: u8,
    pub genesis_hash: sha256d::Hash,
    pub aux_pow_activation_version: Option<u32>,
    pub default_folder: PathBuf,
}

impl<T: Coin> From<T> for CoinType {
    fn from(coin: T) -> Self {
        CoinType {
            name: coin.name(),
            magic: coin.magic(),
            version_id: coin.version_id(),
            genesis_hash: coin.genesis(),
            aux_pow_activation_version: coin.aux_pow_activation_version(),
            default_folder: coin.default_folder(),
        }
    }
}

impl Coin for Bitcoin {
    fn name(&self) -> String {
        String::from("Bitcoin")
    }
    fn magic(&self) -> u32 {
        0xd9b4bef9
    }
    fn version_id(&self) -> u8 {
        0x00
    }
    fn genesis(&self) -> sha256d::Hash {
        sha256d::Hash::from_str("000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f")
            .unwrap()
    }
    fn default_folder(&self) -> PathBuf {
        Path::new(".bitcoin").join("blocks")
    }
}
