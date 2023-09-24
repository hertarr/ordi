use bitcoin::constants::COIN_VALUE;
use derive_more::Display;

use crate::height::Height;

const SUBSIDY_HALVING_INTERVAL: u64 =
    bitcoin::blockdata::constants::SUBSIDY_HALVING_INTERVAL as u64;

#[derive(Copy, Clone, Eq, PartialEq, Debug, Display, PartialOrd)]
pub struct Epoch(pub u64);

impl Epoch {
    pub const FIRST_POST_SUBSIDY: Epoch = Self(33);

    pub fn subsidy(self) -> u64 {
        if self < Self::FIRST_POST_SUBSIDY {
            (50 * COIN_VALUE) >> self.0
        } else {
            0
        }
    }
}

impl From<Height> for Epoch {
    fn from(height: Height) -> Self {
        Self(height.0 / SUBSIDY_HALVING_INTERVAL)
    }
}
