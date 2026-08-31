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

use std::{fmt::Debug, rc::Rc};

use jiff::{civil::Date, tz::Offset};
use nautilus_common::{actor::DataActor, timer::TimeEvent};
use nautilus_core::UnixNanos;
use nautilus_model::{enums::TradingState, identifiers::StrategyId};
use rust_decimal::Decimal;

use super::{config::UserPnLConfig, runtime::UserPnLRuntime};
use crate::{
    nautilus_strategy,
    strategy::{Strategy, StrategyCore},
};

const USER_PNL_TIMER: &str = "user_pnl";

/// Latch state for the account kill-switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UserPnLState {
    #[default]
    Idle,
    Flattening,
    Halted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TripBound {
    MaxLoss,
    MaxProfit,
}

/// Sidecar that trips account PnL and delegates flatten to each algo's `market_exit()`.
///
/// Algos do not implement anything. This strategy must not call `self.market_exit()`;
/// it owns no book.
pub struct UserPnL {
    pub(super) core: StrategyCore,
    pub(super) config: UserPnLConfig,
    pub(super) state: UserPnLState,
    runtime: Option<Rc<dyn UserPnLRuntime>>,
    /// UTC calendar date of the current daily window (`reset_daily` only).
    session_date: Option<Date>,
    /// Session `total_pnls` captured at the last UTC date change.
    day_anchor_pnl: Option<Decimal>,
    trip_bound: Option<TripBound>,
    /// UTC date on which the current flatten was tripped (`reset_daily` halt is that day only).
    trip_date: Option<Date>,
    /// After a no-halt flatten, ignore the same bound until PnL is back inside the band.
    waiting_to_rearm: bool,
    stopped_for_halt: Vec<StrategyId>,
    flatten_started: Option<UnixNanos>,
    last_exit_request: Option<UnixNanos>,
    flatten_timed_out: bool,
    /// True only when this sidecar latched `HALTED` (so day-roll may restore `ACTIVE`).
    latched_halted: bool,
    warned_missing_pnl: bool,
}

impl UserPnL {
    /// Creates a new [`UserPnL`] instance from a validated config.
    ///
    /// # Errors
    ///
    /// Returns an error if [`UserPnLConfig::validate`] fails.
    pub fn try_new(config: UserPnLConfig) -> anyhow::Result<Self> {
        config.validate()?;
        Ok(Self {
            core: StrategyCore::new(config.base.clone()),
            config,
            state: UserPnLState::Idle,
            runtime: None,
            session_date: None,
            day_anchor_pnl: None,
            trip_bound: None,
            trip_date: None,
            waiting_to_rearm: false,
            stopped_for_halt: Vec::new(),
            flatten_started: None,
            last_exit_request: None,
            flatten_timed_out: false,
            latched_halted: false,
            warned_missing_pnl: false,
        })
    }

    /// Creates a new [`UserPnL`] from a validated config.
    ///
    /// # Panics
    ///
    /// Panics if [`UserPnLConfig::validate`] fails. Prefer [`Self::try_new`] at
    /// registration boundaries.
    #[must_use]
    pub fn new(config: UserPnLConfig) -> Self {
        Self::try_new(config).expect("UserPnLConfig::validate failed")
    }

    /// Inject node hooks (`Trader.market_exit_strategy` + stop/start + trading state).
    #[must_use]
    pub fn with_runtime(mut self, runtime: Rc<dyn UserPnLRuntime>) -> Self {
        self.runtime = Some(runtime);
        self
    }

    /// Current kill-switch latch.
    #[must_use]
    pub const fn state(&self) -> UserPnLState {
        self.state
    }

    fn self_strategy_id(&self) -> StrategyId {
        StrategyId::from(self.actor_id().inner().as_str())
    }

    fn is_self(&self, strategy_id: StrategyId) -> bool {
        strategy_id == self.self_strategy_id()
            || self
                .config
                .base
                .strategy_id
                .is_some_and(|configured| configured == strategy_id)
    }

