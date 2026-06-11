use crate::models::CorrosionAnalysis;
use crate::kinetics;
use chrono::Utc;

pub const GAS_CONSTANT_R: f64 = 8.314;
pub const ACTIVATION_ENERGY_EA: f64 = 85_000.0;
pub const PRE_EXPONENTIAL_A: f64 = 1.2e10;
pub const REFERENCE_TEMP_K: f64 = 298.15;

pub const HYDROXYAPATITE_CA_P_STOICHIOMETRIC: f64 = 1.667;

pub struct ArrheniusConfig {
    pub ea: f64,
    pub a: f64,
    pub r: f64,
    pub ph_acid_coeff: f64,
    pub ph_base_coeff: f64,
    pub ph_neutral_point: f64,
}

impl Default for ArrheniusConfig {
    fn default() -> Self {
        Self {
            ea: ACTIVATION_ENERGY_EA,
            a: PRE_EXPONENTIAL_A,
            r: GAS_CONSTANT_R,
            ph_acid_coeff: 4.5e-4,
            ph_base_coeff: 8.0e-5,
            ph_neutral_point: 7.0,
        }
    }
}

pub struct CalciumPhosphateConfig {
    pub stoichiometric_ca_p: f64,
    pub initial_bone_mass_g: f64,
    pub mineral_fraction: f64,
    pub organic_fraction: f64,
    pub water_fraction: f64,
    pub ca_in_hap_mass_frac: f64,
    pub p_in_hap_mass_frac: f64,
    pub solubility_constant_pksp: f64,
}

impl Default for CalciumPhosphateConfig {
    fn default() -> Self {
        Self {
            stoichiometric_ca_p: HYDROXYAPATITE_CA_P_STOICHIOMETRIC,
            initial_bone_mass_g: 100.0,
            mineral_fraction: 0.69,
            organic_fraction: 0.22,
            water_fraction: 0.09,
            ca_in_hap_mass_frac: 0.3989,
            p_in_hap_mass_frac: 0.1850,
            solubility_constant_pksp: 57.0,
        }
    }
}

pub fn arrhenius_rate_constant(temp_celsius: f64, config: &ArrheniusConfig) -> f64 {
    let temp_k = temp_celsius + 273.15;
    let exponent = -config.ea / (config.r * temp_k);
    config.a * exponent.exp()
}

pub fn ph_catalysis_factor(pH: f64, config: &ArrheniusConfig) -> f64 {
    let h_plus = 10.0_f64.powf(-pH);
    let oh_minus = 10.0_f64.powf(pH - 14.0);
    1.0 + config.ph_acid_coeff * h_plus + config.ph_base_coeff * oh_minus
}

pub fn orp_effect_factor(orp_mv: f64) -> f64 {
    let normalized = (orp_mv + 300.0) / 600.0;
    let clamped = normalized.clamp(0.0, 1.0);
    1.0 + 0.8 * clamped
}

pub fn collagen_hydrolysis_rate(
    temp_celsius: f64,
    pH: f64,
    orp_mv: f64,
    config: Option<&ArrheniusConfig>,
) -> f64 {
    let cfg = config.copied().unwrap_or_default();
    let k_arrhenius = arrhenius_rate_constant(temp_celsius, &cfg);
    let ph_factor = ph_catalysis_factor(pH, &cfg);
    let orp_factor = orp_effect_factor(orp_mv);
    k_arrhenius * ph_factor * orp_factor
}

pub fn collagen_degradation_percent(
    rate_constant: f64,
    elapsed_seconds: f64,
) -> f64 {
    let remaining = (-rate_constant * elapsed_seconds).exp();
    ((1.0 - remaining) * 100.0).clamp(0.0, 100.0)
}

pub fn expected_collagen_deg_elapsed_months(
    rate_constant: f64,
    months: f64,
) -> f64 {
    let seconds = months * 30.0 * 24.0 * 3600.0;
    collagen_degradation_percent(rate_constant, seconds)
}

