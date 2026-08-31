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

//! Python bindings for the example strategy and actor configs.

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::BarType,
    enums::TimeInForce,
    identifiers::{AccountId, ActorId, ClientId, InstrumentId, StrategyId, Venue},
    types::{Currency, Quantity},
};
use pyo3::prelude::*;
use rust_decimal::{Decimal, prelude::FromPrimitive};

use crate::examples::{
    actors::BookImbalanceActorConfig,
    strategies::{
        CompositeMarketMakerConfig, DeltaNeutralVolConfig, EmaCrossConfig, GridMarketMakerConfig,
        HurstVpinDirectionalConfig, UserPnLConfig,
    },
};

fn optional_decimal(value: Option<f64>, name: &str) -> PyResult<Option<Decimal>> {
    match value {
        None => Ok(None),
        Some(v) => Decimal::from_f64(v)
            .ok_or_else(|| to_pyvalue_err(format!("Invalid {name}: {v}")))
            .map(Some),
    }
}

macro_rules! impl_strategy_config_base_getters {
    ($type:ty) => {
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        #[pymethods]
        impl $type {
            #[getter]
            #[pyo3(name = "strategy_id")]
            fn py_strategy_id(&self) -> Option<StrategyId> {
                self.base.strategy_id
            }

            #[getter]
            #[pyo3(name = "order_id_tag")]
            fn py_order_id_tag(&self) -> Option<&str> {
                self.base.order_id_tag.as_deref()
            }
        }
    };
}

