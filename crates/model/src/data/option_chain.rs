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

//! Option chain data types for aggregated option series snapshots.

use std::{
    collections::{BTreeMap, HashSet},
    fmt::Display,
    ops::Deref,
};

use nautilus_core::{UnixNanos, serialization::Serializable};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use super::HasTsInit;
use crate::{
    data::{
        QuoteTick,
        greeks::{HasGreeks, OptionGreekValues},
    },
    enums::GreeksConvention,
    identifiers::{InstrumentId, OptionSeriesId},
    types::Price,
};

/// Number of strikes either side of ATM that [`StrikeRange::Delta`] selects as a
/// fallback when Greeks are not yet available for delta resolution.
pub(crate) const DEFAULT_DELTA_FALLBACK_STRIKES: usize = 5;

/// Defines which strikes to include in an option chain subscription.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StrikeRange {
    /// Subscribe to a fixed set of strike prices.
    Fixed(Vec<Price>),
    /// Subscribe to N listed strikes above and below ATM.
    ///
    /// Steps are taken over the series instruments in cache at subscribe (the
    /// OCC ladder), not over quoted `OptionChainSlice::strikes()`.
    AtmRelative {
        strikes_above: usize,
        strikes_below: usize,
    },
    /// Subscribe to strikes within a percentage band around ATM price.
    AtmPercent { pct: f64 },
    /// Subscribe to strikes whose absolute option delta is near `target`.
    ///
    /// Delta resolution needs Greeks, so the option chain aggregator performs it.
    /// The model-level [`StrikeRange::resolve`] has no Greeks and falls back to an
    /// ATM-relative window of `DEFAULT_DELTA_FALLBACK_STRIKES` strikes either side
    /// of ATM until Greeks are available.
    Delta { target: f64, tolerance: f64 },
}

impl StrikeRange {
    /// Resolves the filtered set of strikes from all available strikes.
    ///
    /// - `Fixed`: returns the fixed strikes directly (intersected with available).
    /// - `AtmRelative`: finds the closest strike to ATM, takes N above and N below.
    /// - `AtmPercent`: filters strikes within a percentage band around ATM.
    /// - `Delta`: has no Greeks at this level, so it falls back to an ATM-relative
    ///   window of `DEFAULT_DELTA_FALLBACK_STRIKES` strikes either side of ATM. The
    ///   option chain aggregator resolves `Delta` from Greeks instead.
    ///
    /// If `atm_price` is `None` for ATM-based variants, returns an empty vec
    /// (subscriptions are deferred until ATM is known).
    #[must_use]
    pub fn resolve(&self, atm_price: Option<Price>, all_strikes: &[Price]) -> Vec<Price> {
        match self {
            Self::Fixed(strikes) => {
                if all_strikes.is_empty() {
                    strikes.clone()
                } else {
                    let available: HashSet<Price> = all_strikes.iter().copied().collect();
                    strikes
                        .iter()
                        .filter(|s| available.contains(s))
                        .copied()
                        .collect()
                }
            }
            Self::AtmRelative {
                strikes_above,
                strikes_below,
            } => {
                let Some(atm) = atm_price else {
                    return vec![]; // Defer until ATM is known
                };
                // Find index of closest strike to ATM
                let atm_idx = match all_strikes.binary_search(&atm) {
                    Ok(idx) => idx,
                    Err(idx) => {
                        if idx == 0 {
                            0
                        } else if idx >= all_strikes.len() {
                            all_strikes.len() - 1
                        } else {
                            // Pick the closer of the two neighbors
                            let diff_below = all_strikes[idx - 1].raw.abs_diff(atm.raw);
                            let diff_above = all_strikes[idx].raw.abs_diff(atm.raw);
                            if diff_below <= diff_above {
                                idx - 1
                            } else {
                                idx
                            }
                        }
                    }
                };
                let start = atm_idx.saturating_sub(*strikes_below);
                let end = atm_idx
                    .saturating_add(*strikes_above)
                    .saturating_add(1)
                    .min(all_strikes.len());
                all_strikes[start..end].to_vec()
            }
            Self::AtmPercent { pct } => {
                let Some(atm) = atm_price else {
                    return vec![]; // Defer until ATM is known
                };
                let atm_decimal = atm.as_decimal();
                if atm_decimal.is_zero() {
                    return all_strikes.to_vec();
                }
                all_strikes
                    .iter()
                    .filter(|s| {
                        let distance = (s.as_decimal() - atm_decimal).abs();
                        let pct_diff = distance / atm_decimal.abs();
                        pct_diff.to_f64().is_some_and(|pct_diff| pct_diff <= *pct)
                    })
                    .copied()
                    .collect()
            }
            Self::Delta { .. } => Self::AtmRelative {
                strikes_above: DEFAULT_DELTA_FALLBACK_STRIKES,
                strikes_below: DEFAULT_DELTA_FALLBACK_STRIKES,
            }
            .resolve(atm_price, all_strikes),
        }
    }
}

