use derive_more::{Display, FromStr};

use crate::epoch::Epoch;

#[derive(Copy, Clone, Debug, Display, FromStr, Ord, Eq, PartialEq, PartialOrd)]
pub struct Height(pub u64);

impl Height {
    pub fn subsidy(self) -> u64 {
        Epoch::from(self).subsidy()
    }
}
