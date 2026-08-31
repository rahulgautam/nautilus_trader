# UserPnL

Account-level **daily** max loss and max profit for the whole trading node. One
sidecar watches combined PnL across every strategy. When the day's loss or
profit hits the bound, Nautilus squares off **all** of them through inherited
`market_exit()`, waits until there are **no open positions and no live orders**,
then stops those strategies and blocks new risk until the next UTC day.

Trading algorithms do **not** implement this. They already inherit
`market_exit()`. Registration order does not matter: the sidecar reads
registered strategy IDs from the trader at trip time.

## The common case: 20 strategies, 1% of capital per day

You have twenty SPX option strategies on one node (spreads, 0DTE, hedges, …).
Starting capital is `$1,000,000`. House rule: **lose 1% or make 1% in a day and
everything goes flat**.

That is not twenty per-strategy stops. It is one account kill-switch:

```python
STARTING_CAPITAL = 1_000_000
DAILY_PCT = 0.01
max_loss = -STARTING_CAPITAL * DAILY_PCT    # -10_000
max_profit = STARTING_CAPITAL * DAILY_PCT   # +10_000
```

| Clock         | Combined PnL today                          | UserPnL                                                          |
| ------------- | ------------------------------------------- | ---------------------------------------------------------------- |
| 10:12         | -$4,200 across 20 strategies                | Idle, they keep trading                                          |
| 13:47         | -$10,000                                    | `REDUCING`, every strategy `market_exit()`                       |
| 13:48         | Account flat (no positions, no live orders) | `HALTED` + `stop()` - no new orders the rest of **this** UTC day |
| Next UTC date | Daily window resets                         | `ACTIVE` + `start()`, fresh ±$10,000 budget                      |

Bounds are dollars, not percentages. Compute `1% of capital` in Python and pass
the dollar amount. It does not track a changing equity high-water mark.

`max_loss` must be ≤ 0. `max_profit` must be ≥ 0. At least one bound is
required. If both fire on the same check, **max_loss wins**.

## Python: live

Add the sidecar after the algos if you like; order is not required.

```python
from nautilus_trader.live import LiveNode
from nautilus_trader.model import Currency
from nautilus_trader.model import Venue
from nautilus_trader.trading import UserPnLConfig

STARTING_CAPITAL = 1_000_000
DAILY_PCT = 0.01

node = LiveNode.build("SPX-NODE", config)

# ... node.add_strategy(strategy) for each of the 20 algos ...

node.add_builtin_strategy(
    "UserPnL",
    UserPnLConfig(
        venue=Venue.from_str("CBOE"),  # venue whose portfolio PnL is summed
        currency=Currency.from_str("USD"),  # must match settlement, or be convertible
        max_loss=-STARTING_CAPITAL * DAILY_PCT,
        max_profit=STARTING_CAPITAL * DAILY_PCT,
        halt_day_on_max_loss=True,
        halt_day_on_max_profit=True,
        reset_daily=True,  # default; bounds are per UTC day, not the whole run
        check_interval_ms=200,
        flatten_redrive_ms=5_000,
        flatten_timeout_ms=30_000,
    ),
)

node.start()
node.run()
```

Algos need no changes. Optional nicety in quote/bar handlers:

```python
def on_quote(self, tick):
    if self.is_exiting():
        return
```

Without that, extra submits during flatten are denied with `MARKET_EXIT_IN_PROGRESS`.

## Python: backtest

Same sidecar, same config. The backtest clock is simulated time, so **day-wise
bounds are the normal backtest behaviour**: a six-month SPX run is many daily
windows, not one lifetime ±1%.

### `BacktestEngine`

