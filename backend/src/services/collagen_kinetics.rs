use crate::algorithms;
use crate::config::AppConfig;
use crate::kinetics;
use crate::models::{SensorReading, SensorType};
use crate::ode;
use crate::services::{KineticsPartial, ServiceMessage};
use log::{info, debug, warn, error};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

struct RelicSensorCache {
    ph: Option<(f64, chrono::DateTime<chrono::Utc>)>,
    orp: Option<(f64, chrono::DateTime<chrono::Utc>)>,
    temperature: Option<(f64, chrono::DateTime<chrono::Utc>)>,
    ca2: Option<(f64, chrono::DateTime<chrono::Utc>)>,
}

impl Default for RelicSensorCache {
    fn default() -> Self {
        Self {
            ph: None,
            orp: None,
            temperature: None,
            ca2: None,
        }
    }
}

pub struct CollagenKineticsService {
    config: AppConfig,
    rx: Arc<Mutex<mpsc::Receiver<ServiceMessage>>>,
    tx: mpsc::Sender<KineticsPartial>,
    cache: Arc<Mutex<HashMap<String, RelicSensorCache>>>,
    elapsed_months_default: f64,
}

impl CollagenKineticsService {
    pub fn new(
        config: AppConfig,
        rx: mpsc::Receiver<ServiceMessage>,
        tx: mpsc::Sender<KineticsPartial>,
    ) -> Self {
        Self {
            config,
            rx: Arc::new(Mutex::new(rx)),
            tx,
            cache: Arc::new(Mutex::new(HashMap::new())),
            elapsed_months_default: 12.0,
        }
    }

    fn arrhenius_rate(&self, temp_c: f64, ph: f64, orp_mv: f64) -> f64 {
        let cfg = &self.config.arrhenius;
        let temp_k = temp_c + 273.15;
        let k_arr = cfg.a_pre * (-cfg.ea / (cfg.r * temp_k)).exp();

        let h_plus = 10.0_f64.powf(-ph);
        let oh_minus = 10.0_f64.powf(ph - 14.0);
        let ph_factor = 1.0 + cfg.ph_acid_coeff * h_plus + cfg.ph_base_coeff * oh_minus;

        let norm_orp = ((orp_mv + 300.0) / 600.0).clamp(0.0, 1.0);
        let orp_factor = 1.0 + 0.8 * norm_orp;

        k_arr * ph_factor * orp_factor
    }

    fn enzyme_rate(&self, substrate: f64, biomass: f64, temp_c: f64, ph: f64) -> f64 {
        let mm_cfg = kinetics::MichaelisMentenConfig {
            v_max: self.config.michaelis_menten.v_max,
            km: self.config.michaelis_menten.km,
            enzyme_concentration: self.config.michaelis_menten.enzyme_concentration,
            substrate_initial: self.config.michaelis_menten.substrate_initial,
            enzyme_ea: self.config.michaelis_menten.enzyme_ea,
            enzyme_a: self.config.michaelis_menten.enzyme_a,
            ph_optimum: self.config.michaelis_menten.ph_optimum,
            ph_range: self.config.michaelis_menten.ph_range,
            temp_optimum_celsius: self.config.michaelis_menten.temp_optimum_celsius,
        };
        kinetics::enzyme_hydrolysis_rate(substrate, biomass, temp_c, ph, &mm_cfg)
    }

    fn microbial_biomass(&self, temp_c: f64, ph: f64, orp_mv: f64) -> f64 {
        algorithms::estimate_microbial_biomass(temp_c, ph, orp_mv)
    }

