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

use std::{cell::RefCell, rc::Rc};

use jiff::civil::Date;
use nautilus_common::{
    actor::DataActor,
    cache::Cache,
    clock::{Clock, TestClock},
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::{AccountAny, CashAccount},
    data::QuoteTick,
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderType, TradingState},
    events::{AccountState, OrderFilled},
    identifiers::{
        AccountId, ClientOrderId, PositionId, StrategyId, TradeId, TraderId, Venue, VenueOrderId,
    },
    instruments::{InstrumentAny, stubs::currency_pair_btcusdt},
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use nautilus_portfolio::portfolio::Portfolio;
use rstest::rstest;
use rust_decimal::Decimal;

use super::{UserPnL, UserPnLConfig, UserPnLRuntime, UserPnLState};

struct FakeRuntime {
    registered: Vec<StrategyId>,
    exits: Rc<RefCell<Vec<StrategyId>>>,
    stops: Rc<RefCell<Vec<StrategyId>>>,
    starts: Rc<RefCell<Vec<StrategyId>>>,
    states: Rc<RefCell<Vec<TradingState>>>,
}

impl UserPnLRuntime for FakeRuntime {
    fn registered_strategy_ids(&self) -> Vec<StrategyId> {
        self.registered.clone()
    }

    fn exit_strategy(&self, strategy_id: StrategyId) -> anyhow::Result<()> {
        self.exits.borrow_mut().push(strategy_id);
        Ok(())
    }

    fn stop_strategy(&self, strategy_id: StrategyId) -> anyhow::Result<bool> {
        self.stops.borrow_mut().push(strategy_id);
        Ok(true)
    }

    fn start_strategy(&self, strategy_id: StrategyId) -> anyhow::Result<()> {
        self.starts.borrow_mut().push(strategy_id);
        Ok(())
    }

    fn set_trading_state(&self, state: TradingState) -> anyhow::Result<()> {
        self.states.borrow_mut().push(state);
        Ok(())
    }
}

struct Harness {
    strategy: UserPnL,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    portfolio: Rc<RefCell<Portfolio>>,
    exits: Rc<RefCell<Vec<StrategyId>>>,
    stops: Rc<RefCell<Vec<StrategyId>>>,
    starts: Rc<RefCell<Vec<StrategyId>>>,
    states: Rc<RefCell<Vec<TradingState>>>,
    next_trade: u32,
}

fn config() -> UserPnLConfig {
    UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_loss(Decimal::from(-10_000))
        .build()
}

fn mm() -> StrategyId {
    StrategyId::from("MM-001")
}

fn grid() -> StrategyId {
    StrategyId::from("GRID-001")
}

fn user_pnl_id() -> StrategyId {
    StrategyId::from("USER_PNL-001")
}

fn binance_account() -> AccountAny {
    let event = AccountState::new(
        AccountId::from("BINANCE-001"),
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("1000000 USDT"),
            Money::from("0 USDT"),
            Money::from("1000000 USDT"),
        )],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        Some(Currency::USDT()),
    );
    AccountAny::Cash(CashAccount::new(event, false, false))
}

fn make_strategy(registered: Vec<StrategyId>) -> Harness {
    make_strategy_with_config(registered, config())
}

fn make_strategy_with_config(registered: Vec<StrategyId>, config: UserPnLConfig) -> Harness {
    let exits = Rc::new(RefCell::new(Vec::new()));
    let stops = Rc::new(RefCell::new(Vec::new()));
    let starts = Rc::new(RefCell::new(Vec::new()));
    let states = Rc::new(RefCell::new(Vec::new()));
    let runtime = Rc::new(FakeRuntime {
        registered,
        exits: exits.clone(),
        stops: stops.clone(),
        starts: starts.clone(),
        states: states.clone(),
    });
    let mut strategy = UserPnL::try_new(config).unwrap().with_runtime(runtime);

    let clock = Rc::new(RefCell::new(TestClock::new()));
    clock
        .borrow_mut()
        .register_default_handler(TimeEventCallback::from(|_: TimeEvent| {}));
    let clock_dyn: Rc<RefCell<dyn Clock>> = clock.clone();
    let cache = Rc::new(RefCell::new(Cache::default()));
    cache.borrow_mut().add_account(binance_account()).unwrap();
    let portfolio = Rc::new(RefCell::new(Portfolio::new(
        clock_dyn.clone(),
        cache.clone(),
        None,
    )));
    strategy
        .core
        .register(
            TraderId::from("TESTER-001"),
            clock_dyn,
            cache.clone(),
            portfolio.clone(),
        )
        .unwrap();

    Harness {
        strategy,
        clock,
        cache,
        portfolio,
        exits,
        stops,
        starts,
        states,
        next_trade: 1,
    }
}