/// Exchange-provided option Greeks and implied volatility for a single instrument.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct OptionGreeks {
    /// The instrument ID these Greeks apply to.
    pub instrument_id: InstrumentId,
    /// The numeraire convention these Greeks are expressed in.
    pub convention: GreeksConvention,
    /// Core Greek sensitivity values.
    pub greeks: OptionGreekValues,
    /// Mark implied volatility.
    pub mark_iv: Option<f64>,
    /// Bid implied volatility.
    pub bid_iv: Option<f64>,
    /// Ask implied volatility.
    pub ask_iv: Option<f64>,
    /// Underlying price at time of Greeks calculation.
    pub underlying_price: Option<f64>,
    /// Open interest for the instrument.
    pub open_interest: Option<f64>,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl HasTsInit for OptionGreeks {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl Deref for OptionGreeks {
    type Target = OptionGreekValues;
    fn deref(&self) -> &Self::Target {
        &self.greeks
    }
}

impl HasGreeks for OptionGreeks {
    fn greeks(&self) -> OptionGreekValues {
        self.greeks
    }
}

impl Default for OptionGreeks {
    fn default() -> Self {
        Self {
            instrument_id: InstrumentId::from("NULL.NULL"),
            convention: GreeksConvention::default(),
            greeks: OptionGreekValues::default(),
            mark_iv: None,
            bid_iv: None,
            ask_iv: None,
            underlying_price: None,
            open_interest: None,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }
}

impl Display for OptionGreeks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OptionGreeks({}, {}, delta={:.4}, gamma={:.4}, vega={:.4}, theta={:.4}, mark_iv={:?})",
            self.instrument_id,
            self.convention,
            self.delta,
            self.gamma,
            self.vega,
            self.theta,
            self.mark_iv
        )
    }
}

impl Serializable for OptionGreeks {}

/// Combined quote and Greeks data for a single strike in an option chain.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct OptionStrikeData {
    /// The latest quote for this strike.
    pub quote: QuoteTick,
    /// Exchange-provided Greeks (if available).
    pub greeks: Option<OptionGreeks>,
}

/// A point-in-time snapshot of an option chain for a single series.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct OptionChainSlice {
    /// The option series identifier.
    pub series_id: OptionSeriesId,
    /// The current ATM strike price (if determined).
    pub atm_strike: Option<Price>,
    /// The price used to window the chain (Greeks/HTTP reference or last trade).
    ///
    /// `None` until ATM is known. This is the tracker price, not a last-trade-only
    /// field. Last-trade mode is [`Self::atm_instrument_id`] being `Some`.
    pub atm_price: Option<Price>,
    /// Last-trade ATM instrument when subscribe set `atm_instrument_id`.
    ///
    /// `None` on the Greeks/reference path. Set from subscribe even before the first trade.
    pub atm_instrument_id: Option<InstrumentId>,
    /// Call option data keyed by strike price (sorted).
    pub calls: BTreeMap<Price, OptionStrikeData>,
    /// Put option data keyed by strike price (sorted).
    pub puts: BTreeMap<Price, OptionStrikeData>,
    /// Sorted unique strikes from the aggregator catalog (cache at subscribe).
    ///
    /// This is the ladder [`StrikeRange::AtmRelative`] steps. Quoted rows in
    /// [`Self::calls`] / [`Self::puts`] may omit some of these names.
    ///
    /// # Invariant
    ///
    /// Must be sorted ascending and deduplicated. [`Self::get_call_listed_offset`]
    /// and [`Self::get_put_listed_offset`] step it by index, so an unsorted vec
    /// silently resolves the wrong neighbor. The aggregator and the Python
    /// constructor both uphold this; hand-built slices must too.
    pub listed_strikes: Vec<Price>,
    /// UNIX timestamp (nanoseconds) when the snapshot event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl HasTsInit for OptionChainSlice {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl Display for OptionChainSlice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OptionChainSlice({}, atm={:?}, calls={}, puts={})",
            self.series_id,
            self.atm_strike,
            self.calls.len(),
            self.puts.len()
        )
    }
}

