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

//! Account-level user PnL sidecar.
//!
//! Watches venue/account PnL on a timer. On `max_loss` or `max_profit` it latches
//! the risk engine to `REDUCING`, asks every other registered strategy to run
//! inherited `market_exit()`, waits until the watched account has no positions
//! and no live orders, then optionally latches `HALTED` and `stop()`s those
//! strategies for the rest of the UTC day.
//!
//! Trading algorithms do not implement flatten logic. They already inherit
//! `market_exit()` from [`Strategy`](crate::strategy::Strategy).

pub mod config;
pub mod runtime;
pub mod strategy;

#[cfg(test)]
mod tests;

pub use config::UserPnLConfig;
pub use runtime::UserPnLRuntime;
pub use strategy::{UserPnL, UserPnLState};