pub struct MassBalanceState {
    pub dissolved_ca_mg: f64,
    pub dissolved_p_mg: f64,
    pub remaining_hap_g: f64,
    pub remaining_collagen_g: f64,
    pub solution_volume_l: f64,
    pub elapsed_days: f64,
}

impl Default for MassBalanceState {
    fn default() -> Self {
        let cfg = CalciumPhosphateConfig::default();
        let hap_mass = cfg.initial_bone_mass_g * cfg.mineral_fraction;
        let collagen_mass = cfg.initial_bone_mass_g * cfg.organic_fraction;
        Self {
            dissolved_ca_mg: 0.0,
            dissolved_p_mg: 0.0,
            remaining_hap_g: hap_mass,
            remaining_collagen_g: collagen_mass,
            solution_volume_l: 0.5,
            elapsed_days: 0.0,
        }
    }
}

pub fn hap_solubility(pH: f64, temp_celsius: f64, pksp: f64) -> f64 {
    let temp_factor = 1.0 + 0.02 * (temp_celsius - 20.0).max(0.0);
    let ph_effect = (1.0 / (10.0_f64.powf(-pH * 0.75))).ln().max(0.0) * 3.0 + 1.0;
    let ksp = 10.0_f64.powf(-pksp) * temp_factor * ph_effect;
    (ksp / 1.0e-58).powf(1.0 / 18.0) * 1.0e-3
}

pub fn dissolution_rate(
    pH: f64,
    temp_celsius: f64,
    ca_concentration_ppm: f64,
    volume_l: f64,
    config: &CalciumPhosphateConfig,
) -> f64 {
    let saturation = ca_concentration_ppm / 1000.0 / config.ca_in_hap_mass_frac;
    let saturation_ratio = (saturation / 0.5).min(2.0);
    let undersaturation_factor = (1.0 - saturation_ratio * 0.4).max(0.1);
    let temp_factor = arrhenius_rate_constant(temp_celsius, &ArrheniusConfig::default())
        / arrhenius_rate_constant(25.0, &ArrheniusConfig::default());
    let acid_factor = (7.0 - pH).max(0.0) * 2.5 + 1.0;
    let sol_rate = hap_solubility(pH, temp_celsius, config.solubility_constant_pksp);
    sol_rate * temp_factor * acid_factor * undersaturation_factor / volume_l
}

pub fn ca_p_ratio_current(state: &MassBalanceState) -> f64 {
    if state.dissolved_p_mg < 1e-9 {
        return HYDROXYAPATITE_CA_P_STOICHIOMETRIC;
    }
    state.dissolved_ca_mg / state.dissolved_p_mg
}

pub fn ca_p_ratio_predicted(
    pH: f64,
    temp_celsius: f64,
    elapsed_days: f64,
    config: Option<&CalciumPhosphateConfig>,
) -> f64 {
    let cfg = config.copied().unwrap_or_default();
    let hap_mass = cfg.initial_bone_mass_g * cfg.mineral_fraction;
    let coll_mass = cfg.initial_bone_mass_g * cfg.organic_fraction;
    let mut state = MassBalanceState {
        remaining_hap_g: hap_mass,
        remaining_collagen_g: coll_mass,
        solution_volume_l: 0.5,
        dissolved_ca_mg: 0.0,
        dissolved_p_mg: 0.0,
        elapsed_days: 0.0,
    };
    let dt_days = 1.0;
    let steps = (elapsed_days / dt_days) as usize;
    for _ in 0..steps {
        let ca_ppm = if state.solution_volume_l > 0.0 {
            state.dissolved_ca_mg / state.solution_volume_l
        } else { 0.0 };
        let rate = dissolution_rate(pH, temp_celsius, ca_ppm, state.solution_volume_l, &cfg);
        let hap_dissolved_g = rate * dt_days * 86400.0 * 0.001;
        let hap_dissolved = hap_dissolved_g.min(state.remaining_hap_g);
        state.remaining_hap_g -= hap_dissolved;
        state.dissolved_ca_mg += hap_dissolved * 1000.0 * cfg.ca_in_hap_mass_frac;
        state.dissolved_p_mg += hap_dissolved * 1000.0 * cfg.p_in_hap_mass_frac;
        state.elapsed_days += dt_days;
    }
    ca_p_ratio_current(&state)
}