```python
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.model import Currency
from nautilus_trader.model import Venue
from nautilus_trader.trading import UserPnLConfig

STARTING_CAPITAL = 1_000_000

engine = BacktestEngine(config)
# engine.add_venue(...) with starting balances = STARTING_CAPITAL
# engine.add_instrument(...) / add_data(...)
# engine.add_strategy(...)  # each of the 20

engine.add_builtin_strategy(
    "UserPnL",
    UserPnLConfig(
        venue=Venue.from_str("CBOE"),
        currency=Currency.from_str("USD"),
        max_loss=-STARTING_CAPITAL * 0.01,
        max_profit=STARTING_CAPITAL * 0.01,
        reset_daily=True,
    ),
)

engine.run()
```

### `BacktestNode` (high-level)

```python
node = BacktestNode(configs=[run_config])
node.build()

# node.add_strategy_from_config(run_config.id, ...) for each algo

node.add_builtin_strategy(
    run_config.id,
    "UserPnL",
    UserPnLConfig(
        venue=Venue.from_str("CBOE"),
        currency=Currency.from_str("USD"),
        max_loss=-STARTING_CAPITAL * 0.01,
        max_profit=STARTING_CAPITAL * 0.01,
        reset_daily=True,
    ),
)

results = node.run()
```

## How backtest day-wise UserPnL works

Live and backtest share the same Rust strategy. In backtest the clock jumps
with the data (ticks or bars). The `check_interval_ms` timer (200ms by default)
fires whenever simulated time advances past it.

```
IDLE --today's pnl <= max_loss or >= max_profit--> FLATTENING
FLATTENING --zero positions and zero live orders (same UTC day)--> stop() then HALTED
FLATTENING --zero positions after UTC date change--> ACTIVE (new day)
FLATTENING --UTC date changes--> FLATTENING (today's PnL resets; flatten continues)
HALTED --UTC date changes--> IDLE (ACTIVE + start())
```

1. Each check reads `portfolio.total_pnls_in(venue, account_id, currency)`
   (session realized + unrealized, converted into `currency` when a rate exists).
2. **Today's PnL** = that total minus the total captured at the last UTC
   midnight. First day of the run uses 0 as the anchor (typical backtest
   start). Live cache purges of closed positions can shift this difference.
3. On breach: `TradingState::Reducing` (algos cannot add size), then
   `Trader.market_exit_strategy(id)` for every **other** registered strategy
   with open risk. Flattening re-issues those exits every `flatten_redrive_ms`
   (default 5s) until books are empty, or until `flatten_timeout_ms` (default
   30s), after which it keeps `REDUCING` and keeps re-issuing; it does not latch
   `HALTED` while residual risk remains.
4. Each algo cancels its book and submits reduce-only **market** closes,
   waiting on in-flight orders. It does not reprice limits.
5. When managed risk is empty **on the same UTC day as the trip**: `stop()`
   those strategies, then `TradingState::Halted` for the rest of that day.
6. When simulated time crosses UTC midnight: today's PnL starts at 0 and
   trading is allowed again. If UserPnL itself latched `HALTED`, restore
   `ACTIVE` and `start()` only strategies it actually stopped. If flattening
   is still in progress, it stays in `FLATTENING` (re-drive and timeout keep
   running, `REDUCING` until books are empty) and then restores `ACTIVE`
   without `HALTED`: `max_loss` / `max_profit` do not carry into the new day.
   Closing that residual realizes into the new day, so if the new window is
   already through a bound the sidecar re-trips on the same check and never
   publishes `ACTIVE`.

So a backtest that loses 1% on 12 separate days will flatten **12 times**, not
once on the first bad day and sit dead for the remaining months.

Set `reset_daily=False` only if you want a **run-long** kill (first breach ends
the rest of the backtest). That is not the usual daily house rule.

Set `halt_day_on_max_loss=False` or `halt_day_on_max_profit=False` to flatten
until empty and then allow trading again that day. The sidecar will not re-trip
until PnL recovers inside the band.

### UTC day vs US cash session

The roll is the **UTC calendar date** on the clock, not 16:00 America/New_York.

For RTH-only SPX (09:30-16:00 ET) this is usually the same trading day: the
whole session sits on one UTC date, and midnight UTC is after the US close.
Start the node/backtest at or before the session you care about. Overnight
or globex books that cross 00:00 UTC will see a new daily window at UTC
midnight.