fn fill(
    strategy_id: StrategyId,
    side: OrderSide,
    trade_n: u32,
    qty: &str,
    px: &str,
) -> OrderFilled {
    let instrument = currency_pair_btcusdt();
    OrderFilled::new(
        TraderId::from("TESTER-001"),
        strategy_id,
        instrument.id,
        ClientOrderId::from(format!("O-{trade_n}").as_str()),
        VenueOrderId::from(format!("V-{trade_n}").as_str()),
        AccountId::from("BINANCE-001"),
        TradeId::from(format!("T-{trade_n}").as_str()),
        side,
        OrderType::Market,
        Quantity::from(qty),
        Price::from(px),
        instrument.quote_currency,
        LiquiditySide::Taker,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(PositionId::from(format!("P-{strategy_id}").as_str())),
        None,
        None,
    )
}

impl Harness {
    fn set_date(&self, year: i16, month: i8, day: i8) {
        let date = Date::new(year, month, day).unwrap();
        let ts = date
            .at(12, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .expect("UTC")
            .timestamp()
            .as_nanosecond();
        self.clock
            .borrow_mut()
            .set_time(UnixNanos::from(u64::try_from(ts).expect("ts")));
    }

    fn seed_instrument_and_quote(&self) {
        let instrument = currency_pair_btcusdt();
        let instrument_id = instrument.id;
        let mut cache = self.cache.borrow_mut();
        cache
            .add_instrument(InstrumentAny::CurrencyPair(instrument))
            .unwrap();
        let quote = QuoteTick::new(
            instrument_id,
            Price::from("50000.00"),
            Price::from("50000.00"),
            Quantity::from("1"),
            Quantity::from("1"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        cache.add_quote(quote).unwrap();
        drop(cache);
        self.portfolio.borrow_mut().update_quote_tick(&quote);
    }

    fn add_realized_pnl(&mut self, strategy_id: StrategyId, pnl: Decimal, keep_open: bool) {
        self.seed_instrument_and_quote();
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let n = self.next_trade;
        self.next_trade += 1;
        let open = fill(strategy_id, OrderSide::Buy, n, "1.000000", "50000.00");
        let mut position = Position::new(&instrument, open);

        position.realized_pnl = Some(Money::from_decimal(pnl, Currency::USDT()).unwrap());
        self.cache
            .borrow_mut()
            .add_position(&position, OmsType::Netting)
            .unwrap();

        if !keep_open {
            let close = fill(
                strategy_id,
                OrderSide::Sell,
                n + 1_000,
                "1.000000",
                "50000.00",
            );
            position.apply(&close);
            position.realized_pnl = Some(Money::from_decimal(pnl, Currency::USDT()).unwrap());
            self.cache.borrow_mut().update_position(&position).unwrap();
            self.next_trade += 1;
        }
    }

    fn close_open_risk(&mut self, strategy_id: StrategyId) {
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let n = self.next_trade;
        self.next_trade += 1;
        let open = fill(strategy_id, OrderSide::Buy, 1, "1.000000", "50000.00");
        let mut position = Position::new(&instrument, open);
        let close = fill(strategy_id, OrderSide::Sell, n, "1.000000", "50000.00");
        position.apply(&close);
        position.realized_pnl = Some(Money::from("-10000 USDT"));
        self.cache.borrow_mut().update_position(&position).unwrap();
    }

    fn update_realized_pnl(&self, strategy_id: StrategyId, pnl: Decimal) {
        let id = PositionId::from(format!("P-{strategy_id}").as_str());
        let mut position = {
            let cache = self.cache.borrow();
            cache
                .position_ref(&id)
                .expect("position must exist")
                .cloned()
        };
        position.realized_pnl = Some(Money::from_decimal(pnl, Currency::USDT()).unwrap());
        self.cache.borrow_mut().update_position(&position).unwrap();
    }

    fn advance_ms(&self, ms: u64) {
        let now = self.clock.borrow().timestamp_ns();
        self.clock.borrow_mut().set_time(UnixNanos::from(
            now.as_u64().saturating_add(ms.saturating_mul(1_000_000)),
        ));
    }
}

fn sorted(ids: &[StrategyId]) -> Vec<StrategyId> {
    let mut ids = ids.to_vec();
    ids.sort();
    ids
}

#[rstest]
fn test_validate_requires_at_least_one_bound() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .build();
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("at least one"));
    assert!(UserPnL::try_new(config).is_err());
}

#[rstest]
fn test_validate_rejects_positive_max_loss() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_loss(Decimal::from(10_000))
        .build();
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("max_loss"));
}