impl_strategy_config_base_getters!(CompositeMarketMakerConfig);
impl_strategy_config_base_getters!(GridMarketMakerConfig);
impl_strategy_config_base_getters!(EmaCrossConfig);
impl_strategy_config_base_getters!(DeltaNeutralVolConfig);
impl_strategy_config_base_getters!(HurstVpinDirectionalConfig);
impl_strategy_config_base_getters!(UserPnLConfig);

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CompositeMarketMakerConfig {
    /// Configuration for the composite market making strategy.
    #[new]
    #[pyo3(signature = (
        instrument_id,
        signal_instrument_id,
        max_position,
        strategy_id=None,
        order_id_tag=None,
        trade_size=None,
        half_spread_bps=5,
        inventory_skew_factor=0.0,
        signal_skew_factor=0.0,
        signal_baseline=None,
        requote_threshold_bps=5,
        expire_time_secs=None,
        on_cancel_resubmit=false,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        instrument_id: InstrumentId,
        signal_instrument_id: InstrumentId,
        max_position: Quantity,
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
        trade_size: Option<Quantity>,
        half_spread_bps: u32,
        inventory_skew_factor: f64,
        signal_skew_factor: f64,
        signal_baseline: Option<f64>,
        requote_threshold_bps: u32,
        expire_time_secs: Option<u64>,
        on_cancel_resubmit: bool,
    ) -> Self {
        let mut config = Self::builder()
            .instrument_id(instrument_id)
            .signal_instrument_id(signal_instrument_id)
            .max_position(max_position)
            .half_spread_bps(half_spread_bps)
            .inventory_skew_factor(inventory_skew_factor)
            .signal_skew_factor(signal_skew_factor)
            .requote_threshold_bps(requote_threshold_bps)
            .on_cancel_resubmit(on_cancel_resubmit)
            .maybe_trade_size(trade_size)
            .maybe_signal_baseline(signal_baseline)
            .maybe_expire_time_secs(expire_time_secs)
            .build();

        if let Some(id) = strategy_id {
            config.base.strategy_id = Some(id);
        }

        if let Some(tag) = order_id_tag {
            config.base.order_id_tag = Some(tag);
        }

        config
    }

    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    fn signal_instrument_id(&self) -> InstrumentId {
        self.signal_instrument_id
    }

    #[getter]
    fn max_position(&self) -> Quantity {
        self.max_position
    }

    #[getter]
    fn trade_size(&self) -> Option<Quantity> {
        self.trade_size
    }

    #[getter]
    fn half_spread_bps(&self) -> u32 {
        self.half_spread_bps
    }

    #[getter]
    fn inventory_skew_factor(&self) -> f64 {
        self.inventory_skew_factor
    }

    #[getter]
    fn signal_skew_factor(&self) -> f64 {
        self.signal_skew_factor
    }

    #[getter]
    fn signal_baseline(&self) -> Option<f64> {
        self.signal_baseline
    }

    #[getter]
    fn requote_threshold_bps(&self) -> u32 {
        self.requote_threshold_bps
    }

    #[getter]
    fn expire_time_secs(&self) -> Option<u64> {
        self.expire_time_secs
    }

    #[getter]
    fn on_cancel_resubmit(&self) -> bool {
        self.on_cancel_resubmit
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl GridMarketMakerConfig {
    /// Configuration for the grid market making strategy.
    #[new]
    #[pyo3(signature = (
        instrument_id,
        max_position,
        strategy_id=None,
        order_id_tag=None,
        trade_size=None,
        num_levels=3,
        grid_step_bps=10,
        skew_factor=0.0,
        requote_threshold_bps=5,
        expire_time_secs=None,
        on_cancel_resubmit=false,
        use_uuid_client_order_ids=false,
        use_hyphens_in_client_order_ids=true,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        instrument_id: InstrumentId,
        max_position: Quantity,
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
        trade_size: Option<Quantity>,
        num_levels: usize,
        grid_step_bps: u32,
        skew_factor: f64,
        requote_threshold_bps: u32,
        expire_time_secs: Option<u64>,
        on_cancel_resubmit: bool,
        use_uuid_client_order_ids: bool,
        use_hyphens_in_client_order_ids: bool,
    ) -> Self {
        let mut config = Self::builder()
            .instrument_id(instrument_id)
            .max_position(max_position)
            .num_levels(num_levels)
            .grid_step_bps(grid_step_bps)
            .skew_factor(skew_factor)
            .requote_threshold_bps(requote_threshold_bps)
            .on_cancel_resubmit(on_cancel_resubmit)
            .maybe_trade_size(trade_size)
            .maybe_expire_time_secs(expire_time_secs)
            .build();

        if let Some(id) = strategy_id {
            config.base.strategy_id = Some(id);
        }

        if let Some(tag) = order_id_tag {
            config.base.order_id_tag = Some(tag);
        }

        config.base.use_uuid_client_order_ids = use_uuid_client_order_ids;
        config.base.use_hyphens_in_client_order_ids = use_hyphens_in_client_order_ids;

        config
    }

    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    fn max_position(&self) -> Quantity {
        self.max_position
    }

    #[getter]
    fn trade_size(&self) -> Option<Quantity> {
        self.trade_size
    }

    #[getter]
    fn num_levels(&self) -> usize {
        self.num_levels
    }

    #[getter]
    fn grid_step_bps(&self) -> u32 {
        self.grid_step_bps
    }

    #[getter]
    fn skew_factor(&self) -> f64 {
        self.skew_factor
    }

    #[getter]
    fn requote_threshold_bps(&self) -> u32 {
        self.requote_threshold_bps
    }

    #[getter]
    fn expire_time_secs(&self) -> Option<u64> {
        self.expire_time_secs
    }

    #[getter]
    fn on_cancel_resubmit(&self) -> bool {
        self.on_cancel_resubmit
    }

    #[getter]
    fn use_uuid_client_order_ids(&self) -> bool {
        self.base.use_uuid_client_order_ids
    }

    #[getter]
    fn use_hyphens_in_client_order_ids(&self) -> bool {
        self.base.use_hyphens_in_client_order_ids
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl EmaCrossConfig {
    /// Configuration for the dual-EMA crossover strategy.
    #[new]
    #[pyo3(signature = (
        instrument_id,
        trade_size,
        fast_period=10,
        slow_period=50,
        strategy_id=None,
        order_id_tag=None,
    ))]
    fn py_new(
        instrument_id: InstrumentId,
        trade_size: Quantity,
        fast_period: usize,
        slow_period: usize,
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
    ) -> Self {
        let mut config = Self::builder()
            .instrument_id(instrument_id)
            .trade_size(trade_size)
            .fast_period(fast_period)
            .slow_period(slow_period)
            .build();

        if let Some(id) = strategy_id {
            config.base.strategy_id = Some(id);
        }

        if let Some(tag) = order_id_tag {
            config.base.order_id_tag = Some(tag);
        }

        config
    }

    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    fn trade_size(&self) -> Quantity {
        self.trade_size
    }

    #[getter]
    fn fast_period(&self) -> usize {
        self.fast_period
    }

    #[getter]
    fn slow_period(&self) -> usize {
        self.slow_period
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DeltaNeutralVolConfig {
    /// Configuration for the delta-neutral short volatility hedger.
    ///
    /// Tracks a short OTM call and put (strangle) and delta-hedges with the
    /// underlying perpetual swap. Rehedges when portfolio delta exceeds a
    /// configurable threshold or on a periodic timer.
    #[new]
    #[pyo3(signature = (
        option_family,
        hedge_instrument_id,
        client_id,
        strategy_id=None,
        order_id_tag=None,
        target_call_delta=0.20,
        target_put_delta=-0.20,
        contracts=1,
        rehedge_delta_threshold=0.5,
        rehedge_interval_secs=30,
        expiry_filter=None,
        enter_strangle=true,
        entry_iv_offset=0.0,
        entry_time_in_force=TimeInForce::Gtc,
        entry_premium_offset_ticks=None,
        iv_param_key="px_vol",
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        option_family: String,
        hedge_instrument_id: InstrumentId,
        client_id: ClientId,
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
        target_call_delta: f64,
        target_put_delta: f64,
        contracts: u64,
        rehedge_delta_threshold: f64,
        rehedge_interval_secs: u64,
        expiry_filter: Option<String>,
        enter_strangle: bool,
        entry_iv_offset: f64,
        entry_time_in_force: TimeInForce,
        entry_premium_offset_ticks: Option<i32>,
        iv_param_key: &str,
    ) -> Self {
        let mut config = Self::builder()
            .option_family(option_family)
            .hedge_instrument_id(hedge_instrument_id)
            .client_id(client_id)
            .target_call_delta(target_call_delta)
            .target_put_delta(target_put_delta)
            .contracts(contracts)
            .rehedge_delta_threshold(rehedge_delta_threshold)
            .rehedge_interval_secs(rehedge_interval_secs)
            .enter_strangle(enter_strangle)
            .entry_iv_offset(entry_iv_offset)
            .entry_time_in_force(entry_time_in_force)
            .iv_param_key(iv_param_key.to_string())
            .maybe_expiry_filter(expiry_filter)
            .maybe_entry_premium_offset_ticks(entry_premium_offset_ticks)
            .build();

        if let Some(id) = strategy_id {
            config.base.strategy_id = Some(id);
        }

        if let Some(tag) = order_id_tag {
            config.base.order_id_tag = Some(tag);
        }

        config
    }

    #[getter]
    fn option_family(&self) -> &str {
        &self.option_family
    }

    #[getter]
    fn hedge_instrument_id(&self) -> InstrumentId {
        self.hedge_instrument_id
    }

    #[getter]
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    #[getter]
    fn target_call_delta(&self) -> f64 {
        self.target_call_delta
    }

    #[getter]
    fn target_put_delta(&self) -> f64 {
        self.target_put_delta
    }

    #[getter]
    fn contracts(&self) -> u64 {
        self.contracts
    }

    #[getter]
    fn rehedge_delta_threshold(&self) -> f64 {
        self.rehedge_delta_threshold
    }

    #[getter]
    fn rehedge_interval_secs(&self) -> u64 {
        self.rehedge_interval_secs
    }

    #[getter]
    fn expiry_filter(&self) -> Option<&str> {
        self.expiry_filter.as_deref()
    }

    #[getter]
    fn enter_strangle(&self) -> bool {
        self.enter_strangle
    }

    #[getter]
    fn entry_iv_offset(&self) -> f64 {
        self.entry_iv_offset
    }

    #[getter]
    fn entry_time_in_force(&self) -> TimeInForce {
        self.entry_time_in_force
    }

    #[getter]
    fn entry_premium_offset_ticks(&self) -> Option<i32> {
        self.entry_premium_offset_ticks
    }

    #[getter]
    #[pyo3(name = "iv_param_key")]
    fn py_iv_param_key(&self) -> &str {
        &self.iv_param_key
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl HurstVpinDirectionalConfig {
    /// Configuration for the Hurst/VPIN directional strategy.
    ///
    /// Combines a rescaled-range Hurst regime filter on dollar bars with a
    /// VPIN-derived informed-flow signal, and gates entry timing on the
    /// live quote stream.
    #[new]
    #[pyo3(signature = (
        instrument_id,
        bar_type,
        trade_size,
        strategy_id=None,
        order_id_tag=None,
        hurst_window=128,
        hurst_lags=None,
        hurst_enter=0.55,
        hurst_exit=0.50,
        vpin_window=50,
        vpin_threshold=0.30,
        max_holding_secs=3600,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        instrument_id: InstrumentId,
        bar_type: BarType,
        trade_size: Quantity,
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
        hurst_window: usize,
        hurst_lags: Option<Vec<usize>>,
        hurst_enter: f64,
        hurst_exit: f64,
        vpin_window: usize,
        vpin_threshold: f64,
        max_holding_secs: u64,
    ) -> Self {
        let mut config = Self::builder()
            .instrument_id(instrument_id)
            .bar_type(bar_type)
            .trade_size(trade_size)
            .hurst_window(hurst_window)
            .maybe_hurst_lags(hurst_lags)
            .hurst_enter(hurst_enter)
            .hurst_exit(hurst_exit)
            .vpin_window(vpin_window)
            .vpin_threshold(vpin_threshold)
            .max_holding_secs(max_holding_secs)
            .build();

        if let Some(id) = strategy_id {
            config.base.strategy_id = Some(id);
        }

        if let Some(tag) = order_id_tag {
            config.base.order_id_tag = Some(tag);
        }

        config
    }

    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    fn bar_type(&self) -> BarType {
        self.bar_type
    }

    #[getter]
    fn trade_size(&self) -> Quantity {
        self.trade_size
    }

    #[getter]
    fn hurst_window(&self) -> usize {
        self.hurst_window
    }

    #[getter]
    fn hurst_lags(&self) -> Vec<usize> {
        self.hurst_lags.clone()
    }

    #[getter]
    fn hurst_enter(&self) -> f64 {
        self.hurst_enter
    }

    #[getter]
    fn hurst_exit(&self) -> f64 {
        self.hurst_exit
    }

    #[getter]
    fn vpin_window(&self) -> usize {
        self.vpin_window
    }

    #[getter]
    fn vpin_threshold(&self) -> f64 {
        self.vpin_threshold
    }

    #[getter]
    fn max_holding_secs(&self) -> u64 {
        self.max_holding_secs
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl UserPnLConfig {
    /// Configuration for the account-level user PnL sidecar.
    ///
    /// Watches `Portfolio` PnL for one venue/account
    /// and, on `max_loss` or `max_profit`, asks every other registered strategy to
    /// `market_exit()` until the account has no positions and no live orders, then
    /// optionally latches `HALTED` and stops those strategies for the rest of the UTC day.
    #[new]
    #[pyo3(signature = (
        venue,
        currency,
        account_id=None,
        max_loss=None,
        max_profit=None,
        halt_day_on_max_loss=true,
        halt_day_on_max_profit=true,
        use_unrealized_only=false,
        check_interval_ms=200,
        flatten_redrive_ms=5_000,
        flatten_timeout_ms=30_000,
        managed_strategy_ids=None,
        skip_flat_strategies=true,
        reset_daily=true,
        strategy_id=None,
        order_id_tag=None,
    ))]
    #[expect(
        clippy::fn_params_excessive_bools,
        clippy::too_many_arguments,
        reason = "constructor mirrors the Python keyword API"
    )]
    fn py_new(
        venue: Venue,
        currency: Currency,
        account_id: Option<AccountId>,
        max_loss: Option<f64>,
        max_profit: Option<f64>,
        halt_day_on_max_loss: bool,
        halt_day_on_max_profit: bool,
        use_unrealized_only: bool,
        check_interval_ms: u64,
        flatten_redrive_ms: u64,
        flatten_timeout_ms: u64,
        managed_strategy_ids: Option<Vec<StrategyId>>,
        skip_flat_strategies: bool,
        reset_daily: bool,
        strategy_id: Option<StrategyId>,
        order_id_tag: Option<String>,
    ) -> PyResult<Self> {
        let mut config = Self::builder()
            .venue(venue)
            .currency(currency)
            .maybe_account_id(account_id)
            .maybe_max_loss(optional_decimal(max_loss, "max_loss")?)
            .maybe_max_profit(optional_decimal(max_profit, "max_profit")?)
            .halt_day_on_max_loss(halt_day_on_max_loss)
            .halt_day_on_max_profit(halt_day_on_max_profit)
            .use_unrealized_only(use_unrealized_only)
            .check_interval_ms(check_interval_ms)
            .flatten_redrive_ms(flatten_redrive_ms)
            .flatten_timeout_ms(flatten_timeout_ms)
            .managed_strategy_ids(managed_strategy_ids.unwrap_or_default())
            .skip_flat_strategies(skip_flat_strategies)
            .reset_daily(reset_daily)
            .build();

        if let Some(id) = strategy_id {
            config.base.strategy_id = Some(id);
        }

        if let Some(tag) = order_id_tag {
            config.base.order_id_tag = Some(tag);
        }

        config.validate().map_err(to_pyvalue_err)?;
        Ok(config)
    }

    #[getter]
    fn venue(&self) -> Venue {
        self.venue
    }

    #[getter]
    fn account_id(&self) -> Option<AccountId> {
        self.account_id
    }

    #[getter]
    fn currency(&self) -> Currency {
        self.currency
    }

    #[getter]
    fn max_loss(&self) -> Option<f64> {
        self.max_loss
            .and_then(|value| rust_decimal::prelude::ToPrimitive::to_f64(&value))
    }

    #[getter]
    fn max_profit(&self) -> Option<f64> {
        self.max_profit
            .and_then(|value| rust_decimal::prelude::ToPrimitive::to_f64(&value))
    }

    #[getter]
    fn halt_day_on_max_loss(&self) -> bool {
        self.halt_day_on_max_loss
    }

    #[getter]
    fn halt_day_on_max_profit(&self) -> bool {
        self.halt_day_on_max_profit
    }

    #[getter]
    fn use_unrealized_only(&self) -> bool {
        self.use_unrealized_only
    }

    #[getter]
    fn check_interval_ms(&self) -> u64 {
        self.check_interval_ms
    }

    #[getter]
    fn flatten_redrive_ms(&self) -> u64 {
        self.flatten_redrive_ms
    }

    #[getter]
    fn flatten_timeout_ms(&self) -> u64 {
        self.flatten_timeout_ms
    }

    #[getter]
    fn managed_strategy_ids(&self) -> Vec<StrategyId> {
        self.managed_strategy_ids.clone()
    }

    #[getter]
    fn skip_flat_strategies(&self) -> bool {
        self.skip_flat_strategies
    }

    #[getter]
    fn reset_daily(&self) -> bool {
        self.reset_daily
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BookImbalanceActorConfig {
    /// Configuration for the order book imbalance actor.
    #[new]
    #[pyo3(signature = (instrument_ids, log_interval=100, actor_id=None))]
    fn py_new(
        instrument_ids: Vec<InstrumentId>,
        log_interval: u64,
        actor_id: Option<ActorId>,
    ) -> Self {
        Self::builder()
            .instrument_ids(instrument_ids)
            .log_interval(log_interval)
            .maybe_actor_id(actor_id)
            .build()
    }

    #[getter]
    fn instrument_ids(&self) -> Vec<InstrumentId> {
        self.instrument_ids.clone()
    }

    #[getter]
    fn log_interval(&self) -> u64 {
        self.log_interval
    }

    #[getter]
    fn actor_id(&self) -> Option<ActorId> {
        self.actor_id
    }
}
