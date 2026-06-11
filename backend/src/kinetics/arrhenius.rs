use super::ArrheniusConfig;

pub const DEFAULT_VMAX: f64 = 3.5e-7;
pub const DEFAULT_KM: f64 = 0.012;
pub const DEFAULT_ENZYME_CONC: f64 = 1.0;
pub const DEFAULT_SUBSTRATE_INIT: f64 = 1.0;

pub struct MichaelisMentenConfig {
    pub v_max: f64,
    pub km: f64,
    pub enzyme_concentration: f64,
    pub substrate_initial: f64,
    pub enzyme_ea: f64,
    pub enzyme_a: f64,
    pub ph_optimum: f64,
    pub ph_range: f64,
    pub temp_optimum_celsius: f64,
}

impl Default for MichaelisMentenConfig {
    fn default() -> Self {
        Self {
            v_max: env_or("MM_VMAX", DEFAULT_VMAX),
            km: env_or("MM_KM", DEFAULT_KM),
            enzyme_concentration: env_or("MM_ENZYME_CONC", DEFAULT_ENZYME_CONC),
            substrate_initial: env_or("MM_SUBSTRATE_INIT", DEFAULT_SUBSTRATE_INIT),
            enzyme_ea: env_or("MM_ENZYME_EA", 55_000.0),
            enzyme_a: env_or("MM_ENZYME_A", 8.0e8),
            ph_optimum: env_or("MM_PH_OPT", 6.8),
            ph_range: env_or("MM_PH_RANGE", 1.5),
            temp_optimum_celsius: env_or("MM_TEMP_OPT", 37.0),
        }
    }
}

fn env_or(key: &str, default: f64) -> f64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub fn michaelis_menten_rate(
    substrate_conc: f64,
    v_max: f64,
    km: f64,
) -> f64 {
    if substrate_conc <= 0.0 {
        return 0.0;
    }
    if km <= 0.0 {
        return v_max;
    }
    v_max * substrate_conc / (km + substrate_conc)
}

pub fn enzyme_ph_factor(ph: f64, config: &MichaelisMentenConfig) -> f64 {
    let delta = ph - config.ph_optimum;
    let factor = (-delta * delta / (2.0 * config.ph_range * config.ph_range)).exp();
    factor.max(0.0)
}

pub fn enzyme_temp_factor(temp_celsius: f64, config: &MichaelisMentenConfig) -> f64 {
    let temp_k = temp_celsius + 273.15;
    let opt_k = config.temp_optimum_celsius + 273.15;
    let r = 8.314;
    let arrhenius_factor =
        (config.enzyme_a * (-config.enzyme_ea / (r * temp_k)).exp()) /
        (config.enzyme_a * (-config.enzyme_ea / (r * opt_k)).exp());

    let denature_temp = config.temp_optimum_celsius + 15.0;
    let denature_factor = if temp_celsius > config.temp_optimum_celsius {
        let excess = temp_celsius - config.temp_optimum_celsius;
        let denature = -excess / denature_temp;
        denature.exp().max(0.1)
    } else {
        1.0
    };

    arrhenius_factor * denature_factor
}

pub fn enzyme_activity_factor(
    temp_celsius: f64,
    ph: f64,
    mm_config: &MichaelisMentenConfig,
) -> f64 {
    let ph_f = enzyme_ph_factor(ph, mm_config);
    let temp_f = enzyme_temp_factor(temp_celsius, mm_config);
    ph_f * temp_f
}

pub fn microbial_biomass_growth(
    current_biomass: f64,
    temp_celsius: f64,
    ph: f64,
    substrate_available: f64,
    dt_days: f64,
    mm_config: &MichaelisMentenConfig,
) -> f64 {
    let mu_max = 0.35;
    let ks = 0.05;
    let temp_factor = enzyme_temp_factor(temp_celsius, mm_config);
    let ph_factor = enzyme_ph_factor(ph, mm_config);
    let substrate_factor = substrate_available / (ks + substrate_available);
    let mu = mu_max * temp_factor * ph_factor * substrate_factor;
    let growth = current_biomass * mu * dt_days;
    let death_rate = 0.05;
    let death = current_biomass * death_rate * dt_days;
    (current_biomass + growth - death).max(0.0).min(5.0)
}

pub fn enzyme_hydrolysis_rate(
    substrate_conc: f64,
    biomass: f64,
    temp_celsius: f64,
    ph: f64,
    mm_config: &MichaelisMentenConfig,
) -> f64 {
    if biomass <= 0.0 || substrate_conc <= 0.0 {
        return 0.0;
    }
    let v_max_eff = mm_config.v_max * biomass * enzyme_activity_factor(temp_celsius, ph, mm_config);
    michaelis_menten_rate(substrate_conc, v_max_eff, mm_config.km)
}