#[rstest]
fn test_validate_rejects_negative_max_profit() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_profit(Decimal::from(-10_000))
        .build();
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("max_profit"));
}

#[rstest]
fn test_validate_rejects_unrealized_with_reset_daily() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_loss(Decimal::from(-10_000))
        .use_unrealized_only(true)
        .reset_daily(true)
        .build();
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("use_unrealized_only"));
}

#[rstest]
fn test_validate_rejects_zero_check_interval() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_loss(Decimal::from(-10_000))
        .check_interval_ms(0)
        .build();
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("check_interval_ms"));
}

#[rstest]
fn test_validate_rejects_redrive_below_check_interval() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_loss(Decimal::from(-10_000))
        .check_interval_ms(1_000)
        .flatten_redrive_ms(500)
        .build();
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("flatten_redrive_ms"));
}

#[rstest]
fn test_validate_rejects_timeout_below_redrive() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_loss(Decimal::from(-10_000))
        .flatten_redrive_ms(10_000)
        .flatten_timeout_ms(5_000)
        .build();
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("flatten_timeout_ms"));
}

#[rstest]
fn test_custom_flatten_intervals_drive_redrive_and_timeout() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_loss(Decimal::from(-10_000))
        .flatten_redrive_ms(1_000)
        .flatten_timeout_ms(2_000)
        .build();
    let mut h = make_strategy_with_config(vec![mm()], config);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(-20_000), true);
    h.strategy.on_check();
    assert_eq!(h.exits.borrow().len(), 1);

    // Below the configured re-drive interval: no second exit.
    h.advance_ms(999);
    h.strategy.on_check();
    assert_eq!(h.exits.borrow().len(), 1);

    h.advance_ms(1);
    h.strategy.on_check();
    assert_eq!(h.exits.borrow().len(), 2);

    // Past the configured timeout, still REDUCING with residual risk.
    h.advance_ms(1_000);
    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Flattening);
    assert_eq!(*h.states.borrow(), vec![TradingState::Reducing]);
    assert!(h.stops.borrow().is_empty());
}

#[rstest]
fn test_idle_does_not_trip_above_max_loss() {
    let mut h = make_strategy(vec![mm(), grid(), user_pnl_id()]);
    h.add_realized_pnl(mm(), Decimal::from(-9_999), false);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Idle);
    assert!(h.exits.borrow().is_empty());
    assert!(h.stops.borrow().is_empty());
    assert!(h.states.borrow().is_empty());
}

#[rstest]
fn test_idle_does_not_trip_below_max_profit() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_profit(Decimal::from(10_000))
        .reset_daily(false)
        .build();
    let mut h = make_strategy_with_config(vec![mm()], config);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(9_999), true);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Idle);
    assert!(h.exits.borrow().is_empty());
}

#[rstest]
fn test_trips_at_max_loss_sets_reducing_exits_stops_and_halted() {
    let mut h = make_strategy(vec![mm(), grid(), user_pnl_id()]);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(-10_000), false);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Halted);
    assert_eq!(
        *h.states.borrow(),
        vec![TradingState::Reducing, TradingState::Halted]
    );
    assert_eq!(sorted(&h.exits.borrow()), vec![grid(), mm()]);
    assert_eq!(sorted(&h.stops.borrow()), vec![grid(), mm()]);
}

