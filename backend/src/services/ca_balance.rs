use crate::config::AppConfig;
use crate::database::Database;
use crate::models::CorrosionAnalysis;
use crate::services::KineticsPartial;
use log::{info, debug, error};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

pub struct CaBalanceService {
    config: AppConfig,
    db: Database,
    rx: Arc<Mutex<mpsc::Receiver<KineticsPartial>>>,
    tx: mpsc::Sender<CorrosionAnalysis>,
}

impl CaBalanceService {
    pub fn new(
        config: AppConfig,
        db: Database,
        rx: mpsc::Receiver<KineticsPartial>,
        tx: mpsc::Sender<CorrosionAnalysis>,
    ) -> Self {
        Self {
            config,
            db,
            rx: Arc::new(Mutex::new(rx)),
            tx,
        }
    }

    fn ca_p_ratio_calc(&self, ca_ppm: f64, coll_deg_pct: f64) -> f64 {
        let cfg = &self.config.calcium_phosphate;
        if ca_ppm <= 0.0 {
            return cfg.stoichiometric_ca_p;
        }
        let ca_mg = ca_ppm * 0.5;
        let p_mg = ca_mg / cfg.stoichiometric_ca_p * (1.0 - (coll_deg_pct / 100.0) * 0.3);
        if p_mg > 1e-9 {
            ca_mg / p_mg
        } else {
            cfg.stoichiometric_ca_p
        }
    }

    fn ca_p_ratio_predicted(&self, ph: f64, temp_c: f64, elapsed_days: f64) -> f64 {
        let cfg = &self.config.calcium_phosphate;
        let h_plus = 10.0_f64.powf(-ph);
        let temp_factor = (-(temp_c - 25.0).abs() / 20.0).exp();

        let initial_ratio = cfg.stoichiometric_ca_p;
        let max_deviation = 0.8;
        let ph_driver = (h_plus * 1e4).min(1.0);

        let mut ratio = initial_ratio;
        let dt = 0.1;
        let steps = (elapsed_days / dt).min(100_000.0) as i32;
        for _ in 0..steps {
            let dissolution_rate = 5e-5 * ph_driver * temp_factor;
            ratio += dissolution_rate * max_deviation * dt / (1.0 + ratio / initial_ratio);
            ratio = ratio.min(initial_ratio + max_deviation);
        }
        ratio
    }

    fn dissolution_rate(&self, ph: f64, temp_c: f64, ca_ppm: f64, saturation_frac: f64) -> f64 {
        let cfg = &self.config.calcium_phosphate;
        let h_plus = 10.0_f64.powf(-ph);
        let temp_factor = 1.0 + 0.05 * (temp_c - 25.0);
        let ksp_factor = 1.0 - saturation_frac;

        let base_rate = 3.0e-7;
        base_rate * (h_plus * 1e6).powf(0.7) * temp_factor * ksp_factor.max(0.01)
    }

    fn corrosion_rate_um_per_year(&self, coll_rate: f64, diss_rate: f64, ph: f64) -> f64 {
        let cfg = &self.config.calcium_phosphate;
        let coll_um_per_year = coll_rate * 365.0 * 86400.0 * 0.5e4;
        let diss_um_per_year = diss_rate * 365.0 * 86400.0 * 1.0e4 * cfg.mineral_fraction;
        let combined = coll_um_per_year + diss_um_per_year;
        let ph_accel = if ph < 5.5 { 1.0 + (5.5 - ph) * 0.8 } else { 1.0 };
        combined * ph_accel
    }

    fn corrosion_depth_um(&self, rate_um_per_year: f64, elapsed_days: f64) -> f64 {
        rate_um_per_year * (elapsed_days / 365.0)
    }

    fn assess_risk(&self, ph: f64, ca_ppm: f64, deg_pct: f64, depth_um: f64) -> String {
        let mut score = 0u32;
        if ph < 5.5 {
            score += 2;
        } else if ph < 6.0 {
            score += 1;
        }
        if ca_ppm > 200.0 {
            score += 2;
        } else if ca_ppm > 100.0 {
            score += 1;
        }
        if deg_pct > 30.0 {
            score += 2;
        } else if deg_pct > 15.0 {
            score += 1;
        }
        if depth_um > 50.0 {
            score += 2;
        } else if depth_um > 20.0 {
            score += 1;
        }

        match score {
            0..=2 => "低".to_string(),
            3..=4 => "中".to_string(),
            5..=6 => "高".to_string(),
            _ => "极高".to_string(),
        }
    }

    fn build_analysis(&self, partial: &KineticsPartial) -> CorrosionAnalysis {
        let ca_ratio = self.ca_p_ratio_calc(partial.ca_concentration, partial.collagen_deg_percent);
        let ca_pred = self.ca_p_ratio_predicted(partial.ph, partial.temperature, partial.elapsed_days);
        let diss_rate = self.dissolution_rate(partial.ph, partial.temperature, partial.ca_concentration, 0.5);
        let cor_rate = self.corrosion_rate_um_per_year(partial.collagen_deg_rate, diss_rate, partial.ph);
        let cor_depth = self.corrosion_depth_um(cor_rate, partial.elapsed_days);
        let risk = self.assess_risk(partial.ph, partial.ca_concentration, partial.collagen_deg_percent, cor_depth);

        CorrosionAnalysis {
            relic_id: partial.relic_id.clone(),
            grid_x: partial.grid_x,
            grid_y: partial.grid_y,
            ph: partial.ph,
            temperature: partial.temperature,
            ca_concentration: partial.ca_concentration,
            orp: partial.orp,
            collagen_deg_rate: partial.collagen_deg_rate,
            collagen_deg_percent: partial.collagen_deg_percent,
            abiotic_rate: partial.abiotic_rate,
            enzyme_rate: partial.enzyme_rate,
            enzyme_contribution_pct: partial.enzyme_contribution_pct,
            microbial_biomass: partial.microbial_biomass,
            ca_p_ratio: ca_ratio,
            ca_p_ratio_predicted: ca_pred,
            corrosion_rate: cor_rate,
            corrosion_depth_um: cor_depth,
            risk_level: risk,
            timestamp: chrono::Utc::now(),
        }
    }

    pub async fn run(&self) {
        info!("[CaBalance] 服务启动，钙磷质量平衡模型就绪");
        let mut rx = self.rx.lock().await;

        while let Some(partial) = rx.recv().await {
            let analysis = self.build_analysis(&partial);

            debug!(
                "[CaBalance] {} | Ca/P={:.3} | 腐蚀速率={:.3} um/年 | 深度={:.2} um | 风险={}",
                analysis.relic_id,
                analysis.ca_p_ratio,
                analysis.collagen_deg_rate * 365.0 * 86400.0 * 0.5e4,
                analysis.corrosion_depth_um,
                analysis.risk_level
            );

            if let Err(e) = self.db.write_corrosion_analysis(&analysis).await {
                error!("[CaBalance] 写入腐蚀分析失败: {:?}", e);
            }

            match self.tx.send(analysis).await {
                Ok(_) => {}
                Err(e) => {
                    error!("[CaBalance] 发送到告警服务失败: {}", e);
                }
            }
        }

        info!("[CaBalance] 管道关闭，退出");
    }
}
