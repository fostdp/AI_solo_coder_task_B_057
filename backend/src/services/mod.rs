pub mod lora_ingest;
pub mod collagen_kinetics;
pub mod ca_balance;
pub mod alerter;

pub use lora_ingest::LoraIngestService;
pub use collagen_kinetics::CollagenKineticsService;
pub use ca_balance::CaBalanceService;
pub use alerter::AlerterService;

use crate::models::SensorReading;

#[derive(Debug, Clone)]
pub struct KineticsPartial {
    pub relic_id: String,
    pub grid_x: f64,
    pub grid_y: f64,
    pub ph: f64,
    pub temperature: f64,
    pub ca_concentration: f64,
    pub orp: f64,
    pub collagen_deg_rate: f64,
    pub collagen_deg_percent: f64,
    pub abiotic_rate: f64,
    pub enzyme_rate: f64,
    pub enzyme_contribution_pct: f64,
    pub microbial_biomass: f64,
    pub elapsed_days: f64,
}

#[derive(Debug, Clone)]
pub enum ServiceMessage {
    Reading(SensorReading),
    Shutdown,
}