#[rstest]
fn test_trips_at_max_profit() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_profit(Decimal::from(10_000))
        .reset_daily(false)
        .build();
    let mut h = make_strategy_with_config(vec![mm(), user_pnl_id()], config);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(10_000), false);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Halted);
    assert_eq!(*h.exits.borrow(), vec![mm()]);
    assert_eq!(*h.stops.borrow(), vec![mm()]);
}

#[rstest]
fn test_max_loss_wins_when_both_bounds_are_zero() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_loss(Decimal::ZERO)
        .max_profit(Decimal::ZERO)
        .halt_day_on_max_loss(true)
        .halt_day_on_max_profit(false)
        .reset_daily(false)
        .build();
    let mut h = make_strategy_with_config(vec![mm()], config);
    h.strategy.config.skip_flat_strategies = false;
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Halted);
    assert_eq!(
        *h.states.borrow(),
        vec![TradingState::Reducing, TradingState::Halted]
    );
    assert_eq!(*h.stops.borrow(), vec![mm()]);
}

#[rstest]
fn test_does_not_exit_or_stop_itself() {
    let mut h = make_strategy(vec![mm(), user_pnl_id()]);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(-20_000), false);
    h.strategy.on_check();

    assert_eq!(*h.exits.borrow(), vec![mm()]);
    assert_eq!(*h.stops.borrow(), vec![mm()]);
}

#[rstest]
fn test_skip_flat_strategies_emits_no_exits_when_books_empty() {
    let mut h = make_strategy(vec![mm(), grid()]);
    h.add_realized_pnl(mm(), Decimal::from(-20_000), false);
    h.strategy.on_check();

    assert!(h.exits.borrow().is_empty());
    assert_eq!(h.strategy.state(), UserPnLState::Halted);
    assert_eq!(sorted(&h.stops.borrow()), vec![grid(), mm()]);
}

#[rstest]
fn test_trips_once() {
    let mut h = make_strategy(vec![mm()]);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(-20_000), false);
    h.strategy.on_check();
    h.strategy.on_check();
    h.strategy.on_check();

    assert_eq!(h.exits.borrow().len(), 1);
    assert_eq!(h.stops.borrow().len(), 1);
    assert_eq!(
        *h.states.borrow(),
        vec![TradingState::Reducing, TradingState::Halted]
    );
}

#[rstest]
fn test_stays_flattening_until_account_empty() {
    let mut h = make_strategy(vec![mm()]);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(-20_000), true);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Flattening);
    assert_eq!(*h.states.borrow(), vec![TradingState::Reducing]);
    assert_eq!(h.exits.borrow().len(), 1);
    assert!(h.stops.borrow().is_empty());

    h.close_open_risk(mm());
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Halted);
    assert_eq!(
        *h.states.borrow(),
        vec![TradingState::Reducing, TradingState::Halted]
    );
    assert_eq!(*h.stops.borrow(), vec![mm()]);
}

#[rstest]
fn test_flatten_redrives_exit_while_not_flat() {
    let mut h = make_strategy(vec![mm()]);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(-20_000), true);
    h.strategy.on_check();
    h.strategy.on_check();
    assert_eq!(h.exits.borrow().len(), 1);

    h.advance_ms(h.strategy.config.flatten_redrive_ms);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Flattening);
    assert_eq!(h.exits.borrow().len(), 2);
}

#[rstest]
fn test_halt_day_false_returns_active_without_stop() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_profit(Decimal::from(10_000))
        .halt_day_on_max_profit(false)
        .reset_daily(false)
        .build();
    let mut h = make_strategy_with_config(vec![mm()], config);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(10_000), false);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Idle);
    assert_eq!(
        *h.states.borrow(),
        vec![TradingState::Reducing, TradingState::Active]
    );
    assert_eq!(*h.exits.borrow(), vec![mm()]);
    assert!(h.stops.borrow().is_empty());
}