pub fn ca_to_ppm(ca_mass_mg: f64, volume_l: f64) -> f64 {
    if volume_l <= 0.0 { 0.0 } else { ca_mass_mg / volume_l }
}

pub fn corrosion_rate_um_per_year(
    collagen_deg_rate: f64,
    dissolution_rate_: f64,
    pH: f64,
) -> f64 {
    let coll_factor = collagen_deg_rate * 1.0e7;
    let mineral_factor = dissolution_rate_ * 1.0e5;
    let ph_factor = if pH < 6.0 { (6.0 - pH) * 5.0 + 1.0 } else { 1.0 };
    (coll_factor + mineral_factor) * ph_factor * 0.1
}

pub fn estimate_corrosion_depth_um(
    rate_um_per_year: f64,
    elapsed_days: f64,
) -> f64 {
    rate_um_per_year * (elapsed_days / 365.25)
}

pub fn assess_risk_level(
    pH: f64,
    ca_ppm: f64,
    collagen_deg_pct: f64,
    corrosion_um: f64,
) -> String {
    let mut score = 0;
    if pH < 5.5 { score += 3; }
    else if pH < 6.0 { score += 1; }
    if ca_ppm > 200.0 { score += 3; }
    else if ca_ppm > 150.0 { score += 1; }
    if collagen_deg_pct > 30.0 { score += 3; }
    else if collagen_deg_pct > 15.0 { score += 1; }
    if corrosion_um > 100.0 { score += 2; }
    else if corrosion_um > 50.0 { score += 1; }
    match score {
        0..=1 => "LOW".to_string(),
        2..=3 => "MEDIUM".to_string(),
        4..=6 => "HIGH".to_string(),
        _ => "CRITICAL".to_string(),
    }
}

pub fn perform_full_analysis(
    relic_id: &str,
    grid_x: f64,
    grid_y: f64,
    pH: f64,
    temperature: f64,
    ca_ppm: f64,
    orp_mv: f64,
    elapsed_months: f64,
) -> CorrosionAnalysis {
    let arr_cfg = ArrheniusConfig::default();
    let mm_cfg = kinetics::MichaelisMentenConfig::default();

    let k_abiotic = collagen_hydrolysis_rate(temperature, pH, orp_mv, Some(&arr_cfg));

    let microbial_biomass = estimate_microbial_biomass(temperature, pH, orp_mv);
    let substrate_conc = 1.0;
    let k_enzyme = kinetics::enzyme_hydrolysis_rate(
        substrate_conc, microbial_biomass, temperature, pH, &mm_cfg,
    );

    let coll_rate = k_abiotic + k_enzyme;
    let coll_deg_pct = expected_collagen_deg_elapsed_months(coll_rate, elapsed_months);

    let enzyme_contribution_pct = if coll_rate > 0.0 {
        (k_enzyme / coll_rate) * 100.0
    } else { 0.0 };

    let elapsed_days = elapsed_months * 30.0;
    let ca_pred = ca_p_ratio_predicted(pH, temperature, elapsed_days, None);
    let diss_rate = dissolution_rate(pH, temperature, ca_ppm, 0.5, &CalciumPhosphateConfig::default());
    let cor_rate = corrosion_rate_um_per_year(coll_rate, diss_rate, pH);
    let cor_depth = estimate_corrosion_depth_um(cor_rate, elapsed_days);
    let risk = assess_risk_level(pH, ca_ppm, coll_deg_pct, cor_depth);

    CorrosionAnalysis {
        relic_id: relic_id.to_string(),
        grid_x,
        grid_y,
        ph: pH,
        temperature,
        ca_concentration: ca_ppm,
        orp: orp_mv,
        collagen_deg_rate: coll_rate,
        collagen_deg_percent: coll_deg_pct,
        abiotic_rate: k_abiotic,
        enzyme_rate: k_enzyme,
        enzyme_contribution_pct,
        microbial_biomass,
        ca_p_ratio: if ca_ppm > 0.0 {
            let ca_cfg = CalciumPhosphateConfig::default();
            let ca_mg = ca_ppm * 0.5;
            let p_mg = ca_mg / HYDROXYAPATITE_CA_P_STOICHIOMETRIC * (1.0 - (coll_deg_pct/100.0)*0.3);
            if p_mg > 1e-9 { ca_mg / p_mg } else { HYDROXYAPATITE_CA_P_STOICHIOMETRIC }
        } else { HYDROXYAPATITE_CA_P_STOICHIOMETRIC },
        ca_p_ratio_predicted: ca_pred,
        corrosion_rate: cor_rate,
        corrosion_depth_um: cor_depth,
        risk_level: risk,
        timestamp: Utc::now(),
    }
}