## What algos must implement

Nothing required.

| Algo state            | UserPnL                                                                     |
| --------------------- | --------------------------------------------------------------------------- |
| Flat, still `RUNNING` | Skipped for `market_exit` (`skip_flat_strategies`); still `stop()`d on halt |
| Called `stop()`       | Skipped if flat; `market_exit` no-ops if not `RUNNING`                      |
| Still long/short      | Flattened, then stopped                                                     |
| Python or Rust        | Same. `ExitMarket` is registered for every strategy                         |

UserPnL never calls `self.market_exit()`; it has no positions.

## Configuration

| Parameter                | Type                | Default    | Description                                                     |
| ------------------------ | ------------------- | ---------- | --------------------------------------------------------------- |
| `venue`                  | `Venue`             | *required* | Venue whose PnL is monitored.                                   |
| `account_id`             | `AccountId \| None` | `None`     | One account when set; else the venue.                           |
| `currency`               | `Currency`          | *required* | Bounds currency. Use venue settlement (FX is fail-open).        |
| `max_loss`               | `float \| None`     | `None`     | Trip when **today's** PnL ≤ this (must be ≤ 0).                 |
| `max_profit`             | `float \| None`     | `None`     | Trip when **today's** PnL ≥ this (must be ≥ 0).                 |
| `halt_day_on_max_loss`   | `bool`              | `True`     | After max-loss flatten, `HALTED` + `stop()` for **that** day.   |
| `halt_day_on_max_profit` | `bool`              | `True`     | After max-profit flatten, `HALTED` + `stop()` for **that** day. |
| `reset_daily`            | `bool`              | `True`     | Daily window + re-arm at UTC date change. `False` = whole run.  |
| `use_unrealized_only`    | `bool`              | `False`    | Unrealized only. Incompatible with `reset_daily`.               |
| `check_interval_ms`      | `int`               | `200`      | PnL sample period (simulated ms in backtest).                   |
| `flatten_redrive_ms`     | `int`               | `5_000`    | Re-issue `market_exit` at this cadence while flattening.        |
| `flatten_timeout_ms`     | `int`               | `30_000`   | Report a stuck flatten after this; keeps `REDUCING`.            |
| `managed_strategy_ids`   | `list[StrategyId]`  | `[]`       | Allowlist for exit/stop **and** the flat check. Empty = venue.  |
| `skip_flat_strategies`   | `bool`              | `True`     | Do not `market_exit` strategies with no live risk.              |

At least one of `max_loss` / `max_profit` is required.

The three intervals must be ordered
`check_interval_ms <= flatten_redrive_ms <= flatten_timeout_ms`, since both
flatten intervals are only evaluated on a check. Re-drive is kept coarser than
the sample period on purpose: PnL can be sampled fast without flooding logs or
the target's in-progress exit loop.

## Design notes

- Flatten is **delegated**. Do not reimplement wait/retry.
- Set `REDUCING` **before** flatten. `HALTED` first would deny the closes.
- Do not `HALTED` until managed risk is empty. Residual positions keep
  `REDUCING` and re-drive `market_exit`.
- When `managed_strategy_ids` is set, the flat check is that allowlist only.
  When it is empty, the gate is venue/account-wide (including unmanaged or
  external residual).
- Node `HALTED` is global. This sidecar may use it. A later PortfolioPnL group
  must not (that would freeze strategies that are not in the group). Day-roll
  restores `ACTIVE` only if UserPnL itself latched `HALTED`.
- Missing PnL (unpriced instruments or missing FX) does not trip (fail-open)
  and logs a warning. Prefer a `currency` that matches venue settlement.
- Session `total_pnls()` is this node run, not lifetime PnL across live restarts.
  Daily windows subtract a cache-based anchor; purging closed positions can
  shift that number.
- `is_exiting()` on an algo is true only while **that** algo's `market_exit`
  loop is running.