#[rstest]
fn test_no_halt_does_not_retrip_until_pnl_recovers() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_loss(Decimal::from(-10_000))
        .halt_day_on_max_loss(false)
        .reset_daily(false)
        .build();
    let mut h = make_strategy_with_config(vec![mm()], config);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(-10_000), false);
    h.strategy.on_check();
    assert_eq!(h.exits.borrow().len(), 1);

    h.strategy.on_check();
    assert_eq!(h.exits.borrow().len(), 1);

    h.update_realized_pnl(mm(), Decimal::from(-5_000));
    h.strategy.on_check();
    assert_eq!(h.exits.borrow().len(), 1);

    h.update_realized_pnl(mm(), Decimal::from(-10_000));
    h.strategy.on_check();
    assert_eq!(h.exits.borrow().len(), 2);
}

#[rstest]
fn test_daily_reset_rearms_starts_and_trips_on_next_day_loss() {
    let mut h = make_strategy(vec![mm()]);
    h.strategy.config.skip_flat_strategies = false;
    h.set_date(2024, 1, 16);
    h.add_realized_pnl(mm(), Decimal::from(-10_000), false);
    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Halted);
    assert_eq!(h.exits.borrow().len(), 1);
    assert_eq!(*h.stops.borrow(), vec![mm()]);

    h.set_date(2024, 1, 17);
    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Idle);
    assert_eq!(*h.starts.borrow(), vec![mm()]);
    assert_eq!(
        *h.states.borrow(),
        vec![
            TradingState::Reducing,
            TradingState::Halted,
            TradingState::Active
        ]
    );

    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Idle);
    assert_eq!(h.exits.borrow().len(), 1);

    h.update_realized_pnl(mm(), Decimal::from(-20_000));
    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Halted);
    assert_eq!(h.exits.borrow().len(), 2);
}

#[rstest]
fn test_reset_daily_false_stays_halted_across_days() {
    let mut h = make_strategy(vec![mm()]);
    h.strategy.config.skip_flat_strategies = false;
    h.strategy.config.reset_daily = false;
    h.set_date(2024, 1, 16);
    h.add_realized_pnl(mm(), Decimal::from(-10_000), false);
    h.strategy.on_check();
    h.set_date(2024, 1, 17);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Halted);
    assert!(h.starts.borrow().is_empty());
    assert_eq!(
        *h.states.borrow(),
        vec![TradingState::Reducing, TradingState::Halted]
    );
}

#[rstest]
fn test_managed_allowlist_is_honored() {
    let mut h = make_strategy(vec![mm(), grid()]);
    h.strategy.config.skip_flat_strategies = false;
    h.strategy.config.managed_strategy_ids = vec![grid()];
    h.add_realized_pnl(grid(), Decimal::from(-20_000), true);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Flattening);
    assert_eq!(*h.exits.borrow(), vec![grid()]);
    assert!(h.stops.borrow().is_empty());
}

#[rstest]
fn test_allowlist_flat_check_ignores_unmanaged_open_risk() {
    let mut h = make_strategy(vec![mm(), grid()]);
    h.strategy.config.skip_flat_strategies = false;
    h.strategy.config.managed_strategy_ids = vec![grid()];
    h.add_realized_pnl(mm(), Decimal::from(-20_000), true);
    h.add_realized_pnl(grid(), Decimal::from(-20_000), false);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Halted);
    assert_eq!(*h.exits.borrow(), vec![grid()]);
    assert_eq!(*h.stops.borrow(), vec![grid()]);
}

#[rstest]
fn test_flatten_timeout_keeps_reducing_without_halted() {
    let mut h = make_strategy(vec![mm()]);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(-20_000), true);
    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Flattening);

    h.advance_ms(h.strategy.config.flatten_timeout_ms);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Flattening);
    assert_eq!(*h.states.borrow(), vec![TradingState::Reducing]);
    assert!(h.stops.borrow().is_empty());
}

#[rstest]
fn test_day_roll_while_flattening_continues_until_flat() {
    let mut h = make_strategy(vec![mm()]);
    h.strategy.config.skip_flat_strategies = false;
    h.set_date(2024, 1, 16);
    h.add_realized_pnl(mm(), Decimal::from(-20_000), true);
    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Flattening);
    assert_eq!(*h.states.borrow(), vec![TradingState::Reducing]);
    assert_eq!(h.exits.borrow().len(), 1);

    h.set_date(2024, 1, 17);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Flattening);
    assert_eq!(*h.states.borrow(), vec![TradingState::Reducing]);
    assert!(h.starts.borrow().is_empty());
    assert!(h.stops.borrow().is_empty());
    assert_eq!(h.exits.borrow().len(), 2);

    h.close_open_risk(mm());
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Idle);
    assert_eq!(
        *h.states.borrow(),
        vec![TradingState::Reducing, TradingState::Active]
    );
    assert!(h.stops.borrow().is_empty());

    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Idle);
    assert_eq!(h.exits.borrow().len(), 2);
}