pub fn estimate_microbial_biomass(temp_celsius: f64, ph: f64, orp_mv: f64) -> f64 {
    let temp_factor = {
        let t_opt = 28.0;
        if temp_celsius < 0.0 {
            0.05
        } else if temp_celsius <= t_opt {
            0.1 + 0.9 * (temp_celsius / t_opt).powf(1.5)
        } else if temp_celsius < 45.0 {
            let excess = temp_celsius - t_opt;
            let decline = (-excess / 12.0).exp();
            1.0 * decline
        } else {
            0.1
        }
    };

    let ph_factor = {
        let ph_opt = 6.5;
        let delta = (ph - ph_opt).abs();
        (-delta * delta / (2.0 * 2.0 * 2.0)).exp()
    };

    let orp_factor = {
        let opt_orp = -50.0;
        let delta = (orp_mv - opt_orp).abs();
        (-delta / 400.0).exp()
    };

    let base_biomass = 0.15;
    base_biomass * temp_factor * ph_factor * orp_factor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arrhenius_normal_temp() {
        let k = arrhenius_rate_constant(25.0, &ArrheniusConfig::default());
        assert!(k > 0.0);
    }

    #[test]
    fn test_ph_factor_acidic() {
        let cfg = ArrheniusConfig::default();
        let f_ac = ph_catalysis_factor(4.0, &cfg);
        let f_ne = ph_catalysis_factor(7.0, &cfg);
        assert!(f_ac > f_ne);
    }

    #[test]
    fn test_collagen_deg_increases() {
        let r = collagen_hydrolysis_rate(25.0, 7.0, 100.0, None);
        let p1 = expected_collagen_deg_elapsed_months(r, 1.0);
        let p12 = expected_collagen_deg_elapsed_months(r, 12.0);
        assert!(p12 > p1);
    }

    #[test]
    fn test_ca_p_ratio_predicted() {
        let ratio = ca_p_ratio_predicted(7.0, 20.0, 365.0, None);
        assert!(ratio > 0.0);
    }

    #[test]
    fn test_risk_level_ph_low() {
        let r = assess_risk_level(4.0, 50.0, 5.0, 20.0);
        assert_eq!(r, "HIGH");
    }

    #[test]
    fn test_full_analysis() {
        let a = perform_full_analysis("RLC-0001", 5.0, 5.0, 6.5, 18.0, 80.0, 200.0, 3.0);
        assert!(a.collagen_deg_percent >= 0.0 && a.collagen_deg_percent <= 100.0);
        assert!(a.corrosion_depth_um >= 0.0);
    }
}