pub fn total_collagen_hydrolysis_rate(
    temp_celsius: f64,
    ph: f64,
    orp_mv: f64,
    substrate_conc: f64,
    microbial_biomass: f64,
    arrhenius_config: Option<&ArrheniusConfig>,
    mm_config: Option<&MichaelisMentenConfig>,
) -> f64 {
    let arr_cfg = arrhenius_config.copied().unwrap_or_default();
    let mm_cfg = mm_config.copied().unwrap_or_default();

    let k_abiotic = arrhenius_rate(&arr_cfg, temp_celsius, ph, orp_mv);
    let k_enzyme = enzyme_hydrolysis_rate(
        substrate_conc, microbial_biomass,
        temp_celsius, ph, &mm_cfg,
    );

    k_abiotic + k_enzyme
}

fn arrhenius_rate(cfg: &ArrheniusConfig, temp_celsius: f64, ph: f64, orp_mv: f64) -> f64 {
    let temp_k = temp_celsius + 273.15;
    let exponent = -cfg.ea / (cfg.r * temp_k);
    let k_arr = cfg.a * exponent.exp();

    let h_plus = 10.0_f64.powf(-ph);
    let oh_minus = 10.0_f64.powf(ph - 14.0);
    let ph_factor = 1.0 + cfg.ph_acid_coeff * h_plus + cfg.ph_base_coeff * oh_minus;

    let normalized = (orp_mv + 300.0) / 600.0;
    let clamped = normalized.clamp(0.0, 1.0);
    let orp_factor = 1.0 + 0.8 * clamped;

    k_arr * ph_factor * orp_factor
}

pub struct EnzymeSimulationState {
    pub substrate: f64,
    pub biomass: f64,
    pub elapsed_days: f64,
    pub cumulative_deg: f64,
    pub history: Vec<(f64, f64, f64)>,
}

impl Default for EnzymeSimulationState {
    fn default() -> Self {
        Self {
            substrate: 1.0,
            biomass: 0.1,
            elapsed_days: 0.0,
            cumulative_deg: 0.0,
            history: Vec::new(),
        }
    }
}

pub fn simulate_enzyme_degradation(
    days: f64,
    temp_celsius: f64,
    ph: f64,
    orp_mv: f64,
    arrhenius_config: Option<&ArrheniusConfig>,
    mm_config: Option<&MichaelisMentenConfig>,
) -> EnzymeSimulationState {
    let mut state = EnzymeSimulationState::default();
    let dt = 0.05;
    let steps = (days / dt) as usize;
    let arr_cfg = arrhenius_config.copied().unwrap_or_default();
    let mm_cfg = mm_config.copied().unwrap_or_default();

    for _ in 0..steps {
        let k_total = total_collagen_hydrolysis_rate(
            temp_celsius, ph, orp_mv,
            state.substrate, state.biomass,
            Some(&arr_cfg), Some(&mm_cfg),
        );

        let deg_dt = k_total * state.substrate * dt * 86400.0;
        let actual_deg = deg_dt.min(state.substrate * 0.01);

        state.substrate = (state.substrate - actual_deg).max(0.0);
        state.cumulative_deg += actual_deg;

        state.biomass = microbial_biomass_growth(
            state.biomass, temp_celsius, ph,
            state.substrate, dt, &mm_cfg,
        );

        state.elapsed_days += dt;

        if state.history.len() < 500 {
            state.history.push((state.elapsed_days, state.cumulative_deg, state.biomass));
        } else if (state.elapsed_days as u32) % 7 == 0 {
            state.history.push((state.elapsed_days, state.cumulative_deg, state.biomass));
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArrheniusConfig;

    #[test]
    fn test_mm_saturation() {
        let r_low = michaelis_menten_rate(0.001, 1.0, 0.01);
        let r_high = michaelis_menten_rate(100.0, 1.0, 0.01);
        assert!(r_low < r_high);
        assert!((r_high - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_enzyme_ph_optimum() {
        let cfg = MichaelisMentenConfig::default();
        let f_opt = enzyme_ph_factor(cfg.ph_optimum, &cfg);
        let f_low = enzyme_ph_factor(4.0, &cfg);
        assert!(f_opt > f_low);
        assert!((f_opt - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_total_rate_with_enzyme() {
        let k_abiotic = total_collagen_hydrolysis_rate(
            20.0, 7.0, 150.0, 1.0, 0.0, None, None,
        );
        let k_with_enzyme = total_collagen_hydrolysis_rate(
            20.0, 7.0, 150.0, 1.0, 1.0, None, None,
        );
        assert!(k_with_enzyme > k_abiotic,
            "有微生物酶参与时水解速率应更快: {} vs {}",
            k_with_enzyme, k_abiotic);
    }

    #[test]
    fn test_simulation_increases() {
        let s = simulate_enzyme_degradation(30.0, 25.0, 6.5, 150.0, None, None);
        assert!(s.cumulative_deg > 0.0);
        assert!(s.substrate < 1.0);
        assert!(s.biomass > 0.0);
    }
}
