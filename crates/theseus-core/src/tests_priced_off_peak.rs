//! A background call settles at the rate in force when it started
//! (theseus-058cl, route.v10 row 5): `TurnRunner::priced`, which every background
//! site books its call with, took the peak-only price, so a DeepSeek call in the
//! provider's off-peak hours (half price) booked at double. Reserves and budgets
//! stay at peak prices: that is the safe direction, and settling lower than the
//! hold only frees the difference.

use theseus_protocol::time::days_from_civil;
use theseus_protocol::Usage;

use crate::provider::ModelResponse;
use crate::tests_route::rig;

/// A UTC instant, in Unix milliseconds.
fn utc(y: i64, m: u32, d: u32, h: u32) -> u64 {
    ((days_from_civil(y, m, d) * 24 + i64::from(h)) * 60) as u64 * 60_000
}

/// Phoenix is UTC-7 with no daylight saving. 2026-10-27 is a Tuesday.
fn phx(h: u32) -> u64 {
    utc(2026, 10, 27, h) + 7 * 3_600_000
}

fn usage() -> Usage {
    Usage {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        ..Usage::default()
    }
}

fn answer(model: &str) -> ModelResponse {
    ModelResponse {
        model: model.into(),
        usage: usage(),
        ..ModelResponse::default()
    }
}

/// 02:00 Phoenix on a weekday is peak; 10:00 is off-peak, at half.
#[test]
fn a_flash_call_settles_at_the_rate_in_force_when_it_started() {
    let r = rig(None, 1, |_| {});
    let runner = &r.core.runner;
    let need = 9_000_000;
    let peak = runner.priced(&answer("deepseek-flash"), "deepseek-flash", need, phx(2));
    let off = runner.priced(&answer("deepseek-flash"), "deepseek-flash", need, phx(10));
    assert_eq!((peak, off), (1_500_000, 750_000));
}

/// A model with no off-peak row costs the same at either hour.
#[test]
fn a_model_without_an_off_peak_row_settles_the_same_at_any_hour() {
    let r = rig(None, 1, |_| {});
    let runner = &r.core.runner;
    let m = "claude-sonnet-5-5";
    let (a, b) = (
        runner.priced(&answer(m), m, 100_000_000, phx(2)),
        runner.priced(&answer(m), m, 100_000_000, phx(10)),
    );
    assert_eq!(a, b);
    assert!(
        a > 0 && a < 100_000_000,
        "priced by its row, not the reservation"
    );
}

/// The served model prices the call, else the asked one, else the reservation.
#[test]
fn the_served_model_then_the_asked_one_then_the_reservation() {
    let r = rig(None, 1, |_| {});
    let runner = &r.core.runner;
    let at = phx(10);
    let served_unknown = answer("not-in-the-catalog");
    assert_eq!(
        runner.priced(&served_unknown, "deepseek-flash", 9_000_000, at),
        750_000,
        "the asked model's off-peak price"
    );
    assert_eq!(
        runner.priced(&served_unknown, "also-unknown", 9_000_000, at),
        9_000_000,
        "no price: the reservation"
    );
}

/// The reservation a site holds is the peak worst case, whatever the hour: it is
/// the only number the off-peak price must not touch, and an off-peak settle is
/// always under it.
#[test]
fn the_reserve_stays_at_the_peak_price() {
    let r = rig(None, 1, |_| {});
    let flash = r.core.runner.catalog.get("deepseek-flash").unwrap().clone();
    let reserve = flash.reserve_micros(1_000_000, 1_000_000);
    assert_eq!(
        reserve,
        flash.cost_micros(&usage()),
        "the reserve is priced from the row's own (peak) prices"
    );
    let off = r.core.runner.priced(
        &answer("deepseek-flash"),
        "deepseek-flash",
        reserve,
        phx(10),
    );
    assert!(off < reserve);
}