    fn account_pnl(&mut self) -> Option<Decimal> {
        let venue = &self.config.venue;
        let account_id = self.config.account_id.as_ref();
        let currency = self.config.currency;
        let pnls = if self.config.use_unrealized_only {
            self.portfolio()
                .unrealized_pnls_in(venue, account_id, currency)
        } else {
            self.portfolio().total_pnls_in(venue, account_id, currency)
        };

        let Some(map) = pnls else {
            if !self.warned_missing_pnl {
                log::warn!(
                    "UserPnL PnL unavailable for {} in {} (unpriced instruments or missing FX); kill switch idle until a value is available",
                    self.config.venue,
                    currency
                );
                self.warned_missing_pnl = true;
            }
            return None;
        };

        self.warned_missing_pnl = false;

        match map.get(&currency) {
            Some(money) => Some(money.as_decimal()),
            None if map.is_empty() => Some(Decimal::ZERO),
            None => {
                log::warn!(
                    "UserPnL PnL map for {} is missing {currency} ({} keys); kill switch idle",
                    self.config.venue,
                    map.len()
                );
                None
            }
        }
    }

    fn trading_date(&self) -> Date {
        Offset::UTC.to_datetime(self.clock().utc_now()).date()
    }

    /// Session total, or today's increment when `reset_daily` is set.
    ///
    /// Daily PnL is `total - day_anchor`. Live cache purges of closed positions
    /// can shift that difference; prefer a settlement `currency` that does not
    /// require FX, and treat this as session-cache PnL rather than a ledger.
    fn watched_pnl(&mut self) -> Option<Decimal> {
        let total = self.account_pnl()?;
        if !self.config.reset_daily {
            return Some(total);
        }

        Some(total - self.day_anchor_pnl.unwrap_or(Decimal::ZERO))
    }

    fn should_trip(&self, pnl: Decimal) -> Option<TripBound> {
        if let Some(max_loss) = self.config.max_loss
            && pnl <= max_loss
        {
            return Some(TripBound::MaxLoss);
        }

        if let Some(max_profit) = self.config.max_profit
            && pnl >= max_profit
        {
            return Some(TripBound::MaxProfit);
        }

        None
    }

    fn inside_band(&self, pnl: Decimal) -> bool {
        let above_loss = self.config.max_loss.is_none_or(|max_loss| pnl > max_loss);
        let below_profit = self
            .config
            .max_profit
            .is_none_or(|max_profit| pnl < max_profit);
        above_loss && below_profit
    }

    fn halt_day_for_bound(&self, bound: TripBound) -> bool {
        match bound {
            TripBound::MaxLoss => self.config.halt_day_on_max_loss,
            TripBound::MaxProfit => self.config.halt_day_on_max_profit,
        }
    }

    /// True when the flatten completed in a later UTC window than the trip.
    ///
    /// Only meaningful with `reset_daily`; without it there is no window to
    /// cross and the trip stays in force for the whole run.
    fn flatten_crossed_into_new_day(&self) -> bool {
        self.config.reset_daily && self.trip_date != Some(self.trading_date())
    }

    /// `HALTED` is only for the UTC day of the trip when `reset_daily` is set.
    /// A flatten that completes after midnight restores `ACTIVE`.
    fn should_halt_after_flatten(&self) -> bool {
        let halt_configured = self
            .trip_bound
            .is_none_or(|bound| self.halt_day_for_bound(bound));
        halt_configured && !self.flatten_crossed_into_new_day()
    }

