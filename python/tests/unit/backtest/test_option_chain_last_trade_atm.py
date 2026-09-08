# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Test last-trade ATM from a spot instrument during option-chain backtest.
"""

from __future__ import annotations

from pathlib import Path

from nautilus_trader.backtest import BacktestDataConfig
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.backtest import BacktestRunConfig
from nautilus_trader.backtest import BacktestVenueConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import AssetClass
from nautilus_trader.model import BookType
from nautilus_trader.model import Currency
from nautilus_trader.model import Equity
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OmsType
from nautilus_trader.model import OptionChainSlice
from nautilus_trader.model import OptionContract
from nautilus_trader.model import OptionKind
from nautilus_trader.model import OptionSeriesId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StrikeRange
from nautilus_trader.model import Symbol
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import TradeTick
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


USD = Currency.from_str("USD")
SPY_ID = InstrumentId.from_str("SPY.ARCA")
EXPIRATION_NS = 1_800_000_000_000_000_000
BASE_TS = 1_700_000_000_000_000_000
TICK = Price.from_str("0.01")
QTY = Quantity.from_int(1)
OPTION_BID = Price.from_str("1.00")
OPTION_ASK = Price.from_str("1.10")


class LastTradeAtmConfig(StrategyConfig):
    """
    Configuration for the last-trade ATM recorder.
    """

    def __init__(
        self,
        *,
        series_id: str,
        atm_instrument_id: str = "SPY.ARCA",
        **_kwargs: object,
    ) -> None:
        """
        Initialize the instance.
        """
        super().__init__()
        self.series_id = series_id
        self.atm_instrument_id = atm_instrument_id


class LastTradeAtmRecorder(Strategy):
    """
    Record option-chain ATM fields and the cached spot last trade.
    """

    def __init__(self, config: LastTradeAtmConfig) -> None:
        """
        Initialize the last-trade ATM recorder.
        """
        super().__init__(config)
        self._series_id = OptionSeriesId.from_str(config.series_id)
        self._atm_id = InstrumentId.from_str(config.atm_instrument_id)
        self.records: list[tuple[object, object, object, object]] = []
        self.catalog_ladders: list[list[Price]] = []

    def on_start(self) -> None:
        """
        On start.
        """
        self.subscribe_option_chain(
            self._series_id,
            strike_range=StrikeRange.atm_relative(1, 1),
            atm_instrument_id=self._atm_id,
            include_greeks=False,
        )

    def on_option_chain(self, slice: OptionChainSlice) -> None:
        """
        On option chain.
        """
        spot = self.cache.trade(self._atm_id)
        spot_price = None if spot is None else spot.price
        self.records.append(
            (slice.atm_instrument_id, slice.atm_price, slice.atm_strike, spot_price),
        )
        self.catalog_ladders.append(slice.listed_strikes())

    def on_stop(self) -> None:
        """
        On stop.
        """
        self.unsubscribe_option_chain(self._series_id)


def _equity() -> Equity:
    return Equity(
        instrument_id=SPY_ID,
        raw_symbol=Symbol("SPY"),
        currency=USD,
        price_precision=2,
        price_increment=TICK,
        ts_event=0,
        ts_init=0,
        lot_size=QTY,
    )


def _option(strike: str, kind: OptionKind) -> OptionContract:
    flag = "C" if kind == OptionKind.CALL else "P"
    ticker = f"SPY251014{flag}{int(float(strike) * 1000):08d}"
    return OptionContract(
        instrument_id=InstrumentId.from_str(f"{ticker}.OPRA"),
        raw_symbol=Symbol(ticker),
        asset_class=AssetClass.EQUITY,
        underlying="SPY",
        option_kind=kind,
        strike_price=Price.from_str(strike),
        currency=USD,
        activation_ns=0,
        expiration_ns=EXPIRATION_NS,
        price_precision=2,
        price_increment=TICK,
        multiplier=Quantity.from_int(100),
        lot_size=QTY,
        ts_event=0,
        ts_init=0,
    )


def _trade(price: str, ts: int, trade_id: str) -> TradeTick:
    return TradeTick(
        SPY_ID,
        Price.from_str(price),
        QTY,
        AggressorSide.NO_AGGRESSOR,
        TradeId(trade_id),
        ts,
        ts,
    )


def _quote(instrument_id: InstrumentId, ts: int) -> QuoteTick:
    return QuoteTick(
        instrument_id,
        OPTION_BID,
        OPTION_ASK,
        QTY,
        QTY,
        ts,
        ts,
    )


def test_option_chain_atm_follows_spot_last_trade(tmp_path: Path) -> None:
    """
    Test option-chain ATM price and strike follow SPY.ARCA last trades.
    """
    options = [
        _option("90.00", OptionKind.CALL),
        _option("90.00", OptionKind.PUT),
        _option("100.00", OptionKind.CALL),
        _option("100.00", OptionKind.PUT),
        _option("110.00", OptionKind.CALL),
        _option("110.00", OptionKind.PUT),
    ]
    quotes = [_quote(inst.id, BASE_TS + 2_000_000_000) for inst in options]
    quotes.append(_quote(options[2].id, BASE_TS + 4_000_000_000))
    trades = [
        _trade("100.00", BASE_TS + 1_000_000_000, "t1"),
        _trade("110.00", BASE_TS + 3_000_000_000, "t2"),
    ]

    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    catalog.write_instruments([_equity()])
    catalog.write_instruments(options)
    catalog.write_trade_ticks(trades)
    by_id: dict[InstrumentId, list[QuoteTick]] = {}
    for quote in quotes:
        by_id.setdefault(quote.instrument_id, []).append(quote)
    for group in by_id.values():
        catalog.write_quote_ticks(group)

    series = OptionSeriesId.from_option_contract(options[0])
    run_config = BacktestRunConfig(
        id="last-trade-atm",
        engine=BacktestEngineConfig(
            trader_id=TraderId("ATM-001"),
            bypass_logging=True,
            run_analysis=False,
        ),
        venues=[
            BacktestVenueConfig(
                name="OPRA",
                oms_type=OmsType.NETTING,
                account_type=AccountType.MARGIN,
                book_type=BookType.L1_MBP,
                starting_balances=["1000000 USD"],
            ),
            BacktestVenueConfig(
                name="ARCA",
                oms_type=OmsType.NETTING,
                account_type=AccountType.CASH,
                book_type=BookType.L1_MBP,
                starting_balances=["1000000 USD"],
            ),
        ],
        data=[
            BacktestDataConfig(
                data_type="QuoteTick",
                catalog_path=str(catalog_path),
                instrument_ids=[inst.id for inst in options],
            ),
            BacktestDataConfig(
                data_type="TradeTick",
                catalog_path=str(catalog_path),
                instrument_ids=[SPY_ID],
            ),
        ],
        dispose_on_completion=False,
    )
    strategy = LastTradeAtmRecorder(
        LastTradeAtmConfig(series_id=series.value, atm_instrument_id=str(SPY_ID)),
    )
    node = BacktestNode([run_config])
    try:
        node.build()
        node.add_strategy(run_config.id, strategy)
        node.run()
    finally:
        node.dispose()

    assert strategy.records, "expected option-chain slices during replay"
    atm_ids = {row[0] for row in strategy.records}
    assert atm_ids == {SPY_ID}, f"ATM must be last-trade SPY.ARCA, was {atm_ids}"

    for atm_id, atm_price, _atm_strike, spot_price in strategy.records:
        assert atm_id == SPY_ID
        assert atm_price is not None
        assert spot_price is not None
        assert atm_price == spot_price
        assert atm_price != OPTION_BID
        assert atm_price != OPTION_ASK

    prices = [row[1] for row in strategy.records]
    assert Price.from_str("100.00") in prices
    assert Price.from_str("110.00") in prices
    assert prices[-1] == Price.from_str("110.00")
    assert strategy.records[-1][2] == Price.from_str("110.00")


def _run_last_trade_atm(
    tmp_path: Path,
    quoted: list[OptionContract],
    definitions: list[OptionContract],
) -> LastTradeAtmRecorder:
    # Last-trade ATM must exist before option quotes, otherwise the manager
    # has no active window and later skips the empty post-trade snapshot.
    trades = [_trade("110.00", BASE_TS + 1_000_000_000, "t1")]
    quotes = [_quote(inst.id, BASE_TS + 2_000_000_000) for inst in quoted]
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    catalog.write_instruments([_equity()])
    catalog.write_instruments(definitions)
    catalog.write_trade_ticks(trades)
    by_id: dict[InstrumentId, list[QuoteTick]] = {}
    for quote in quotes:
        by_id.setdefault(quote.instrument_id, []).append(quote)
    for group in by_id.values():
        catalog.write_quote_ticks(group)

    series = OptionSeriesId.from_option_contract(definitions[0])
    run_config = BacktestRunConfig(
        id="last-trade-atm-ladder",
        engine=BacktestEngineConfig(
            trader_id=TraderId("ATM-001"),
            bypass_logging=True,
            run_analysis=False,
        ),
        venues=[
            BacktestVenueConfig(
                name="OPRA",
                oms_type=OmsType.NETTING,
                account_type=AccountType.MARGIN,
                book_type=BookType.L1_MBP,
                starting_balances=["1000000 USD"],
            ),
            BacktestVenueConfig(
                name="ARCA",
                oms_type=OmsType.NETTING,
                account_type=AccountType.CASH,
                book_type=BookType.L1_MBP,
                starting_balances=["1000000 USD"],
            ),
        ],
        data=[
            BacktestDataConfig(
                data_type="QuoteTick",
                catalog_path=str(catalog_path),
                instrument_ids=[inst.id for inst in quoted],
            ),
            BacktestDataConfig(
                data_type="TradeTick",
                catalog_path=str(catalog_path),
                instrument_ids=[SPY_ID],
            ),
        ],
        dispose_on_completion=False,
    )
    strategy = LastTradeAtmRecorder(
        LastTradeAtmConfig(series_id=series.value, atm_instrument_id=str(SPY_ID)),
    )
    node = BacktestNode([run_config])
    try:
        node.build()

        for inst in definitions:
            if inst.id not in {row.id for row in quoted}:
                node.add_instrument(run_config.id, inst)
        node.add_strategy(run_config.id, strategy)
        node.run()
    finally:
        node.dispose()
    return strategy


def test_option_chain_atm_uses_engine_registered_unquoted_strikes(tmp_path: Path) -> None:
    """
    Test ATM strike uses engine-registered contracts that have no quote stream.
    """
    quoted = [
        _option("90.00", OptionKind.CALL),
        _option("90.00", OptionKind.PUT),
    ]
    unquoted = [
        _option("110.00", OptionKind.CALL),
        _option("110.00", OptionKind.PUT),
    ]
    strategy = _run_last_trade_atm(tmp_path, quoted, quoted + unquoted)
    assert strategy.records, "expected option-chain slices during replay"
    strikes = {row[2] for row in strategy.records}
    assert Price.from_str("110.00") in strikes
    assert Price.from_str("90.00") not in strikes
    assert strategy.catalog_ladders, "expected listed catalog on published slices"
    ladder = strategy.catalog_ladders[-1]
    assert Price.from_str("90.00") in ladder
    assert Price.from_str("110.00") in ladder
    assert strategy.records[-1][1] == Price.from_str("110.00")
    assert strategy.records[-1][2] == Price.from_str("110.00")
