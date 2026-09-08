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

//! Node-level hooks used by [`super::UserPnL`].
//!
//! `nautilus-trading` cannot depend on `nautilus-system`, so flatten, stop/start,
//! and trading-state latching are injected by the live/backtest node when the
//! strategy is registered.

use nautilus_model::{enums::TradingState, identifiers::StrategyId};

/// Node capabilities [`super::UserPnL`] needs but does not own.
pub trait UserPnLRuntime {
    /// Registered strategy IDs on the trader (not the cache order index).
    fn registered_strategy_ids(&self) -> Vec<StrategyId>;

    /// Ask a strategy to run its inherited `market_exit()` loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or has no control endpoint.
    fn exit_strategy(&self, strategy_id: StrategyId) -> anyhow::Result<()>;

    /// Stop a strategy so it cannot add risk after the account is flat.
    ///
    /// Returns `true` when the component is actually `Stopped`. `Trader::stop_strategy`
    /// can return `Ok` while deferring the stop (`manage_stop` with an in-flight exit).
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or cannot be stopped.
    fn stop_strategy(&self, strategy_id: StrategyId) -> anyhow::Result<bool>;

    /// Start a strategy that this sidecar previously stopped (UTC day roll).
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or cannot be started.
    fn start_strategy(&self, strategy_id: StrategyId) -> anyhow::Result<()>;

    /// Latch the risk engine trading state (`ACTIVE` / `REDUCING` / `HALTED`).
    ///
    /// # Errors
    ///
    /// Returns an error if the risk engine cannot be borrowed or updated.
    fn set_trading_state(&self, state: TradingState) -> anyhow::Result<()>;
}