impl OptionChainSlice {
    /// Creates a new empty [`OptionChainSlice`] for the given series.
    #[must_use]
    pub fn new(series_id: OptionSeriesId) -> Self {
        Self {
            series_id,
            atm_strike: None,
            atm_price: None,
            atm_instrument_id: None,
            calls: BTreeMap::new(),
            puts: BTreeMap::new(),
            listed_strikes: Vec::new(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }

    /// Returns the number of call entries.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls.len()
    }

    /// Returns the number of put entries.
    #[must_use]
    pub fn put_count(&self) -> usize {
        self.puts.len()
    }

    /// Returns the call data for a given strike price.
    #[must_use]
    pub fn get_call(&self, strike: &Price) -> Option<&OptionStrikeData> {
        self.calls.get(strike)
    }

    /// Returns the put data for a given strike price.
    #[must_use]
    pub fn get_put(&self, strike: &Price) -> Option<&OptionStrikeData> {
        self.puts.get(strike)
    }

    /// Returns the call quote for a given strike price.
    #[must_use]
    pub fn get_call_quote(&self, strike: &Price) -> Option<&QuoteTick> {
        self.calls.get(strike).map(|d| &d.quote)
    }

    /// Returns the call Greeks for a given strike price.
    #[must_use]
    pub fn get_call_greeks(&self, strike: &Price) -> Option<&OptionGreeks> {
        self.calls.get(strike).and_then(|d| d.greeks.as_ref())
    }

    /// Returns the put quote for a given strike price.
    #[must_use]
    pub fn get_put_quote(&self, strike: &Price) -> Option<&QuoteTick> {
        self.puts.get(strike).map(|d| &d.quote)
    }

    /// Returns the put Greeks for a given strike price.
    #[must_use]
    pub fn get_put_greeks(&self, strike: &Price) -> Option<&OptionGreeks> {
        self.puts.get(strike).and_then(|d| d.greeks.as_ref())
    }

    /// Returns strike prices that currently have a quoted call or put row.
    ///
    /// This is not the full listed series. Missing quotes omit that strike.
    /// [`Self::get_call_atm_offset`] steps this set. Listed C[+N] / P[-N]
    /// uses [`Self::get_call_listed_offset`] / [`Self::get_put_listed_offset`].
    #[must_use]
    pub fn strikes(&self) -> Vec<Price> {
        let mut strikes: Vec<Price> = self.calls.keys().chain(self.puts.keys()).copied().collect();
        strikes.sort();
        strikes.dedup();
        strikes
    }

    /// Returns the total number of unique strikes.
    #[must_use]
    pub fn strike_count(&self) -> usize {
        self.strikes().len()
    }

    /// Returns `true` if the chain has no data.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.calls.is_empty() && self.puts.is_empty()
    }

    /// Steps `offset` places from [`Self::atm_strike`] along the strikes in this slice.
    ///
    /// The anchor is matched exactly, so the ATM strike must itself be present in
    /// [`Self::strikes`]. See [`Self::get_call_atm_offset`] for when that does not hold.
    fn strike_at_atm_offset(&self, offset: i32) -> Option<Price> {
        let atm = self.atm_strike?;
        let strikes = self.strikes();
        let atm_idx = strikes.iter().position(|s| *s == atm)?;
        let target = i32::try_from(atm_idx).ok()?.checked_add(offset)?;
        let target = usize::try_from(target).ok()?;
        strikes.get(target).copied()
    }

    /// Returns call data at `offset` quoted strikes from [`Self::atm_strike`].
    ///
    /// `0` is ATM, positive is higher strikes (OTM calls), negative is lower
    /// strikes (ITM calls). Steps are taken over [`Self::strikes`] (quoted rows
    /// in this snapshot), not the listed OCC series used by
    /// [`StrikeRange::AtmRelative`]. For listed C[+N] / P[-N], use
    /// [`Self::get_call_listed_offset`].
    ///
    /// Returns `None` when:
    /// - [`Self::atm_strike`] is unknown (no ATM price has been established yet).
    /// - [`Self::atm_strike`] has no row in this slice. It is the closest listed strike
    ///   across the whole series, so it is absent whenever it falls outside the active
    ///   strike window (such as a [`StrikeRange::Fixed`] range that excludes it) or has
    ///   not quoted yet. Every offset resolves to `None` while that is the case.
    /// - `offset` steps past either end of [`Self::strikes`].
    /// - The resolved strike has a put row but no call row.
    #[must_use]
    pub fn get_call_atm_offset(&self, offset: i32) -> Option<&OptionStrikeData> {
        self.strike_at_atm_offset(offset)
            .and_then(|strike| self.get_call(&strike))
    }

    /// Returns put data at `offset` quoted strikes from [`Self::atm_strike`].
    ///
    /// Same quoted-slice stepping as [`Self::get_call_atm_offset`], not listed OCC
    /// steps. `0` is ATM, negative is lower strikes (OTM puts), positive is higher
    /// strikes (ITM puts). Returns `None` under the same conditions, with the last
    /// inverted: the resolved strike has a call row but no put row.
    #[must_use]
    pub fn get_put_atm_offset(&self, offset: i32) -> Option<&OptionStrikeData> {
        self.strike_at_atm_offset(offset)
            .and_then(|strike| self.get_put(&strike))
    }