#[rstest]
fn test_new_day_breach_halts_without_publishing_active() {
    let mut h = make_strategy(vec![mm()]);
    h.strategy.config.skip_flat_strategies = false;
    h.set_date(2024, 1, 16);
    h.add_realized_pnl(mm(), Decimal::from(-20_000), true);
    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Flattening);

    // Anchor for the new day is captured at -20,000 while still flattening.
    h.set_date(2024, 1, 17);
    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Flattening);

    // Closing the residual realizes -15,000 against the new day's anchor, so the
    // new window is already breached the moment the trip is released.
    h.close_open_risk(mm());
    h.update_realized_pnl(mm(), Decimal::from(-35_000));
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Halted);
    assert_eq!(*h.stops.borrow(), vec![mm()]);
    assert_eq!(
        *h.states.borrow(),
        vec![
            TradingState::Reducing,
            TradingState::Reducing,
            TradingState::Halted
        ]
    );
    assert!(!h.states.borrow().contains(&TradingState::Active));
}

#[rstest]
fn test_run_long_no_halt_does_not_retrip_after_flatten_spans_midnight() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USDT())
        .max_loss(Decimal::from(-10_000))
        .halt_day_on_max_loss(false)
        .reset_daily(false)
        .build();
    let mut h = make_strategy_with_config(vec![mm()], config);
    h.strategy.config.skip_flat_strategies = false;
    h.set_date(2024, 1, 16);
    h.add_realized_pnl(mm(), Decimal::from(-20_000), true);
    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Flattening);

    h.set_date(2024, 1, 17);
    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Flattening);

    h.close_open_risk(mm());
    h.update_realized_pnl(mm(), Decimal::from(-20_000));
    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Idle);
    let exits_after_flatten = h.exits.borrow().len();

    // Without daily windows a date change is not a fresh budget, so the still
    // breached run total must not trip again.
    h.strategy.on_check();
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Idle);
    assert_eq!(h.exits.borrow().len(), exits_after_flatten);
}

#[rstest]
fn test_usd_currency_without_fx_does_not_trip() {
    let config = UserPnLConfig::builder()
        .venue(Venue::from("BINANCE"))
        .currency(Currency::USD())
        .max_loss(Decimal::from(-10_000))
        .reset_daily(false)
        .build();
    let mut h = make_strategy_with_config(vec![mm()], config);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(-20_000), false);
    h.strategy.on_check();

    assert_eq!(h.strategy.state(), UserPnLState::Idle);
    assert!(h.exits.borrow().is_empty());
}

#[rstest]
fn test_on_start_sets_timer() {
    let mut strategy = UserPnL::try_new(config()).unwrap();
    let clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    clock
        .borrow_mut()
        .register_default_handler(TimeEventCallback::from(|_: TimeEvent| {}));
    let clock_dyn: Rc<RefCell<dyn Clock>> = clock;
    let cache = Rc::new(RefCell::new(Cache::default()));
    let portfolio = Rc::new(RefCell::new(Portfolio::new(
        clock_dyn.clone(),
        cache.clone(),
        None,
    )));
    strategy
        .core
        .register(TraderId::from("TESTER-001"), clock_dyn, cache, portfolio)
        .unwrap();
    strategy.on_start().unwrap();
    assert!(
        strategy
            .clock()
            .timer_names()
            .iter()
            .any(|name| name == "user_pnl")
    );
}

#[rstest]
fn test_on_stop_resets_state() {
    let mut h = make_strategy(vec![mm()]);
    h.strategy.config.skip_flat_strategies = false;
    h.add_realized_pnl(mm(), Decimal::from(-20_000), false);
    h.strategy.on_check();
    assert_eq!(h.strategy.state(), UserPnLState::Halted);

    h.strategy.on_stop().unwrap();
    assert_eq!(h.strategy.state(), UserPnLState::Idle);
}