    async fn try_analyze_relic(&self, relic_id: &str, grid_x: f64, grid_y: f64) -> Option<KineticsPartial> {
        let cache = self.cache.lock().await;
        let entry = cache.get(relic_id)?;

        let (ph_val, _) = entry.ph?;
        let (orp_val, _) = entry.orp?;
        let (temp_val, _) = entry.temperature?;
        let (ca_val, _) = entry.ca2.unwrap_or((0.5, chrono::Utc::now()));

        let k_abiotic = self.arrhenius_rate(temp_val, ph_val, orp_val);
        let biomass = self.microbial_biomass(temp_val, ph_val, orp_val);
        let k_enzyme = self.enzyme_rate(1.0, biomass, temp_val, ph_val);

        let k_total = k_abiotic + k_enzyme;
        let elapsed_sec = self.elapsed_months_default * 30.0 * 86400.0;
        let deg_pct = ((1.0 - (-k_total * elapsed_sec).exp()) * 100.0).clamp(0.0, 100.0);

        let enzyme_pct = if k_total > 0.0 {
            (k_enzyme / k_total) * 100.0
        } else {
            0.0
        };

        let ode_cfg = ode::OdeSolverConfig {
            rtol: self.config.ode_solver.rtol,
            atol: self.config.ode_solver.atol,
            max_steps: self.config.ode_solver.max_steps,
            initial_dt: self.config.ode_solver.initial_dt,
            max_order: self.config.ode_solver.max_order,
            enforce_non_negative: self.config.ode_solver.enforce_non_negative,
        };

        let _ode_result = ode::solve_collagen_degradation(
            self.elapsed_months_default * 30.0,
            1.0,
            biomass,
            temp_val,
            ph_val,
            orp_val,
            Some(ode_cfg),
        );

        Some(KineticsPartial {
            relic_id: relic_id.to_string(),
            grid_x,
            grid_y,
            ph: ph_val,
            temperature: temp_val,
            ca_concentration: ca_val,
            orp: orp_val,
            collagen_deg_rate: k_total,
            collagen_deg_percent: deg_pct,
            abiotic_rate: k_abiotic,
            enzyme_rate: k_enzyme,
            enzyme_contribution_pct: enzyme_pct,
            microbial_biomass: biomass,
            elapsed_days: self.elapsed_months_default * 30.0,
        })
    }

    pub async fn run(&self) {
        info!("[CollagenKinetics] 服务启动，BDF ODE求解器就绪");
        let mut rx = self.rx.lock().await;

        while let Some(msg) = rx.recv().await {
            match msg {
                ServiceMessage::Reading(reading) => {
                    self.process_reading(reading).await;
                }
                ServiceMessage::Shutdown => {
                    info!("[CollagenKinetics] 收到关闭信号，退出");
                    break;
                }
            }
        }
    }

    async fn process_reading(&self, reading: SensorReading) {
        let relic_key = reading.relic_id.clone().unwrap_or_else(|| reading.sensor_id.clone());
        let ts = reading.timestamp.unwrap_or_else(chrono::Utc::now);

        {
            let mut cache = self.cache.lock().await;
            let entry = cache.entry(relic_key.clone()).or_default();

            match reading.sensor_type {
                SensorType::PH => {
                    entry.ph = Some((reading.value, ts));
                    if let Some(t) = reading.temperature {
                        entry.temperature = Some((t, ts));
                    }
                }
                SensorType::ORP => {
                    entry.orp = Some((reading.value, ts));
                }
                SensorType::CA2 => {
                    entry.ca2 = Some((reading.value, ts));
                }
                SensorType::TEMP => {
                    entry.temperature = Some((reading.value, ts));
                }
            }
        }

        if let Some(result) = self.try_analyze_relic(&relic_key, reading.grid_x, reading.grid_y).await {
            debug!(
                "[CollagenKinetics] {} | 速率={:.2e} s⁻¹ | 降解={:.2}% | 酶贡献={:.1}%",
                result.relic_id,
                result.collagen_deg_rate,
                result.collagen_deg_percent,
                result.enzyme_contribution_pct
            );

            match self.tx.send(result).await {
                Ok(_) => {}
                Err(e) => {
                    error!("[CollagenKinetics] 发送到钙磷服务失败: {}", e);
                }
            }
        } else {
            debug!("[CollagenKinetics] {} 数据不完整，暂存等待", relic_key);
        }
    }
}