    fn maybe_roll_day(&mut self) {
        if !self.config.reset_daily {
            return;
        }

        let date = self.trading_date();
        match self.session_date {
            None => self.session_date = Some(date),
            Some(prev) if prev != date => {
                self.session_date = Some(date);
                self.day_anchor_pnl = self.account_pnl();

                if self.state == UserPnLState::Flattening {
                    log::warn!(
                        "UserPnL new UTC day {date}; flattening continues until books are empty"
                    );
                } else if self.state != UserPnLState::Idle || !self.stopped_for_halt.is_empty() {
                    log::warn!("UserPnL new UTC day {date}; re-arming after previous session halt");
                    self.rearm_for_new_day();
                }
            }
            Some(_) => {}
        }
    }

    fn rearm_for_new_day(&mut self) {
        let restore_active = self.latched_halted;
        self.state = UserPnLState::Idle;
        self.trip_bound = None;
        self.trip_date = None;
        self.waiting_to_rearm = false;
        self.flatten_started = None;
        self.last_exit_request = None;
        self.flatten_timed_out = false;
        self.latched_halted = false;

        if let Some(runtime) = self.runtime.clone() {
            if restore_active && let Err(e) = runtime.set_trading_state(TradingState::Active) {
                log::error!("UserPnL failed to set ACTIVE on day roll: {e}");
            }

            for strategy_id in self.stopped_for_halt.drain(..) {
                if let Err(e) = runtime.start_strategy(strategy_id) {
                    log::error!("UserPnL start_strategy({strategy_id}) failed: {e}");
                }
            }
        } else {
            self.stopped_for_halt.clear();
        }
    }

    fn has_open_risk(&self, strategy_id: StrategyId) -> bool {
        let venue = Some(&self.config.venue);
        let account_id = self.config.account_id.as_ref();
        let cache = self.cache();
        let positions =
            cache.positions_open_count(venue, None, Some(&strategy_id), account_id, None);
        let open_orders =
            cache.orders_open_count(venue, None, Some(&strategy_id), account_id, None);
        let inflight =
            cache.orders_inflight_count(venue, None, Some(&strategy_id), account_id, None);
        positions + open_orders + inflight > 0
    }

    fn managed_risk_is_flat(&self) -> bool {
        if self.config.managed_strategy_ids.is_empty() {
            let venue = Some(&self.config.venue);
            let account_id = self.config.account_id.as_ref();
            let cache = self.cache();
            let positions = cache.positions_open_count(venue, None, None, account_id, None);
            let open_orders = cache.orders_open_count(venue, None, None, account_id, None);
            let inflight = cache.orders_inflight_count(venue, None, None, account_id, None);
            positions + open_orders + inflight == 0
        } else {
            self.candidate_strategy_ids()
                .iter()
                .all(|id| !self.has_open_risk(*id))
        }
    }

    fn candidate_strategy_ids(&self) -> Vec<StrategyId> {
        let mut ids: Vec<StrategyId> = if self.config.managed_strategy_ids.is_empty() {
            self.runtime.as_ref().map_or_else(
                || self.cache().strategy_ids().into_iter().collect(),
                |runtime| runtime.registered_strategy_ids(),
            )
        } else {
            self.config.managed_strategy_ids.clone()
        };

        ids.retain(|id| !self.is_self(*id));
        ids.sort();
        ids.dedup();
        ids
    }

    fn strategies_to_exit(&self) -> Vec<StrategyId> {
        self.candidate_strategy_ids()
            .into_iter()
            .filter(|id| !self.config.skip_flat_strategies || self.has_open_risk(*id))
            .collect()
    }

    fn request_exits(&mut self, initial: bool) {
        let Some(runtime) = self.runtime.clone() else {
            return;
        };

        for strategy_id in self.strategies_to_exit() {
            if initial {
                log::warn!("UserPnL requesting market_exit for {strategy_id}");
            } else {
                log::debug!("UserPnL re-issuing market_exit for {strategy_id}");
            }

            if let Err(e) = runtime.exit_strategy(strategy_id) {
                log::error!("UserPnL market_exit_strategy({strategy_id}) failed: {e}");
            }
        }

        let now = self.clock().timestamp_ns();
        self.last_exit_request = Some(now);
    }

