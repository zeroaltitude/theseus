//! `forecast.calibration` (theseus-zz752): how often a task's forecast held.
//! A read of the in-memory table of finished tasks (`crate::forecast`): no
//! frame, no lock on a session or an execution. The backtest is a few
//! million comparisons at the table's full size, so it runs on the blocking
//! pool, never on a runtime worker. Numbers only, so a shared place may ask.

use theseus_protocol::forecast::ForecastCalibrationResult;

use super::server::RpcFailure;
use super::Core;

impl Core {
    pub(super) async fn forecast_calibration(
        &self,
    ) -> Result<ForecastCalibrationResult, RpcFailure> {
        let book = self.tools.forecasts.clone();
        let now = theseus_protocol::now_unix_ms();
        tokio::task::spawn_blocking(move || book.calibration(now))
            .await
            .map_err(|e| {
                RpcFailure::invalid(anyhow::anyhow!("the calibration did not finish: {e}"))
            })
    }
}