    /// Steps `offset` places from [`Self::atm_strike`] along [`Self::listed_strikes`].
    ///
    /// The anchor is an exact match on the catalog ladder, so [`Self::atm_strike`]
    /// must itself be present in [`Self::listed_strikes`].
    ///
    /// When the ATM price falls exactly between two listed strikes, the aggregator
    /// resolves [`Self::atm_strike`] to the **lower** one, so offsets are anchored
    /// there.
    fn strike_at_listed_offset(&self, offset: i32) -> Option<Price> {
        let atm = self.atm_strike?;
        let atm_idx = self.listed_strikes.iter().position(|s| *s == atm)?;
        let target = i32::try_from(atm_idx).ok()?.checked_add(offset)?;
        let target = usize::try_from(target).ok()?;
        self.listed_strikes.get(target).copied()
    }

    /// Returns call data at `offset` listed strikes from [`Self::atm_strike`].
    ///
    /// `0` is ATM, positive is higher strikes (OTM calls), negative is lower
    /// strikes (ITM calls). Steps are taken over [`Self::listed_strikes`] (the
    /// aggregator catalog, same ladder as [`StrikeRange::AtmRelative`]), then
    /// [`Self::get_call`]. A missing quote does not skip to the next listed
    /// strike.
    ///
    /// Returns `None` when:
    /// - [`Self::atm_strike`] is unknown.
    /// - [`Self::listed_strikes`] is empty, or does not contain [`Self::atm_strike`].
    /// - `offset` steps past either end of [`Self::listed_strikes`].
    /// - The listed strike has no call row in this snapshot.
    #[must_use]
    pub fn get_call_listed_offset(&self, offset: i32) -> Option<&OptionStrikeData> {
        self.strike_at_listed_offset(offset)
            .and_then(|strike| self.get_call(&strike))
    }

