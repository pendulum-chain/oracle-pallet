use log::error;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct PriceDivergenceAlert {
    pub asset: String,
    pub bp_divergence: f64,
    pub threshold_bp: u64,
    pub dark_oracle_price: f64,
    pub pyth_price: f64,
}


pub async fn run_divergence_alert_processor(mut rx: mpsc::Receiver<PriceDivergenceAlert>) {
    // TODO send slack alert potentially
    while let Some(alert) = rx.recv().await {
        error!(
            "{} price divergence too high: {:.2} bp > {} bp \
             (prices: DarkOracle: {}, Pyth: {})",
            alert.asset,
            alert.bp_divergence,
            alert.threshold_bp,
            alert.dark_oracle_price,
            alert.pyth_price,
        );
    }
}