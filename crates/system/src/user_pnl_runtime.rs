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

//! Kernel-backed [`UserPnLRuntime`](nautilus_trading::examples::strategies::UserPnLRuntime).

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use nautilus_common::{component::component_state, enums::ComponentState};
use nautilus_model::{enums::TradingState, identifiers::StrategyId};
use nautilus_risk::engine::RiskEngine;
use nautilus_trading::examples::strategies::UserPnLRuntime;

use crate::trader::Trader;

/// Flatten / stop / start / trading-state hooks used by the `UserPnL` sidecar.
pub struct KernelUserPnLRuntime {
    /// Kernel trader that owns registered strategies.
    pub trader: Rc<RefCell<Trader>>,
    /// Kernel risk engine used to latch `ACTIVE` / `REDUCING` / `HALTED`.
    pub risk_engine: Rc<RefCell<RiskEngine>>,
}

impl Debug for KernelUserPnLRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KernelUserPnLRuntime)).finish()
    }
}

impl UserPnLRuntime for KernelUserPnLRuntime {
    fn registered_strategy_ids(&self) -> Vec<StrategyId> {
        self.trader.borrow().strategy_ids()
    }

    fn exit_strategy(&self, strategy_id: StrategyId) -> anyhow::Result<()> {
        Trader::market_exit_strategy(&self.trader, &strategy_id)
    }

    fn stop_strategy(&self, strategy_id: StrategyId) -> anyhow::Result<bool> {
        self.trader.borrow_mut().stop_strategy(&strategy_id)?;
        Ok(component_state(&strategy_id.inner())
            .is_ok_and(|state| state == ComponentState::Stopped))
    }

    fn start_strategy(&self, strategy_id: StrategyId) -> anyhow::Result<()> {
        if component_state(&strategy_id.inner()).is_ok_and(|state| state == ComponentState::Running)
        {
            return Ok(());
        }

        self.trader.borrow().start_strategy(&strategy_id)
    }

    fn set_trading_state(&self, state: TradingState) -> anyhow::Result<()> {
        self.risk_engine.borrow_mut().set_trading_state(state);
        Ok(())
    }
}