    fn should_redrive_exits(&self) -> bool {
        let Some(last) = self.last_exit_request else {
            return true;
        };

        let elapsed_ms = self
            .clock()
            .timestamp_ns()
            .as_u64()
            .saturating_sub(last.as_u64())
            / 1_000_000;
        elapsed_ms >= self.config.flatten_redrive_ms
    }

    fn trip(&mut self, bound: TripBound) {
        self.state = UserPnLState::Flattening;
        self.trip_bound = Some(bound);
        self.trip_date = Some(self.trading_date());
        self.waiting_to_rearm = false;
        let now = self.clock().timestamp_ns();
        self.flatten_started = Some(now);
        self.flatten_timed_out = false;

        if let Some(runtime) = self.runtime.clone() {
            if let Err(e) = runtime.set_trading_state(TradingState::Reducing) {
                log::error!("UserPnL failed to set REDUCING: {e}");
            }

            self.request_exits(true);
        } else {
            log::error!(
                "UserPnL tripped but no runtime is attached; cannot flatten other strategies or latch trading state"
            );
        }
    }

    fn maybe_timeout_flatten(&mut self) {
        let Some(started) = self.flatten_started else {
            return;
        };

        let elapsed_ms = self
            .clock()
            .timestamp_ns()
            .as_u64()
            .saturating_sub(started.as_u64())
            / 1_000_000;

        let timeout_ms = self.config.flatten_timeout_ms;

        if elapsed_ms < timeout_ms || self.flatten_timed_out {
            return;
        }

        self.flatten_timed_out = true;
        log::error!(
            "UserPnL flatten still has residual risk after {timeout_ms}ms; keeping REDUCING and re-issuing market_exit (HALTED is deferred until books are empty)"
        );
    }

    fn stop_managed_strategies(&mut self) {
        let Some(runtime) = self.runtime.clone() else {
            return;
        };

        self.stopped_for_halt.clear();

        for strategy_id in self.candidate_strategy_ids() {
            log::warn!("UserPnL stopping {strategy_id}");
            match runtime.stop_strategy(strategy_id) {
                Ok(true) => self.stopped_for_halt.push(strategy_id),
                Ok(false) => log::warn!(
                    "UserPnL stop of {strategy_id} deferred (component still running); will not auto-start it on day roll"
                ),
                Err(e) => log::error!("UserPnL stop_strategy({strategy_id}) failed: {e}"),
            }
        }
    }

    fn latch_halted_after_flatten(&mut self) {
        self.stop_managed_strategies();
        self.state = UserPnLState::Halted;
        self.latched_halted = true;
        self.flatten_started = None;
        self.last_exit_request = None;
        self.flatten_timed_out = false;

        if let Some(runtime) = &self.runtime
            && let Err(e) = runtime.set_trading_state(TradingState::Halted)
        {
            log::error!("UserPnL failed to set HALTED: {e}");
        }

        log::warn!("UserPnL managed risk is flat; trading state HALTED");
    }

    fn release_active_after_flatten(&mut self) {
        // Suppress an immediate re-trip only within the trip's own window. A new
        // UTC day must stay armed, since closing the residual can realize past a
        // bound and that loss belongs to the new day.
        let crossed_into_new_day = self.flatten_crossed_into_new_day();
        self.state = UserPnLState::Idle;
        self.waiting_to_rearm = !crossed_into_new_day;
        self.flatten_started = None;
        self.last_exit_request = None;
        self.flatten_timed_out = false;

        if let Some(runtime) = &self.runtime
            && let Err(e) = runtime.set_trading_state(TradingState::Active)
        {
            log::error!("UserPnL failed to set ACTIVE after no-halt flatten: {e}");
        }

        if crossed_into_new_day {
            log::warn!(
                "UserPnL managed risk is flat; trip day ended, remaining ACTIVE for the new UTC day"
            );
        } else {
            log::warn!(
                "UserPnL managed risk is flat; remaining ACTIVE for the rest of the UTC day"
            );
        }
    }

