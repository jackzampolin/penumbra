use penumbra_proto::{
    stake::{self as pb},
    Protobuf,
};
use serde::{Deserialize, Serialize};

use crate::{Epoch, IdentityKey, FundingStream};

/// FIXME: set this less arbitrarily, and allow this to be set per-epoch
/// 3bps -> 11% return over 365 epochs, why not
const BASE_REWARD_RATE: u64 = 3_0000;

/// Describes a validator's reward rate and voting power in some epoch.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(try_from = "pb::RateData", into = "pb::RateData")]
pub struct RateData {
    /// The validator's identity key.
    pub identity_key: IdentityKey,
    /// The index of the epoch for which this rate is valid.
    pub epoch_index: u64,
    /// The validator's voting power.
    pub voting_power: u64,
    /// The validator-specific reward rate.
    pub validator_reward_rate: u64,
    /// The validator-specific exchange rate.
    pub validator_exchange_rate: u64,
}

impl RateData {
    pub fn next_rates(
        &self,
        base_rate_data: &BaseRateData,
        funding_streams: Vec<FundingStream>,
    ) -> RateData {
        // compute the validator's total commissio
        let commission_rate_bps = funding_streams
            .iter()
            .fold(0u64, |total, stream| total + stream.rate_bps as u64);

        // compute next validator reward rate
        // 1 bps = 1e-4, so here we group digits by 4s rather than 3s as is usual
        let validator_reward_rate =
            ((1_0000_0000u64 - (commission_rate_bps * 1_0000)) * BASE_REWARD_RATE) / 1_0000_0000;

        // compute validator exchange rate
        let validator_exchange_rate = (self.validator_exchange_rate
            * (self.validator_reward_rate + 1_0000_0000))
            / 1_0000_0000;

        // this is supposed to be multiplied by the number of delegation tokens,
        // how do we track that?
        // 
        // todo: consider specifying the voting power function as a pure function of current epoch
        // state (delegation tokens, etc) instead of an adjustmenet function
        let voting_power_adjustment =
            (validator_exchange_rate * 1_0000_0000) / base_rate_data.base_exchange_rate;

        RateData {
            identity_key: self.identity_key.clone(),
            epoch_index: self.epoch_index + 1,
            voting_power: self.voting_power * voting_power_adjustment,
            validator_reward_rate: validator_reward_rate,
            validator_exchange_rate: validator_exchange_rate,
        }
    }
}
/// Describes the base reward and exchange rates in some epoch.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(try_from = "pb::BaseRateData", into = "pb::BaseRateData")]
pub struct BaseRateData {
    /// The index of the epoch for which this rate is valid.
    pub epoch_index: u64,
    /// The base reward rate.
    pub base_reward_rate: u64,
    /// The base exchange rate.
    pub base_exchange_rate: u64,
}

impl BaseRateData {
    /// compute the next base exchange rate, epoch index, and base reward rate based on the current
    /// rates and the supplied Epoch.
    pub fn next_base_rate(&self) -> BaseRateData {
        let base_exchange_rate =
            (self.base_exchange_rate * (BASE_REWARD_RATE + 1_0000_0000)) / 1_0000_0000;
        return BaseRateData {
            base_exchange_rate,
            base_reward_rate: BASE_REWARD_RATE,
            epoch_index: self.epoch_index + 1,
        };
    }
}

impl Protobuf<pb::RateData> for RateData {}

impl From<RateData> for pb::RateData {
    fn from(v: RateData) -> Self {
        pb::RateData {
            identity_key: Some(v.identity_key.into()),
            epoch_index: v.epoch_index,
            voting_power: v.voting_power,
            validator_reward_rate: v.validator_reward_rate,
            validator_exchange_rate: v.validator_exchange_rate,
        }
    }
}

impl TryFrom<pb::RateData> for RateData {
    type Error = anyhow::Error;
    fn try_from(v: pb::RateData) -> Result<Self, Self::Error> {
        Ok(RateData {
            identity_key: v
                .identity_key
                .ok_or_else(|| anyhow::anyhow!("missing identity key"))?
                .try_into()?,
            epoch_index: v.epoch_index,
            voting_power: v.voting_power,
            validator_reward_rate: v.validator_reward_rate,
            validator_exchange_rate: v.validator_exchange_rate,
        })
    }
}

impl Protobuf<pb::BaseRateData> for BaseRateData {}

impl From<BaseRateData> for pb::BaseRateData {
    fn from(v: BaseRateData) -> Self {
        pb::BaseRateData {
            epoch_index: v.epoch_index,
            base_reward_rate: v.base_reward_rate,
            base_exchange_rate: v.base_exchange_rate,
        }
    }
}

impl TryFrom<pb::BaseRateData> for BaseRateData {
    type Error = anyhow::Error;
    fn try_from(v: pb::BaseRateData) -> Result<Self, Self::Error> {
        Ok(BaseRateData {
            epoch_index: v.epoch_index,
            base_reward_rate: v.base_reward_rate,
            base_exchange_rate: v.base_exchange_rate,
        })
    }
}