    /// Returns put data at `offset` listed strikes from [`Self::atm_strike`].
    ///
    /// Same catalog stepping as [`Self::get_call_listed_offset`]. `0` is ATM,
    /// negative is lower strikes (OTM puts), positive is higher strikes (ITM
    /// puts). Returns `None` under the same conditions, with the last inverted:
    /// the listed strike has no put row in this snapshot.
    #[must_use]
    pub fn get_put_listed_offset(&self, offset: i32) -> Option<&OptionStrikeData> {
        self.strike_at_listed_offset(offset)
            .and_then(|strike| self.get_put(&strike))
    }
}

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;
    use crate::{identifiers::Venue, types::Quantity};

    fn make_quote(instrument_id: InstrumentId) -> QuoteTick {
        QuoteTick::new(
            instrument_id,
            Price::from("100.00"),
            Price::from("101.00"),
            Quantity::from("1.0"),
            Quantity::from("1.0"),
            UnixNanos::from(1u64),
            UnixNanos::from(1u64),
        )
    }

    fn make_series_id() -> OptionSeriesId {
        OptionSeriesId::new(
            Venue::new("DERIBIT"),
            ustr::Ustr::from("BTC"),
            ustr::Ustr::from("BTC"),
            UnixNanos::from(1_700_000_000_000_000_000u64),
        )
    }

    #[rstest]
    fn test_strike_range_fixed() {
        let range = StrikeRange::Fixed(vec![Price::from("50000"), Price::from("55000")]);
        assert_eq!(
            range,
            StrikeRange::Fixed(vec![Price::from("50000"), Price::from("55000")])
        );
    }

    #[rstest]
    fn test_strike_range_atm_relative() {
        let range = StrikeRange::AtmRelative {
            strikes_above: 5,
            strikes_below: 5,
        };

        if let StrikeRange::AtmRelative {
            strikes_above,
            strikes_below,
        } = range
        {
            assert_eq!(strikes_above, 5);
            assert_eq!(strikes_below, 5);
        } else {
            panic!("Expected AtmRelative variant");
        }
    }

    #[rstest]
    fn test_strike_range_atm_percent() {
        let range = StrikeRange::AtmPercent { pct: 0.1 };
        if let StrikeRange::AtmPercent { pct } = range {
            assert!((pct - 0.1).abs() < f64::EPSILON);
        } else {
            panic!("Expected AtmPercent variant");
        }
    }

    #[rstest]
    fn test_option_greeks_default_fields() {
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            convention: GreeksConvention::BlackScholes,
            greeks: OptionGreekValues::default(),
            mark_iv: None,
            bid_iv: None,
            ask_iv: None,
            underlying_price: None,
            open_interest: None,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        };
        assert_eq!(greeks.delta, 0.0);
        assert_eq!(greeks.gamma, 0.0);
        assert_eq!(greeks.vega, 0.0);
        assert_eq!(greeks.theta, 0.0);
        assert!(greeks.mark_iv.is_none());
        assert_eq!(greeks.convention, GreeksConvention::BlackScholes);
    }

    #[rstest]
    fn test_option_greeks_default_is_black_scholes() {
        let greeks = OptionGreeks::default();
        assert_eq!(greeks.convention, GreeksConvention::BlackScholes);
    }

    #[rstest]
    fn test_option_greeks_display() {
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            convention: GreeksConvention::PriceAdjusted,
            greeks: OptionGreekValues {
                delta: 0.55,
                gamma: 0.001,
                vega: 10.0,
                theta: -5.0,
                rho: 0.0,
            },
            mark_iv: Some(0.65),
            bid_iv: None,
            ask_iv: None,
            underlying_price: None,
            open_interest: None,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        };
        let display = format!("{greeks}");
        assert!(display.contains("OptionGreeks"));
        assert!(display.contains("PRICE_ADJUSTED"));
        assert!(display.contains("0.55"));
    }

    #[rstest]
    fn test_option_greeks_data_serde_round_trip() {
        let greeks = OptionGreeks {
            instrument_id: InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
            convention: GreeksConvention::PriceAdjusted,
            greeks: OptionGreekValues {
                delta: 0.55,
                gamma: 0.001,
                vega: 10.0,
                theta: -5.0,
                rho: 0.2,
            },
            mark_iv: Some(0.65),
            bid_iv: None,
            ask_iv: Some(0.66),
            underlying_price: Some(50_000.0),
            open_interest: None,
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(2u64),
        };
        let data = crate::data::Data::OptionGreeks(greeks);

        let json = serde_json::to_string(&data).unwrap();
        let roundtripped: crate::data::Data = serde_json::from_str(&json).unwrap();

        assert_eq!(roundtripped, data);
    }

    #[rstest]
    fn test_option_chain_slice_empty() {
        let slice = OptionChainSlice {
            series_id: make_series_id(),
            atm_strike: None,
            atm_price: None,
            atm_instrument_id: None,
            calls: BTreeMap::new(),
            puts: BTreeMap::new(),
            listed_strikes: Vec::new(),
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(1u64),
        };

        assert!(slice.is_empty());
        assert_eq!(slice.strike_count(), 0);
        assert!(slice.strikes().is_empty());
        assert!(slice.listed_strikes.is_empty());
        assert!(slice.get_call_listed_offset(0).is_none());
    }

    #[rstest]
    fn test_option_chain_slice_with_data() {
        let call_id = InstrumentId::from("BTC-20240101-50000-C.DERIBIT");
        let put_id = InstrumentId::from("BTC-20240101-50000-P.DERIBIT");
        let strike = Price::from("50000");

        let mut calls = BTreeMap::new();
        calls.insert(
            strike,
            OptionStrikeData {
                quote: make_quote(call_id),
                greeks: Some(OptionGreeks {
                    instrument_id: call_id,
                    greeks: OptionGreekValues {
                        delta: 0.55,
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            },
        );

        let mut puts = BTreeMap::new();
        puts.insert(
            strike,
            OptionStrikeData {
                quote: make_quote(put_id),
                greeks: None,
            },
        );

        let slice = OptionChainSlice {
            series_id: make_series_id(),
            atm_strike: Some(strike),
            atm_price: None,
            atm_instrument_id: None,
            calls,
            puts,
            listed_strikes: vec![strike],
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(1u64),
        };

        assert!(!slice.is_empty());
        assert_eq!(slice.strike_count(), 1);
        assert_eq!(slice.strikes(), vec![strike]);
        assert!(slice.get_call(&strike).is_some());
        assert!(slice.get_put(&strike).is_some());
        assert!(slice.get_call_greeks(&strike).is_some());
        assert!(slice.get_put_greeks(&strike).is_none());
        assert_eq!(slice.get_call_greeks(&strike).unwrap().delta, 0.55);
    }

    fn make_strike_pair(strike: Price) -> (Price, OptionStrikeData, OptionStrikeData) {
        let call_id = InstrumentId::from(&format!("BTC-20240101-{strike}-C.DERIBIT"));
        let put_id = InstrumentId::from(&format!("BTC-20240101-{strike}-P.DERIBIT"));
        (
            strike,
            OptionStrikeData {
                quote: make_quote(call_id),
                greeks: None,
            },
            OptionStrikeData {
                quote: make_quote(put_id),
                greeks: None,
            },
        )
    }

    fn make_offset_slice(atm_strike: Option<Price>) -> OptionChainSlice {
        let mut calls = BTreeMap::new();
        let mut puts = BTreeMap::new();

        for strike in ["49000", "50000", "51000"] {
            let (strike, call, put) = make_strike_pair(Price::from(strike));
            calls.insert(strike, call);
            puts.insert(strike, put);
        }
        OptionChainSlice {
            series_id: make_series_id(),
            atm_strike,
            atm_price: None,
            atm_instrument_id: None,
            calls,
            puts,
            listed_strikes: vec![
                Price::from("49000"),
                Price::from("50000"),
                Price::from("51000"),
            ],
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(1u64),
        }
    }

    #[rstest]
    fn test_option_chain_slice_atm_offset_helpers() {
        let slice = make_offset_slice(Some(Price::from("50000")));

        assert_eq!(
            slice.get_call_atm_offset(0).unwrap().quote.instrument_id,
            InstrumentId::from("BTC-20240101-50000-C.DERIBIT"),
        );
        assert_eq!(
            slice.get_put_atm_offset(0).unwrap().quote.instrument_id,
            InstrumentId::from("BTC-20240101-50000-P.DERIBIT"),
        );
        assert_eq!(
            slice.get_call_atm_offset(1).unwrap().quote.instrument_id,
            InstrumentId::from("BTC-20240101-51000-C.DERIBIT"),
        );
        assert_eq!(
            slice.get_put_atm_offset(-1).unwrap().quote.instrument_id,
            InstrumentId::from("BTC-20240101-49000-P.DERIBIT"),
        );
        assert!(slice.get_call_atm_offset(2).is_none());
        assert!(slice.get_put_atm_offset(-2).is_none());
    }

    #[rstest]
    fn test_option_chain_slice_atm_offset_none_without_atm() {
        let slice = make_offset_slice(None);
        assert!(slice.get_call_atm_offset(0).is_none());
        assert!(slice.get_put_atm_offset(0).is_none());
    }

    #[rstest]
    fn test_option_chain_slice_atm_offset_none_when_atm_off_chain() {
        let slice = make_offset_slice(Some(Price::from("99999")));
        assert!(slice.get_call_atm_offset(0).is_none());
        assert!(slice.get_put_atm_offset(0).is_none());
    }

    /// `atm_strike` is the closest strike listed for the whole series, so it can name a
    /// strike with no row in this slice (outside the active window, or not yet quoted).
    /// Offsets are anchored on an exact match, so all of them resolve to `None`.
    #[rstest]
    fn test_option_chain_slice_atm_offset_none_when_atm_strike_absent_from_slice() {
        let mut slice = make_offset_slice(Some(Price::from("50000")));
        slice.calls.remove(&Price::from("50000"));
        slice.puts.remove(&Price::from("50000"));

        assert!(!slice.is_empty());
        assert!(slice.get_call_atm_offset(0).is_none());
        assert!(slice.get_call_atm_offset(1).is_none());
        assert!(slice.get_put_atm_offset(-1).is_none());
    }

    /// Offsets step over the union of call and put strikes, so a strike listed on only one
    /// side yields `None` for the other side rather than skipping to the next strike.
    #[rstest]
    fn test_option_chain_slice_atm_offset_none_when_side_missing_at_strike() {
        let mut slice = make_offset_slice(Some(Price::from("50000")));
        slice.calls.remove(&Price::from("51000"));

        assert!(slice.get_call_atm_offset(1).is_none());
        assert_eq!(
            slice.get_put_atm_offset(1).unwrap().quote.instrument_id,
            InstrumentId::from("BTC-20240101-51000-P.DERIBIT"),
        );
    }

    fn make_listed_hole_slice() -> OptionChainSlice {
        let mut calls = BTreeMap::new();
        let mut puts = BTreeMap::new();

        for strike in ["49000", "51000", "53000"] {
            let (strike, call, put) = make_strike_pair(Price::from(strike));
            calls.insert(strike, call);
            puts.insert(strike, put);
        }
        OptionChainSlice {
            series_id: make_series_id(),
            atm_strike: Some(Price::from("50000")),
            atm_price: Some(Price::from("50000")),
            atm_instrument_id: None,
            calls,
            puts,
            listed_strikes: vec![
                Price::from("49000"),
                Price::from("50000"),
                Price::from("51000"),
                Price::from("52000"),
                Price::from("53000"),
            ],
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(1u64),
        }
    }

    /// Quoted offsets fail closed when ATM itself has no row. Listed offsets still
    /// step the catalog, then `get_call` / `get_put` (missing quote stays `None`).
    #[rstest]
    fn test_option_chain_slice_listed_offset_steps_catalog_not_quoted_holes() {
        let slice = make_listed_hole_slice();

        assert_eq!(
            slice.strikes(),
            vec![
                Price::from("49000"),
                Price::from("51000"),
                Price::from("53000"),
            ],
        );
        assert!(slice.get_call_atm_offset(0).is_none());
        assert!(slice.get_call_atm_offset(1).is_none());

        assert!(slice.get_call_listed_offset(0).is_none());
        assert_eq!(
            slice.get_call_listed_offset(1).unwrap().quote.instrument_id,
            InstrumentId::from("BTC-20240101-51000-C.DERIBIT"),
        );
        assert!(slice.get_call_listed_offset(2).is_none());
        assert_eq!(
            slice.get_call_listed_offset(3).unwrap().quote.instrument_id,
            InstrumentId::from("BTC-20240101-53000-C.DERIBIT"),
        );
        assert!(slice.get_call_listed_offset(4).is_none());
        assert_eq!(
            slice.get_put_listed_offset(-1).unwrap().quote.instrument_id,
            InstrumentId::from("BTC-20240101-49000-P.DERIBIT"),
        );
    }

    #[rstest]
    fn test_option_chain_slice_listed_offset_none_without_catalog() {
        let mut slice = make_offset_slice(Some(Price::from("50000")));
        slice.listed_strikes.clear();
        assert!(slice.get_call_listed_offset(0).is_none());
        assert!(slice.get_put_listed_offset(0).is_none());
        assert!(slice.get_call_atm_offset(0).is_some());
    }

    #[rstest]
    fn test_option_chain_slice_listed_offset_matches_quoted_when_catalog_is_dense() {
        let slice = make_offset_slice(Some(Price::from("50000")));
        assert_eq!(
            slice.get_call_listed_offset(1).unwrap().quote.instrument_id,
            slice.get_call_atm_offset(1).unwrap().quote.instrument_id,
        );
        assert_eq!(
            slice.get_put_listed_offset(-1).unwrap().quote.instrument_id,
            slice.get_put_atm_offset(-1).unwrap().quote.instrument_id,
        );
    }

    #[rstest]
    fn test_option_chain_slice_display() {
        let slice = OptionChainSlice {
            series_id: make_series_id(),
            atm_strike: None,
            atm_price: None,
            atm_instrument_id: None,
            calls: BTreeMap::new(),
            puts: BTreeMap::new(),
            listed_strikes: Vec::new(),
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(1u64),
        };

        let display = format!("{slice}");
        assert!(display.contains("OptionChainSlice"));
        assert!(display.contains("DERIBIT"));
    }

    #[rstest]
    fn test_option_chain_slice_ts_init() {
        let slice = OptionChainSlice {
            series_id: make_series_id(),
            atm_strike: None,
            atm_price: None,
            atm_instrument_id: None,
            calls: BTreeMap::new(),
            puts: BTreeMap::new(),
            listed_strikes: Vec::new(),
            ts_event: UnixNanos::from(1u64),
            ts_init: UnixNanos::from(42u64),
        };

        assert_eq!(slice.ts_init(), UnixNanos::from(42u64));
    }

    // -- StrikeRange::resolve tests --

    #[rstest]
    fn test_strike_range_resolve_fixed() {
        let range = StrikeRange::Fixed(vec![Price::from("50000"), Price::from("55000")]);
        let result = range.resolve(None, &[]);
        assert_eq!(result, vec![Price::from("50000"), Price::from("55000")]);
    }

    #[rstest]
    fn test_strike_range_resolve_fixed_intersects_available_strikes() {
        let range = StrikeRange::Fixed(vec![
            Price::from("50000"),
            Price::from("55000"),
            Price::from("60000"),
        ]);
        let available = [
            Price::from("45000"),
            Price::from("50000"),
            Price::from("60000"),
        ];

        let result = range.resolve(None, &available);

        assert_eq!(result, vec![Price::from("50000"), Price::from("60000")]);
    }

    #[rstest]
    fn test_strike_range_resolve_atm_relative() {
        let range = StrikeRange::AtmRelative {
            strikes_above: 2,
            strikes_below: 2,
        };
        let strikes: Vec<Price> = [45000, 47000, 50000, 53000, 55000, 57000]
            .iter()
            .map(|s| Price::from(&s.to_string()))
            .collect();
        let atm = Some(Price::from("50000"));
        let result = range.resolve(atm, &strikes);
        // ATM at index 2, below=2 → start=0, above=2 → end=5
        assert_eq!(result.len(), 5);
        assert_eq!(result[0], Price::from("45000"));
        assert_eq!(result[4], Price::from("55000"));
    }

    #[rstest]
    #[case("49750", "50000")]
    #[case("50250", "50000")]
    #[case("51750", "52000")]
    fn test_strike_range_resolve_atm_relative_selects_nearest_strike(
        #[case] atm: &str,
        #[case] expected: &str,
    ) {
        let range = StrikeRange::AtmRelative {
            strikes_above: 0,
            strikes_below: 0,
        };
        let strikes = [
            Price::from("48000"),
            Price::from("50000"),
            Price::from("52000"),
        ];

        let result = range.resolve(Some(Price::from(atm)), &strikes);

        assert_eq!(result, vec![Price::from(expected)]);
    }

    #[rstest]
    fn test_strike_range_resolve_atm_relative_exact_high_value() {
        let range = StrikeRange::AtmRelative {
            strikes_above: 0,
            strikes_below: 0,
        };
        let atm = Price::from("9007199253.999000000");
        let collapsed = Price::from("9007199253.999000001");
        let strikes = [atm, collapsed];
        assert_eq!(collapsed.as_f64(), atm.as_f64());

        let result = range.resolve(Some(atm), &strikes);

        assert_eq!(result, vec![atm]);
    }

    #[rstest]
    fn test_strike_range_resolve_atm_relative_saturates_extreme_window() {
        // An extreme window must clamp to the available strikes without overflowing
        let range = StrikeRange::AtmRelative {
            strikes_above: usize::MAX,
            strikes_below: usize::MAX,
        };
        let strikes: Vec<Price> = [45000, 50000, 55000]
            .iter()
            .map(|s| Price::from(&s.to_string()))
            .collect();
        let atm = Some(Price::from("50000"));

        let result = range.resolve(atm, &strikes);

        assert_eq!(result, strikes);
    }

    #[rstest]
    fn test_strike_range_resolve_atm_relative_no_atm() {
        let range = StrikeRange::AtmRelative {
            strikes_above: 2,
            strikes_below: 2,
        };
        let strikes = vec![Price::from("50000"), Price::from("55000")];
        let result = range.resolve(None, &strikes);
        // No ATM → return empty (deferred until ATM known)
        assert!(result.is_empty());
    }

    #[rstest]
    fn test_strike_range_resolve_atm_percent() {
        let range = StrikeRange::AtmPercent { pct: 0.1 }; // 10%
        let strikes: Vec<Price> = [45000, 48000, 50000, 52000, 55000, 60000]
            .iter()
            .map(|s| Price::from(&s.to_string()))
            .collect();
        let atm = Some(Price::from("50000"));
        let result = range.resolve(atm, &strikes);
        // 10% of 50000 = 5000, so [45000..55000] inclusive (<=)
        assert_eq!(result.len(), 5); // 45000, 48000, 50000, 52000, 55000
        assert!(result.contains(&Price::from("45000")));
        assert!(result.contains(&Price::from("48000")));
        assert!(result.contains(&Price::from("50000")));
        assert!(result.contains(&Price::from("52000")));
        assert!(result.contains(&Price::from("55000")));
    }

    #[rstest]
    fn test_strike_range_resolve_atm_percent_zero_exact_high_value() {
        let range = StrikeRange::AtmPercent { pct: 0.0 };
        let atm = Price::from("9007199253.999000000");
        let collapsed = Price::from("9007199253.999000001");
        let strikes = [atm, collapsed];
        assert_eq!(atm.as_f64(), collapsed.as_f64());

        let result = range.resolve(Some(atm), &strikes);

        assert_eq!(result, vec![atm]);
    }

    #[rstest]
    fn test_option_chain_slice_new_empty() {
        let slice = OptionChainSlice::new(make_series_id());
        assert!(slice.is_empty());
        assert_eq!(slice.call_count(), 0);
        assert_eq!(slice.put_count(), 0);
        assert!(slice.atm_strike.is_none());
    }

    #[rstest]
    fn test_strike_range_resolve_delta_falls_back_to_atm_relative() {
        // The model-level resolve has no Greeks, so Delta delegates to an
        // AtmRelative window of DEFAULT_DELTA_FALLBACK_STRIKES either side of ATM.
        let strikes: Vec<Price> = (0..=20)
            .map(|i| Price::from(&(40000 + i * 1000).to_string()))
            .collect();
        let atm = Some(Price::from("50000")); // index 10
        let delta = StrikeRange::Delta {
            target: 0.25,
            tolerance: 0.05,
        };
        let expected = StrikeRange::AtmRelative {
            strikes_above: DEFAULT_DELTA_FALLBACK_STRIKES,
            strikes_below: DEFAULT_DELTA_FALLBACK_STRIKES,
        }
        .resolve(atm, &strikes);

        let result = delta.resolve(atm, &strikes);
        assert_eq!(result, expected);
        assert_eq!(result.len(), 2 * DEFAULT_DELTA_FALLBACK_STRIKES + 1);
        assert!(result.contains(&Price::from("50000")));
        assert!(!result.contains(&Price::from("40000")));
        assert!(!result.contains(&Price::from("60000")));
    }

    #[rstest]
    fn test_strike_range_resolve_delta_empty_without_atm() {
        let delta = StrikeRange::Delta {
            target: 0.25,
            tolerance: 0.05,
        };
        let strikes = vec![Price::from("50000"), Price::from("55000")];
        // No ATM -> deferred (empty), matching ATM-relative behavior.
        assert!(delta.resolve(None, &strikes).is_empty());
    }
}