    fn finish_if_managed_flat(&mut self) {
        if !self.managed_risk_is_flat() {
            return;
        }

        if self.should_halt_after_flatten() {
            self.latch_halted_after_flatten();
            return;
        }

        // Releasing the trip on a new UTC day must not publish `ACTIVE` on a window
        // that is already breached: closing the residual realizes into the new day,
        // so re-trip on this same check rather than leaving one interval of
        // unprotected `ACTIVE` in which other strategies could add size.
        if self.flatten_crossed_into_new_day() {
            let pnl = self.watched_pnl();
            if let Some(bound) = pnl.and_then(|pnl| self.should_trip(pnl)) {
                log::warn!(
                    "UserPnL flatten completed on a new UTC day already past {bound:?}; re-tripping without releasing ACTIVE"
                );
                self.trip(bound);

                // `trip_date` is now today and the books are already empty, so the
                // new window settles here and cannot re-enter this branch.
                if self.should_halt_after_flatten() {
                    self.latch_halted_after_flatten();
                } else {
                    self.release_active_after_flatten();
                }
                return;
            }
        }

        self.release_active_after_flatten();
    }

    fn on_flattening(&mut self) {
        if self.should_redrive_exits() {
            self.request_exits(false);
        }

        self.maybe_timeout_flatten();
        self.finish_if_managed_flat();
    }

    pub(super) fn on_check(&mut self) {
        self.maybe_roll_day();

        match self.state {
            UserPnLState::Idle => {
                let Some(pnl) = self.watched_pnl() else {
                    return;
                };

                if self.waiting_to_rearm {
                    if self.inside_band(pnl) {
                        self.waiting_to_rearm = false;
                    } else {
                        return;
                    }
                }

                if let Some(bound) = self.should_trip(pnl) {
                    log::warn!(
                        "UserPnL tripped: pnl={pnl} {} bound={bound:?} max_loss={:?} max_profit={:?}",
                        self.config.currency,
                        self.config.max_loss,
                        self.config.max_profit
                    );
                    self.trip(bound);
                    self.finish_if_managed_flat();
                }
            }
            UserPnLState::Flattening => self.on_flattening(),
            UserPnLState::Halted => {}
        }
    }
}

nautilus_strategy!(UserPnL);

impl Debug for UserPnL {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(UserPnL))
            .field("venue", &self.config.venue)
            .field("account_id", &self.config.account_id)
            .field("max_loss", &self.config.max_loss)
            .field("max_profit", &self.config.max_profit)
            .field("state", &self.state)
            .field("has_runtime", &self.runtime.is_some())
            .finish()
    }
}

impl DataActor for UserPnL {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let interval_ns = self
            .config
            .check_interval_ms
            .saturating_mul(1_000_000)
            .max(1_000_000);
        self.clock()
            .set_timer_ns(USER_PNL_TIMER, interval_ns, None, None, None, None, None)?;
        log::info!(
            "UserPnL watching {} max_loss={:?} max_profit={:?} {} every {}ms reset_daily={}",
            self.config.venue,
            self.config.max_loss,
            self.config.max_profit,
            self.config.currency,
            self.config.check_interval_ms,
            self.config.reset_daily
        );
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.clock().cancel_timer(USER_PNL_TIMER);
        self.state = UserPnLState::Idle;
        self.trip_bound = None;
        self.trip_date = None;
        self.waiting_to_rearm = false;
        self.stopped_for_halt.clear();
        self.flatten_started = None;
        self.last_exit_request = None;
        self.flatten_timed_out = false;
        self.latched_halted = false;
        Ok(())
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        if event.name.as_str() == USER_PNL_TIMER {
            self.on_check();
        }
        Ok(())
    }
}
