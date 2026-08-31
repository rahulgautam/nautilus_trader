// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Configuration for the account-level `UserPnL` sidecar.

use nautilus_model::{
    identifiers::{AccountId, StrategyId, Venue},
    types::Currency,
};
use rust_decimal::Decimal;

use crate::strategy::StrategyConfig;

/// Configuration for the account-level user PnL sidecar.
///
/// Watches [`Portfolio`](nautilus_portfolio::Portfolio) PnL for one venue/account
/// and, on `max_loss` or `max_profit`, asks every other registered strategy to
/// `market_exit()` until the account has no positions and no live orders, then
/// optionally latches `HALTED` and stops those strategies for the rest of the UTC day.
#[derive(Debug, Clone, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.trading", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.trading")
)]
pub struct UserPnLConfig {
    /// Base strategy configuration.
    #[builder(default = StrategyConfig {
        strategy_id: Some(StrategyId::from("USER_PNL-001")),
        order_id_tag: Some("UPNL".to_string()),
        ..Default::default()
    })]
    pub base: StrategyConfig,
    /// Venue whose portfolio PnL is monitored.
    pub venue: Venue,
    /// Optional account scope. When `None`, aggregates the venue.
    pub account_id: Option<AccountId>,
    /// Currency of `max_loss` / `max_profit`.
    ///
    /// Portfolio PnL is requested in this currency (converted when a rate is
    /// available). Use the venue settlement currency so the kill switch does
    /// not depend on FX. A missing conversion fails open and logs a warning.
    pub currency: Currency,
    /// Trip when daily PnL is less than or equal to this value (must be ≤ 0).
    ///
    /// A `10_000` loss limit is expressed as `-10000`. `None` disables the bound.
    pub max_loss: Option<Decimal>,
    /// Trip when daily PnL is greater than or equal to this value (must be ≥ 0).
    ///
    /// `None` disables the bound.
    pub max_profit: Option<Decimal>,
    /// After a max-loss flatten, latch `HALTED` and stop other strategies for **that** UTC day.
    #[builder(default = true)]
    pub halt_day_on_max_loss: bool,
    /// After a max-profit flatten, latch `HALTED` and stop other strategies for **that** UTC day.
    #[builder(default = true)]
    pub halt_day_on_max_profit: bool,
    /// When true, compare unrealized PnL only. When false (default), use
    /// session total PnL (realized + unrealized).
    #[builder(default)]
    pub use_unrealized_only: bool,
    /// How often to sample PnL, in milliseconds.
    #[builder(default = 200)]
    pub check_interval_ms: u64,
    /// How often to re-issue `market_exit` while flattening, in milliseconds.
    ///
    /// Kept independent of `check_interval_ms` so PnL sampling can stay fast
    /// without flooding logs or the target's in-progress exit loop. Re-drives
    /// are only evaluated on a check, so the effective cadence is rounded up to
    /// a multiple of `check_interval_ms`.
    #[builder(default = 5_000)]
    pub flatten_redrive_ms: u64,
    /// How long a flatten may run before it is reported as stuck, in milliseconds.
    ///
    /// On expiry the sidecar logs an error and keeps re-issuing `market_exit`;
    /// it does not latch `HALTED` while residual risk remains, since `HALTED`
    /// would deny the remaining closes.
    #[builder(default = 30_000)]
    pub flatten_timeout_ms: u64,
    /// Explicit strategies to flatten and to include in the flat check.
    /// When empty, uses the node registration list (minus this sidecar) and
    /// waits for the whole venue/account to be empty. When set, the flat
    /// check is that allowlist only (other strategies on the same venue are
    /// ignored).
    #[builder(default)]
    pub managed_strategy_ids: Vec<StrategyId>,
    /// Skip `market_exit` for strategies with no open positions and no live orders.
    #[builder(default = true)]
    pub skip_flat_strategies: bool,
    /// When true (default), compare **today's** PnL against the bounds and
    /// re-arm (`ACTIVE` + idle, restart stopped strategies) at each UTC date
    /// change. When false, a halt is permanent until restart.
    #[builder(default = true)]
    pub reset_daily: bool,
}

impl UserPnLConfig {
    /// Checks that at least one bound is set, that signs are valid, and that the
    /// timing intervals are ordered.
    ///
    /// # Errors
    ///
    /// Returns an error if both bounds are `None`, `max_loss` is positive,
    /// `max_profit` is negative, `use_unrealized_only` is combined with
    /// `reset_daily`, `check_interval_ms` is zero, or the flatten intervals are
    /// shorter than the interval that drives them.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.max_loss.is_none() && self.max_profit.is_none() {
            anyhow::bail!("UserPnLConfig requires at least one of max_loss or max_profit");
        }

        if let Some(max_loss) = self.max_loss
            && max_loss > Decimal::ZERO
        {
            anyhow::bail!("max_loss must be less than or equal to 0, was {max_loss}");
        }

        if let Some(max_profit) = self.max_profit
            && max_profit < Decimal::ZERO
        {
            anyhow::bail!("max_profit must be greater than or equal to 0, was {max_profit}");
        }

        if self.use_unrealized_only && self.reset_daily {
            anyhow::bail!(
                "use_unrealized_only cannot be combined with reset_daily; unrealized PnL is a mark, not a running total"
            );
        }

        if self.check_interval_ms == 0 {
            anyhow::bail!("check_interval_ms must be greater than 0");
        }

        // Both flatten intervals are only evaluated on a check, so an interval
        // below the one driving it cannot be honoured.
        if self.flatten_redrive_ms < self.check_interval_ms {
            anyhow::bail!(
                "flatten_redrive_ms ({}) must be greater than or equal to check_interval_ms ({})",
                self.flatten_redrive_ms,
                self.check_interval_ms
            );
        }

        if self.flatten_timeout_ms < self.flatten_redrive_ms {
            anyhow::bail!(
                "flatten_timeout_ms ({}) must be greater than or equal to flatten_redrive_ms ({})",
                self.flatten_timeout_ms,
                self.flatten_redrive_ms
            );
        }

        Ok(())
    }
}
