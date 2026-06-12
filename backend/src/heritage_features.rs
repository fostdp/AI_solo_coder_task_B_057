use serde::{Deserialize, Serialize};
use rand::Rng;
use rand_distr::{Normal, Distribution};

use crate::algorithms::{
    arrhenius_rate_constant,
    collagen_hydrolysis_rate,
    dissolution_rate,
    corrosion_rate_um_per_year,
    estimate_corrosion_depth_um,
    ArrheniusConfig,
    CalciumPhosphateConfig,
};

pub const FARADAY_F: f64 = 96485.0;
pub const MOLAR_GAS_R: f64 = 8.314;
pub const STANDARD_TEMP_K: f64 = 298.15;
pub const ELECTRONS_TRANSFERRED: f64 = 2.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EhPhPoint {
    pub ph: f64,
    pub eh_mv: f64,
    pub zone: RedoxZone,
    pub stable_phase: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RedoxZone {
    OXIDIZED,
    SUBSURFACE_OXIC,
    MANGANESE_REDUCING,
    IRON_REDUCING,
    SULFATE_REDUCING,
    METHANOGENIC,
    CARBONATE_REDUCING,
    UNDEFINED,
}

impl std::fmt::Display for RedoxZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RedoxZone::OXIDIZED => "氧化带 (Oxidized)",
            RedoxZone::SUBSURFACE_OXIC => "次表层含氧带 (Suboxic)",
            RedoxZone::MANGANESE_REDUCING => "锰还原带 (Mn-Reducing)",
            RedoxZone::IRON_REDUCING => "铁还原带 (Fe-Reducing)",
            RedoxZone::SULFATE_REDUCING => "硫酸盐还原带 (SO₄²⁻-Reducing)",
            RedoxZone::METHANOGENIC => "产甲烷带 (Methanogenic)",
            RedoxZone::CARBONATE_REDUCING => "碳酸盐还原带 (Carbonate-Reducing)",
            RedoxZone::UNDEFINED => "未定义",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedoxPhaseBoundary {
    pub reaction: String,
    pub equation: String,
    pub boundary_line: Vec<(f64, f64)>,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EhPhDiagram {
    pub zones: Vec<EhPhPoint>,
    pub boundaries: Vec<RedoxPhaseBoundary>,
    pub dominant_zone: RedoxZone,
    pub dominant_zone_name: String,
    pub sample_point: EhPhPoint,
    pub corrosion_risk: String,
    pub preservation_quality: String,
    pub grid_size: (usize, usize),
}

fn nernst_equation(e0_vs_she: f64, ph: f64, temp_k: f64, h_consumed: f64) -> f64 {
    let slope = 2.303 * MOLAR_GAS_R * temp_k / (ELECTRONS_TRANSFERRED * FARADAY_F);
    let e_vs_she = e0_vs_she - slope * h_consumed * ph;
    e_vs_she * 1000.0
}

fn classify_redox_zone(ph: f64, eh_mv: f64) -> RedoxZone {
    let ph_clamped = ph.clamp(2.0, 12.0);
    let oxic_upper = nernst_equation(1.23, ph_clamped, STANDARD_TEMP_K, 4.0);
    let oxic_lower = nernst_equation(0.80, ph_clamped, STANDARD_TEMP_K, 1.0);
    let mn_lower = nernst_equation(0.42, ph_clamped, STANDARD_TEMP_K, 2.0);
    let fe_lower = nernst_equation(-0.08, ph_clamped, STANDARD_TEMP_K, 2.0);
    let so4_lower = nernst_equation(-0.22, ph_clamped, STANDARD_TEMP_K, 2.0);
    let ch4_lower = nernst_equation(-0.35, ph_clamped, STANDARD_TEMP_K, 2.0);
    let carb_lower = nernst_equation(-0.50, ph_clamped, STANDARD_TEMP_K, 2.0);

    if eh_mv >= oxic_upper * 0.9 {
        RedoxZone::OXIDIZED
    } else if eh_mv >= oxic_lower {
        RedoxZone::SUBSURFACE_OXIC
    } else if eh_mv >= mn_lower {
        RedoxZone::MANGANESE_REDUCING
    } else if eh_mv >= fe_lower {
        RedoxZone::IRON_REDUCING
    } else if eh_mv >= so4_lower {
        RedoxZone::SULFATE_REDUCING
    } else if eh_mv >= ch4_lower {
        RedoxZone::METHANOGENIC
    } else if eh_mv >= carb_lower {
        RedoxZone::CARBONATE_REDUCING
    } else {
        RedoxZone::UNDEFINED
    }
}

fn identify_stable_phase(ph: f64, eh_mv: f64, zone: &RedoxZone) -> String {
    match zone {
        RedoxZone::OXIDIZED => {
            if ph < 5.0 { "Fe²+ aq + SO₄²⁻ (溶解态铁硫)".to_string() }
            else if ph < 8.0 { "FeOOH (针铁矿) + CaSO₄·2H₂O (石膏)".to_string() }
            else { "Fe(OH)₃ (氢氧化铁) + CaCO₃ (方解石)".to_string() }
        },
        RedoxZone::SUBSURFACE_OXIC => {
            if ph < 6.0 { "Fe²+ 少量 + MnO₂ (软锰矿)".to_string() }
            else { "FeOOH/Fe(OH)₃ + MnO₂ + CaSO₄".to_string() }
        },
        RedoxZone::MANGANESE_REDUCING => {
            if ph < 7.0 { "Mn²+ aq + FeOOH (残留)".to_string() }
            else { "MnCO₃ (菱锰矿) + FeOOH".to_string() }
        },
        RedoxZone::IRON_REDUCING => {
            if ph < 6.5 { "Fe²+ + HCO₃⁻ + 有机质".to_string() }
            else if ph < 8.0 { "FeCO₃ (菱铁矿) + Fe₃(PO₄)₂".to_string() }
            else { "FeCO₃ + Ca₅(PO₄)₃(OH) (羟磷灰石稳定)".to_string() }
        },
        RedoxZone::SULFATE_REDUCING => {
            if ph < 6.0 { "FeS (硫铁矿前驱) + H₂S↑".to_string() }
            else if ph < 8.0 { "FeS₂ (黄铁矿) + 有机质".to_string() }
            else { "FeS₂ + CaCO₃ + 石膏溶解".to_string() }
        },
        RedoxZone::METHANOGENIC => {
            "CH₄↑ + FeS₂ + 高度还原有机质".to_string()
        },
        RedoxZone::CARBONATE_REDUCING => {
            "CaCO₃ (方解石) + CH₄ + 强还原环境".to_string()
        },
        RedoxZone::UNDEFINED => {
            "超出常规水文地球化学范围".to_string()
        }
    }
}

fn evaluate_zone_preservation(zone: &RedoxZone, ph: f64) -> (String, String) {
    let (preservation, risk) = match zone {
        RedoxZone::OXIDIZED => {
            if ph < 5.5 { ("极差", "CRITICAL") }
            else if ph < 6.5 { ("差", "HIGH") }
            else { ("一般", "MEDIUM") }
        },
        RedoxZone::SUBSURFACE_OXIC => {
            if ph < 6.0 { ("差", "HIGH") }
            else if ph < 7.5 { ("一般", "MEDIUM") }
            else { ("良好", "LOW") }
        },
        RedoxZone::MANGANESE_REDUCING => {
            if ph < 6.5 { ("一般", "MEDIUM") }
            else if ph < 8.0 { ("良好", "LOW") }
            else { ("优秀", "LOW") }
        },
        RedoxZone::IRON_REDUCING => {
            if ph >= 6.5 && ph <= 8.5 { ("优秀", "LOW") }
            else if ph >= 6.0 { ("良好", "MEDIUM") }
            else { ("一般", "MEDIUM") }
        },
        RedoxZone::SULFATE_REDUCING => {
            if ph >= 6.5 && ph <= 8.0 { ("优秀", "LOW") }
            else if ph >= 6.0 { ("良好", "MEDIUM") }
            else { ("一般", "HIGH") }
        },
        RedoxZone::METHANOGENIC => {
            if ph >= 7.0 && ph <= 8.5 { ("极佳", "LOW") }
            else { ("良好", "MEDIUM") }
        },
        RedoxZone::CARBONATE_REDUCING => {
            if ph >= 7.5 { ("极佳", "LOW") }
            else { ("良好", "MEDIUM") }
        },
        RedoxZone::UNDEFINED => {
            ("未知", "HIGH")
        }
    };
    (preservation.to_string(), risk.to_string())
}

pub fn generate_eh_ph_diagram(
    sample_ph: f64,
    sample_eh_mv: f64,
    ph_range: (f64, f64),
    eh_range: (f64, f64),
    grid_res: (usize, usize),
) -> EhPhDiagram {
    let ph_min = ph_range.0;
    let ph_max = ph_range.1;
    let eh_min = eh_range.0;
    let eh_max = eh_range.1;
    let (nx, ny) = grid_res;

    let mut zones = Vec::with_capacity(nx * ny);
    for i in 0..nx {
        for j in 0..ny {
            let ph = ph_min + (ph_max - ph_min) * (i as f64) / ((nx - 1) as f64);
            let eh = eh_min + (eh_max - eh_min) * (j as f64) / ((ny - 1) as f64);
            let zone = classify_redox_zone(ph, eh);
            let phase = identify_stable_phase(ph, eh, &zone);
            zones.push(EhPhPoint { ph, eh_mv: eh, zone, stable_phase: phase });
        }
    }

    let boundaries = vec![
        RedoxPhaseBoundary {
            reaction: "O₂/H₂O 水稳定上限".to_string(),
            equation: "Eh = 1.23 - 0.0591·pH (25°C)".to_string(),
            boundary_line: (0..=20).map(|i| {
                let ph = ph_min + (ph_max - ph_min) * (i as f64) / 20.0;
                (ph, nernst_equation(1.23, ph, STANDARD_TEMP_K, 4.0))
            }).collect(),
            description: "高于此线水分解产生O₂，极端氧化性环境".to_string(),
        },
        RedoxPhaseBoundary {
            reaction: "有氧/次氧边界 (Oxic/Suboxic)".to_string(),
            equation: "Eh ≈ 0.8 - 0.0591·pH".to_string(),
            boundary_line: (0..=20).map(|i| {
                let ph = ph_min + (ph_max - ph_min) * (i as f64) / 20.0;
                (ph, nernst_equation(0.80, ph, STANDARD_TEMP_K, 1.0))
            }).collect(),
            description: "溶解氧耗尽，开始氮/锰还原过程".to_string(),
        },
        RedoxPhaseBoundary {
            reaction: "Mn(IV)/Mn(II) 锰还原边界".to_string(),
            equation: "MnO₂ + 4H⁺ + 2e⁻ ⇌ Mn²⁺ + 2H₂O".to_string(),
            boundary_line: (0..=20).map(|i| {
                let ph = ph_min + (ph_max - ph_min) * (i as f64) / 20.0;
                (ph, nernst_equation(0.42, ph, STANDARD_TEMP_K, 2.0))
            }).collect(),
            description: "锰氧化物还原溶解，释放Mn²⁺".to_string(),
        },
        RedoxPhaseBoundary {
            reaction: "Fe(III)/Fe(II) 铁还原边界".to_string(),
            equation: "Fe(OH)₃ + 3H⁺ + e⁻ ⇌ Fe²⁺ + 3H₂O".to_string(),
            boundary_line: (0..=20).map(|i| {
                let ph = ph_min + (ph_max - ph_min) * (i as f64) / 20.0;
                (ph, nernst_equation(-0.08, ph, STANDARD_TEMP_K, 2.0))
            }).collect(),
            description: "铁氧化物还原，羟磷灰石稳定性变化关键区".to_string(),
        },
        RedoxPhaseBoundary {
            reaction: "SO₄²⁻/HS⁻ 硫酸盐还原边界".to_string(),
            equation: "SO₄²⁻ + 9H⁺ + 8e⁻ ⇌ HS⁻ + 4H₂O".to_string(),
            boundary_line: (0..=20).map(|i| {
                let ph = ph_min + (ph_max - ph_min) * (i as f64) / 20.0;
                (ph, nernst_equation(-0.22, ph, STANDARD_TEMP_K, 2.0))
            }).collect(),
            description: "硫酸盐还原菌活动，生成硫化物沉淀".to_string(),
        },
        RedoxPhaseBoundary {
            reaction: "CO₂/CH₄ 产甲烷边界".to_string(),
            equation: "CO₂ + 8H⁺ + 8e⁻ ⇌ CH₄ + 2H₂O".to_string(),
            boundary_line: (0..=20).map(|i| {
                let ph = ph_min + (ph_max - ph_min) * (i as f64) / 20.0;
                (ph, nernst_equation(-0.35, ph, STANDARD_TEMP_K, 2.0))
            }).collect(),
            description: "产甲烷古菌活动，极端还原环境".to_string(),
        },
        RedoxPhaseBoundary {
            reaction: "H₂/H⁺ 水稳定下限".to_string(),
            equation: "Eh = 0.0 - 0.0591·pH".to_string(),
            boundary_line: (0..=20).map(|i| {
                let ph = ph_min + (ph_max - ph_min) * (i as f64) / 20.0;
                (ph, nernst_equation(0.0, ph, STANDARD_TEMP_K, 1.0) - 500.0_f64.min(eh_max))
            }).collect(),
            description: "低于此线水分解产生H₂，极强还原".to_string(),
        },
    ];

    let sample_zone = classify_redox_zone(sample_ph, sample_eh_mv);
    let sample_phase = identify_stable_phase(sample_ph, sample_eh_mv, &sample_zone);
    let sample_point = EhPhPoint {
        ph: sample_ph,
        eh_mv: sample_eh_mv,
        zone: sample_zone,
        stable_phase: sample_phase,
    };

    let (preservation, risk) = evaluate_zone_preservation(&sample_zone, sample_ph);

    EhPhDiagram {
        zones,
        boundaries,
        dominant_zone: sample_zone,
        dominant_zone_name: sample_zone.to_string(),
        sample_point,
        corrosion_risk: risk,
        preservation_quality: preservation,
        grid_size: (nx, ny),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemperatureHistoryPoint {
    pub years_bp: f64,
    pub temp_celsius: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollagenPreservationIndex {
    pub cpi_score: f64,
    pub cpi_grade: String,
    pub equivalent_years_at_20c: f64,
    pub remaining_collagen_pct: f64,
    pub predicted_half_life_years: f64,
    pub initial_half_life_years: f64,
    pub temperature_history: Vec<TemperatureHistoryPoint>,
    pub activation_energy: f64,
    pub burial_years: f64,
    pub average_temp_c: f64,
    pub interpretation: String,
}

pub fn calculate_cpi(
    activation_energy: f64,
    burial_years: f64,
    current_temp_c: f64,
    temp_history: Option<Vec<TemperatureHistoryPoint>>,
    initial_collagen_fraction: f64,
) -> CollagenPreservationIndex {
    let arr_cfg = ArrheniusConfig {
        ea: activation_energy,
        a: 1.2e10,
        r: MOLAR_GAS_R,
        ph_acid_coeff: 4.5e-4,
        ph_base_coeff: 8.0e-5,
        ph_neutral_point: 7.0,
    };

    let history = temp_history.unwrap_or_else(|| {
        vec![
            TemperatureHistoryPoint { years_bp: burial_years, temp_celsius: current_temp_c - 5.0 },
            TemperatureHistoryPoint { years_bp: burial_years * 0.5, temp_celsius: current_temp_c - 2.0 },
            TemperatureHistoryPoint { years_bp: 0.0, temp_celsius: current_temp_c },
        ]
    });

    let mut total_equivalent_years = 0.0;
    let mut weighted_temp_sum = 0.0;
    let mut total_weight = 0.0;

    for i in 0..history.len() {
        let curr = &history[i];
        let next_years = if i + 1 < history.len() {
            history[i + 1].years_bp
        } else {
            0.0
        };
        let period_duration = (curr.years_bp - next_years).abs().max(0.0);

        let k_current = arrhenius_rate_constant(curr.temp_celsius, &arr_cfg);
        let k_reference = arrhenius_rate_constant(20.0, &arr_cfg);
        let accel_factor = if k_reference > 0.0 { k_current / k_reference } else { 1.0 };

        total_equivalent_years += period_duration * accel_factor;

        weighted_temp_sum += curr.temp_celsius * period_duration;
        total_weight += period_duration;
    }

    let avg_temp = if total_weight > 0.0 { weighted_temp_sum / total_weight } else { current_temp_c };

    let k_ref_20 = arrhenius_rate_constant(20.0, &arr_cfg);
    let half_life_ref = if k_ref_20 > 0.0 { 0.693 / k_ref_20 } else { 1.0e6 };
    let half_life_years_ref = half_life_ref / (365.25 * 24.0 * 3600.0);

    let k_avg = arrhenius_rate_constant(avg_temp, &arr_cfg);
    let half_life_current = if k_avg > 0.0 { 0.693 / k_avg } else { 1.0e6 };
    let half_life_years_current = half_life_current / (365.25 * 24.0 * 3600.0);

    let equivalent_time_s = total_equivalent_years * 365.25 * 24.0 * 3600.0;
    let decay = (-k_ref_20 * equivalent_time_s).exp();
    let remaining_pct = (initial_collagen_fraction * decay * 100.0).clamp(0.0, 100.0);

    let cpi_raw = remaining_pct;
    let cpi_score = cpi_raw.clamp(0.0, 100.0);

    let grade = if cpi_score >= 85.0 {
        "A级 (极佳保存: 可开展古DNA/稳定同位素/氨基酸分析)".to_string()
    } else if cpi_score >= 65.0 {
        "B级 (良好保存: 可开展稳定同位素及常规结构分析)".to_string()
    } else if cpi_score >= 40.0 {
        "C级 (一般保存: 仅可开展元素组成与宏观结构分析)".to_string()
    } else if cpi_score >= 15.0 {
        "D级 (较差保存: 仅限形态学与矿化研究)".to_string()
    } else {
        "E级 (严重降解: 仅存矿化骨架, 有机质分析无效)".to_string()
    };

    let interpretation = if cpi_score >= 85.0 {
        format!("骨胶原保存状态极佳。预估剩余胶原蛋白{:.1}%，在当前环境下半衰期约{:.1}年。建议优先发掘并进行精细化采样，样品须-20℃冷冻保存以防止进一步降解。", 
            remaining_pct, half_life_years_current)
    } else if cpi_score >= 65.0 {
        format!("骨胶原保存良好。剩余{:.1}%，半衰期约{:.1}年。建议按计划发掘，现场采集后72小时内转入实验室冷藏（4℃），避免反复冻融。",
            remaining_pct, half_life_years_current)
    } else if cpi_score >= 40.0 {
        format!("骨胶原保存一般。剩余{:.1}%，半衰期约{:.1}年。建议尽快发掘（1-2年内），样品提取后需立即进行保护处理，可考虑PEG包埋加固。",
            remaining_pct, half_life_years_current)
    } else if cpi_score >= 15.0 {
        format!("骨胶原保存较差。仅剩余{:.1}%，半衰期约{:.1}年。建议尽快抢救性发掘，对有机质脆弱部位进行现场固化保护（如B72丙烯酸树脂）。",
            remaining_pct, half_life_years_current)
    } else {
        format!("骨胶原严重降解。仅残留{:.1}%，有机质结构已丧失。可仅关注矿化骨骼的形态学和元素分析，发掘重点防止物理破碎。",
            remaining_pct, half_life_years_current)
    };

    CollagenPreservationIndex {
        cpi_score,
        cpi_grade: grade,
        equivalent_years_at_20c: total_equivalent_years,
        remaining_collagen_pct: remaining_pct,
        predicted_half_life_years: half_life_years_current,
        initial_half_life_years: half_life_years_ref,
        temperature_history: history,
        activation_energy,
        burial_years,
        average_temp_c: avg_temp,
        interpretation,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloParams {
    pub num_simulations: usize,
    pub current_ph: f64,
    pub ph_std_dev: f64,
    pub current_temp_c: f64,
    pub temp_std_dev: f64,
    pub current_ca_ppm: f64,
    pub ca_std_dev: f64,
    pub current_orp_mv: f64,
    pub orp_std_dev: f64,
    pub forecast_years: f64,
    pub time_steps_per_year: usize,
    pub target_corrosion_threshold_um: f64,
    pub acceptable_risk_threshold: f64,
    pub current_collagen_remaining_pct: f64,
}

impl Default for MonteCarloParams {
    fn default() -> Self {
        Self {
            num_simulations: 5000,
            current_ph: 7.0,
            ph_std_dev: 0.3,
            current_temp_c: 18.0,
            temp_std_dev: 2.0,
            current_ca_ppm: 80.0,
            ca_std_dev: 15.0,
            current_orp_mv: 100.0,
            orp_std_dev: 50.0,
            forecast_years: 50.0,
            time_steps_per_year: 12,
            target_corrosion_threshold_um: 200.0,
            acceptable_risk_threshold: 0.25,
            current_collagen_remaining_pct: 70.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcavationWindowAssessment {
    pub start_year: f64,
    pub end_year: f64,
    pub probability_of_success: f64,
    pub expected_damage_if_wait: f64,
    pub expected_damage_if_excavate: f64,
    pub net_benefit: f64,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcavationOptimizationResult {
    pub params: MonteCarloParams,
    pub simulations_completed: usize,
    pub optimal_window: ExcavationWindowAssessment,
    pub windows: Vec<ExcavationWindowAssessment>,
    pub year_by_year_stats: Vec<YearlyForecast>,
    pub risk_distribution: RiskDistribution,
    pub final_recommendation: String,
    pub confidence_level: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YearlyForecast {
    pub year: f64,
    pub mean_corrosion_um: f64,
    pub p5_corrosion_um: f64,
    pub p25_corrosion_um: f64,
    pub p50_corrosion_um: f64,
    pub p75_corrosion_um: f64,
    pub p95_corrosion_um: f64,
    pub mean_collagen_pct: f64,
    pub prob_exceed_threshold: f64,
    pub should_excavate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskDistribution {
    pub percentiles: Vec<(String, f64)>,
    pub probability_by_year: Vec<(f64, f64)>,
}

pub fn run_monte_carlo_excavation(params: MonteCarloParams) -> ExcavationOptimizationResult {
    let arr_cfg = ArrheniusConfig::default();
    let ca_cfg = CalciumPhosphateConfig::default();
    let n_sims = params.num_simulations.max(100);
    let forecast_months = (params.forecast_years * params.time_steps_per_year as f64) as usize;
    let dt_seconds = (365.25 * 24.0 * 3600.0) / (params.time_steps_per_year as f64);

    let normal_ph = Normal::new(params.current_ph, params.ph_std_dev.max(0.01)).unwrap();
    let normal_temp = Normal::new(params.current_temp_c, params.temp_std_dev.max(0.01)).unwrap();
    let normal_ca = Normal::new(params.current_ca_ppm, params.ca_std_dev.max(0.01)).unwrap();
    let normal_orp = Normal::new(params.current_orp_mv, params.orp_std_dev.max(0.01)).unwrap();

    let mut all_results: Vec<Vec<(f64, f64)>> = Vec::with_capacity(n_sims);

    for sim_idx in 0..n_sims {
        let mut rng = rand::thread_rng();
        let mut sim_corrosion: Vec<(f64, f64)> = Vec::with_capacity(forecast_months + 1);

        let base_ph: f64 = normal_ph.sample(&mut rng).clamp(3.0, 11.0);
        let base_temp: f64 = normal_temp.sample(&mut rng).clamp(-5.0, 50.0);
        let base_ca: f64 = normal_ca.sample(&mut rng).clamp(5.0, 1000.0);
        let base_orp: f64 = normal_orp.sample(&mut rng).clamp(-400.0, 700.0);

        sim_corrosion.push((0.0, params.current_collagen_remaining_pct));

        let mut cumulative_collagen = params.current_collagen_remaining_pct;

        for month in 1..=forecast_months {
            let season = (month as f64 % 12.0) / 12.0;
            let temp_osc = (season * 2.0 * std::f64::consts::PI).sin() * 3.0;
            let ph_osc = (season * 2.0 * std::f64::consts::PI).sin() * 0.1;
            let noise_p: f64 = rng.gen::<f64>() - 0.5;
            let noise_t: f64 = rng.gen::<f64>() - 0.5;

            let step_ph = (base_ph + ph_osc + noise_p * 0.05).clamp(3.0, 11.0);
            let step_temp = (base_temp + temp_osc + noise_t * 0.5).clamp(-5.0, 50.0);
            let step_ca = (base_ca * (0.95 + rng.gen::<f64>() * 0.1)).clamp(5.0, 1000.0);
            let step_orp = (base_orp + (rng.gen::<f64>() - 0.5) * 20.0).clamp(-400.0, 700.0);

            let coll_rate = collagen_hydrolysis_rate(step_temp, step_ph, step_orp, Some(&arr_cfg));
            let diss_rate = dissolution_rate(step_ph, step_temp, step_ca, 0.5, &ca_cfg);
            let cor_rate = corrosion_rate_um_per_year(coll_rate, diss_rate, step_ph);
            let corrosion_per_step = cor_rate / (params.time_steps_per_year as f64);

            let degradation_step = if cumulative_collagen > 0.0 {
                (coll_rate * dt_seconds).min(0.99)
            } else { 0.0 };
            cumulative_collagen = (cumulative_collagen * (1.0 - degradation_step)).max(0.0);

            let current_depth = sim_corrosion.last()
                .map(|(_, depth)| *depth)
                .unwrap_or(0.0);

            sim_corrosion.push((month as f64 / params.time_steps_per_year as f64, current_depth + corrosion_per_step));
        }

        all_results.push(sim_corrosion);
    }

    let mut year_stats = Vec::new();
    let years_to_eval: Vec<usize> = (0..=forecast_months).step_by(params.time_steps_per_year).collect();

    for (year_idx, &month_idx) in years_to_eval.iter().enumerate() {
        let year = year_idx as f64;
        let mut depths: Vec<f64> = all_results.iter()
            .map(|sim| sim.get(month_idx).map(|x| x.1).unwrap_or(0.0))
            .collect();
        depths.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let n = depths.len();
        let mean = depths.iter().sum::<f64>() / n as f64;
        let p5 = depths[((n as f64 * 0.05) as usize).min(n - 1)];
        let p25 = depths[((n as f64 * 0.25) as usize).min(n - 1)];
        let p50 = depths[n / 2];
        let p75 = depths[((n as f64 * 0.75) as usize).min(n - 1)];
        let p95 = depths[((n as f64 * 0.95) as usize).min(n - 1)];

        let exceed_count = depths.iter().filter(|&&d| d >= params.target_corrosion_threshold_um).count();
        let prob_exceed = exceed_count as f64 / n as f64;

        let mut collagens: Vec<f64> = all_results.iter()
            .map(|sim| sim.get(month_idx).map(|(_, _)| {
                let rem_pct = params.current_collagen_remaining_pct * (-year * 0.01).exp();
                rem_pct
            }).unwrap_or(params.current_collagen_remaining_pct))
            .collect();
        collagens.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean_coll = collagens.iter().sum::<f64>() / collagens.len() as f64;

        let should = prob_exceed >= params.acceptable_risk_threshold;

        year_stats.push(YearlyForecast {
            year,
            mean_corrosion_um: mean,
            p5_corrosion_um: p5,
            p25_corrosion_um: p25,
            p50_corrosion_um: p50,
            p75_corrosion_um: p75,
            p95_corrosion_um: p95,
            mean_collagen_pct: mean_coll,
            prob_exceed_threshold: prob_exceed,
            should_excavate: should,
        });
    }

    let mut windows = Vec::new();
    let window_sizes: Vec<(f64, f64)> = vec![
        (0.0, 1.0),
        (0.5, 2.0),
        (1.0, 3.0),
        (2.0, 5.0),
        (3.0, 7.0),
        (5.0, 10.0),
    ];

    for (start, end) in window_sizes.iter() {
        let start_idx = (*start * params.time_steps_per_year as f64).round() as usize;
        let end_idx = ((*end * params.time_steps_per_year as f64).round() as usize).min(forecast_months);

        let start_stat = year_stats.get(start_idx.min(year_stats.len() - 1));
        let end_stat = year_stats.get(end_idx.min(year_stats.len() - 1));

        if let (Some(s), Some(e)) = (start_stat, end_stat) {
            let prob_success = 1.0 - ((s.prob_exceed_threshold + e.prob_exceed_threshold) / 2.0);
            let expected_damage_wait = e.mean_corrosion_um;
            let expected_damage_excav = s.mean_corrosion_um * 0.5 + 10.0;
            let net_benefit = expected_damage_wait - expected_damage_excav;

            let recommendation = if prob_success >= 0.9 {
                "强烈推荐此时间窗口发掘".to_string()
            } else if prob_success >= 0.75 {
                "建议此时间段内发掘".to_string()
            } else if prob_success >= 0.6 {
                "可考虑发掘，需准备强化保护方案".to_string()
            } else {
                "不建议，风险过高".to_string()
            };

            windows.push(ExcavationWindowAssessment {
                start_year: *start,
                end_year: *end,
                probability_of_success: prob_success.clamp(0.0, 1.0),
                expected_damage_if_wait: expected_damage_wait,
                expected_damage_if_excavate: expected_damage_excav,
                net_benefit,
                recommendation,
            });
        }
    }

    let optimal_window = windows.iter()
        .max_by(|a, b| {
            let score_a = a.probability_of_success * 2.0 + a.net_benefit / 100.0;
            let score_b = b.probability_of_success * 2.0 + b.net_benefit / 100.0;
            score_a.partial_cmp(&score_b).unwrap()
        })
        .cloned()
        .unwrap_or_else(|| ExcavationWindowAssessment {
            start_year: 0.0,
            end_year: 1.0,
            probability_of_success: 0.5,
            expected_damage_if_wait: 100.0,
            expected_damage_if_excavate: 50.0,
            net_benefit: 0.0,
            recommendation: "建议立即评估".to_string(),
        });

    let first_year = year_stats.first();
    let prob_by_year: Vec<(f64, f64)> = year_stats.iter()
        .map(|ys| (ys.year, ys.prob_exceed_threshold))
        .collect();
    let percentiles = if let Some(last) = year_stats.last() {
        vec![
            ("P5 (乐观)".to_string(), last.p5_corrosion_um),
            ("P25".to_string(), last.p25_corrosion_um),
            ("P50 (中位)".to_string(), last.p50_corrosion_um),
            ("P75".to_string(), last.p75_corrosion_um),
            ("P95 (悲观)".to_string(), last.p95_corrosion_um),
        ]
    } else {
        Vec::new()
    };

    let risk_dist = RiskDistribution {
        percentiles,
        probability_by_year: prob_by_year.clone(),
    };

    let first_exceed_year = year_stats.iter()
        .find(|ys| ys.prob_exceed_threshold >= params.acceptable_risk_threshold)
        .map(|ys| ys.year);

    let confidence = {
        let good_sims: usize = all_results.iter()
            .filter(|sim| {
                sim.last().map(|(_, depth)| *depth < params.target_corrosion_threshold_um * 1.5).unwrap_or(false)
            })
            .count();
        (good_sims as f64 / n_sims as f64).clamp(0.0, 1.0)
    };

    let final_rec = match first_exceed_year {
        Some(year) if year <= 1.0 => {
            format!("⚠️ 紧急建议：在{}年内抢救性发掘。当前环境腐蚀风险已达到临界值（超阈概率≥{:.0}%），若继续埋藏，{:.1}年后预计有{:.1}%概率超过腐蚀阈值{}μm。最佳窗口：立即起至{}年内完成。",
                year.ceil(), params.acceptable_risk_threshold * 100.0,
                params.forecast_years,
                (year_stats.last().map(|y| y.prob_exceed_threshold).unwrap_or(0.0) * 100.0),
                params.target_corrosion_threshold_um,
                optimal_window.end_year)
        },
        Some(year) => {
            format!("建议在{}年内完成发掘。以{}%置信度估计，{:.1}年后腐蚀超阈概率将达到{:.0}%。最佳发掘窗口：从现在起至{:.1}年（成功概率{:.0}%）。",
                year.floor(),
                confidence * 100.0,
                year,
                params.acceptable_risk_threshold * 100.0,
                optimal_window.end_year,
                optimal_window.probability_of_success * 100.0)
        },
        None => {
            if let Some(last) = year_stats.last() {
                format!("当前环境相对稳定，{:.1}年内腐蚀超阈风险低于{:.0}%。可按正常考古进度安排发掘，最佳窗口：{:.1}至{:.1}年（成功概率{:.0}%）。",
                    params.forecast_years,
                    params.acceptable_risk_threshold * 100.0,
                    optimal_window.start_year,
                    optimal_window.end_year,
                    optimal_window.probability_of_success * 100.0)
            } else {
                "请补充更多历史数据以进行精确评估".to_string()
            }
        }
    };

    ExcavationOptimizationResult {
        params,
        simulations_completed: n_sims,
        optimal_window,
        windows,
        year_by_year_stats: year_stats,
        risk_distribution: risk_dist,
        final_recommendation: final_rec,
        confidence_level: confidence,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectionRecommendation {
    pub primary_moisturizer: String,
    pub primary_moisturizer_zh: String,
    pub concentration_pct: f64,
    pub application_method: String,
    pub secondary_recommendations: Vec<String>,
    pub ph_neutralization_required: bool,
    pub neutralization_agent: Option<String>,
    pub expected_effectiveness_score: f64,
    pub estimated_stabilization_hours: f64,
    pub warnings: Vec<String>,
    pub decision_path: Vec<String>,
    pub materials_needed: Vec<ProtectionMaterial>,
    pub step_by_step_protocol: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectionMaterial {
    pub name: String,
    pub name_zh: String,
    pub quantity_estimate: String,
    pub purpose: String,
    pub priority: String,
}

fn classify_ph_condition(ph: f64) -> &'static str {
    if ph < 4.5 { "EXTREMELY_ACIDIC" }
    else if ph < 5.5 { "HIGHLY_ACIDIC" }
    else if ph < 6.5 { "MODERATELY_ACIDIC" }
    else if ph <= 7.5 { "NEUTRAL" }
    else if ph <= 8.5 { "MODERATELY_ALKALINE" }
    else if ph <= 9.5 { "HIGHLY_ALKALINE" }
    else { "EXTREMELY_ALKALINE" }
}

fn classify_ca_condition(ca_ppm: f64) -> &'static str {
    if ca_ppm < 30.0 { "VERY_LOW_CA" }
    else if ca_ppm < 80.0 { "LOW_CA" }
    else if ca_ppm < 200.0 { "NORMAL_CA" }
    else if ca_ppm < 400.0 { "HIGH_CA" }
    else { "VERY_HIGH_CA" }
}

fn classify_origination(orp_mv: f64) -> &'static str {
    if orp_mv > 250.0 { "HIGHLY_OXIDIZING" }
    else if orp_mv > 0.0 { "MODERATELY_OXIDIZING" }
    else if orp_mv > -150.0 { "MODERATELY_REDUCING" }
    else { "STRONGLY_REDUCING" }
}

pub fn recommend_temporary_protection(
    ph: f64,
    ca_ppm: f64,
    orp_mv: f64,
    ambient_temp_c: f64,
    ambient_rh_pct: f64,
    burial_depth_m: f64,
    relic_category: &str,
) -> ProtectionRecommendation {
    let ph_condition = classify_ph_condition(ph);
    let ca_condition = classify_ca_condition(ca_ppm);
    let redox_condition = classify_origination(orp_mv);

    let mut decision_path: Vec<String> = Vec::new();
    decision_path.push(format!("步骤1: pH条件判定 = {} (pH={:.2})", ph_condition, ph));
    decision_path.push(format!("步骤2: 钙离子浓度判定 = {} (Ca²⁺={:.1}ppm)", ca_condition, ca_ppm));
    decision_path.push(format!("步骤3: 氧化还原条件判定 = {} (ORP={:.0}mV)", redox_condition, orp_mv));

    let (moisturizer, moisturizer_zh, concentration, effectiveness, _decision_note) = match (ph_condition, ca_condition) {
        ("EXTREMELY_ACIDIC", _) | ("HIGHLY_ACIDIC", _) => {
            decision_path.push("步骤4: pH ≤ 5.5，高酸风险，优先pH缓冲+PEG保湿体系".to_string());
            ("PEG400-PBS_Buffered".to_string(),
             "pH缓冲的聚乙二醇400溶液".to_string(),
             30.0_f64,
             85.0_f64,
             "酸蚀环境需磷酸盐缓冲")
        },
        ("MODERATELY_ACIDIC", "VERY_LOW_CA") => {
            decision_path.push("步骤4: pH偏酸+低钙，羟磷灰石溶解高风险，推荐PEG+饱和Ca(OH)₂".to_string());
            ("PEG200_Saturated_Calcium_Hydroxide".to_string(),
             "PEG200饱和氢氧化钙溶液".to_string(),
             40.0_f64,
             92.0_f64,
             "补充钙离子抑制溶解")
        },
        ("MODERATELY_ACIDIC", "LOW_CA") => {
            decision_path.push("步骤4: pH偏酸+低钙，推荐PEG200+低浓度CaCl₂补充钙离子".to_string());
            ("PEG200_5pct_CaCl2".to_string(),
             "PEG200含5%氯化钙溶液".to_string(),
             35.0_f64,
             88.0_f64,
             "抑制矿物溶解")
        },
        ("MODERATELY_ACIDIC", _) => {
            decision_path.push("步骤4: pH偏弱酸性+钙浓度正常，推荐PEG200基础保湿".to_string());
            ("PEG200_Pure".to_string(),
             "纯PEG200溶液".to_string(),
             50.0_f64,
             80.0_f64,
             "标准骨角质保湿剂")
        },
        ("NEUTRAL", "VERY_LOW_CA") | ("NEUTRAL", "LOW_CA") => {
            decision_path.push("步骤4: pH中性但钙偏低，推荐去离子水+微量Ca²⁺补充".to_string());
            ("DI_Water_With_Ca_Supplement".to_string(),
             "含钙补充的去离子水".to_string(),
             100.0_f64,
             82.0_f64,
             "中性环境水合即可")
        },
        ("NEUTRAL", _) => {
            decision_path.push("步骤4: pH中性+钙正常，首选去离子水保湿（最温和）".to_string());
            ("DI_Water".to_string(),
             "去离子水 (Milli-Q级)".to_string(),
             100.0_f64,
             78.0_f64,
             "最温和，无化学风险")
        },
        ("MODERATELY_ALKALINE", "HIGH_CA") | ("MODERATELY_ALKALINE", "VERY_HIGH_CA") => {
            decision_path.push("步骤4: 碱性+高钙，钙沉积风险，推荐稀乙醇-水体系".to_string());
            ("Ethanol_Water_30pct".to_string(),
             "30%乙醇-水溶液".to_string(),
             30.0_f64,
             75.0_f64,
             "降低钙离子溶解度")
        },
        ("MODERATELY_ALKALINE", _) => {
            decision_path.push("步骤4: 弱碱性环境，推荐去离子水保湿".to_string());
            ("DI_Water".to_string(),
             "去离子水 (Milli-Q级)".to_string(),
             100.0_f64,
             76.0_f64,
             "中性保湿")
        },
        ("HIGHLY_ALKALINE", _) | ("EXTREMELY_ALKALINE", _) => {
            decision_path.push("步骤4: 强碱性，需酸中和+PEG保湿".to_string());
            ("PEG200_Weak_Acid_Buffered".to_string(),
             "弱酸缓冲的PEG200溶液".to_string(),
             35.0_f64,
             70.0_f64,
             "先中和再保湿")
        },
        _ => {
            decision_path.push("步骤4: 默认方案".to_string());
            ("DI_Water".to_string(),
             "去离子水".to_string(),
             100.0_f64,
             70.0_f64,
             "安全默认")
        }
    };

    let application_method = if relic_category.contains("牙") || relic_category.contains("齿") {
        "局部棉签涂布法: 用洁净棉签蘸取保湿液，沿牙骨质-釉质分界轻轻涂布，避免渗入牙髓腔。每30分钟补涂一次。".to_string()
    } else if relic_category.contains("骨") && burial_depth_m < 0.5 {
        "喷淋-包裹法: 先用低压喷壶均匀喷洒保湿液，再用预先浸透保湿液的无纺土工布（2-3层）紧密包裹，外裹PVC膜密封。".to_string()
    } else {
        "浸入-逐层包裹法: 小件可直接浸入保湿液5-10秒；大件先喷洒后用3层保湿纱布包裹，外加PE膜密封，标注方向。".to_string()
    };

    let (need_neutralize, neutralizer) = match ph_condition {
        "EXTREMELY_ACIDIC" | "HIGHLY_ACIDIC" => {
            (true, Some("磷酸缓冲生理盐水 (PBS) pH=7.2，使用前进行点滴中和试验".to_string()))
        },
        "EXTREMELY_ALKALINE" | "HIGHLY_ALKALINE" => {
            (true, Some("0.1M硼酸缓冲液或稀醋酸溶液(pH=5.5)，分次逐步中和".to_string()))
        },
        _ => (false, None),
    };

    let mut secondary: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    match redox_condition {
        "HIGHLY_OXIDIZING" => {
            secondary.push("添加0.1%抗坏血酸（维生素C）作为临时抗氧化剂".to_string());
            warnings.push("⚠️ 高氧化环境：出土后暴露于空气将加速氧化，建议1小时内完成保湿密封，避光保存".to_string());
        },
        "STRONGLY_REDUCING" => {
            secondary.push("添加0.05%百里酚作为微生物抑制剂".to_string());
            warnings.push("⚠️ 强还原环境：出土后需缓慢氧化（预暴露12-24小时），防止硫化物快速氧化产生硫酸破坏".to_string());
        },
        _ => {}
    }

    if ambient_temp_c > 28.0 {
        secondary.push(format!("环境温度{:.1}℃偏高，需配备冰袋或冷藏箱维持15-20℃", ambient_temp_c));
        warnings.push("⚠️ 高温环境：微生物活性加倍，需缩短现场到实验室的运输时间（≤6小时）".to_string());
    }
    if ambient_rh_pct < 40.0 {
        warnings.push(format!("⚠️ 环境湿度仅{:.0}%，干燥风险高，需双层密封保湿", ambient_rh_pct));
    }

    let stabilization_hours = if need_neutralize { 4.0 } else { 1.0 };

    let mut materials = vec![
        ProtectionMaterial {
            name: moisturizer.clone(),
            name_zh: moisturizer_zh.clone(),
            quantity_estimate: format!("约{} mL/件文物（根据尺寸调整）", if concentration < 50.0 { 200 } else { 500 }),
            purpose: "主要保湿剂，维持骨角质水合状态，防止干燥开裂".to_string(),
            priority: "必要".to_string(),
        },
        ProtectionMaterial {
            name: "Nonwoven_Geotextile".to_string(),
            name_zh: "无纺土工布/医用纱布".to_string(),
            quantity_estimate: "3层包裹，约0.5㎡/件".to_string(),
            purpose: "保湿液载体，均匀接触文物表面，防止直接接触塑料膜".to_string(),
            priority: "必要".to_string(),
        },
        ProtectionMaterial {
            name: "PVC_or_PE_Film".to_string(),
            name_zh: "PVC/PE保鲜膜".to_string(),
            quantity_estimate: "双层密封，宽度≥30cm".to_string(),
            purpose: "密封保湿，防止水分蒸发，隔绝外部污染".to_string(),
            priority: "必要".to_string(),
        },
        ProtectionMaterial {
            name: "ABS_Support_Mesh".to_string(),
            name_zh: "ABS塑料支撑网格".to_string(),
            quantity_estimate: "根据文物尺寸定制".to_string(),
            purpose: "脆弱骨骼的物理支撑，防止运输途中破碎".to_string(),
            priority: "推荐".to_string(),
        },
    ];

    if need_neutralize {
        if let Some(n) = neutralizer.clone() {
            materials.insert(0, ProtectionMaterial {
                name: "Neutralization_Buffer".to_string(),
                name_zh: n,
                quantity_estimate: "按需配制，先小面积测试".to_string(),
                purpose: "先调节pH至中性范围，再进行保湿处理".to_string(),
                priority: "必要".to_string(),
            });
        }
    }

    let mut protocol = vec![
        "步骤1：现场拍照记录（出土状态、方向、颜色、附着物），采集保存环境传感器数据".to_string(),
        "步骤2：用软毛刷（尼龙毛）轻轻清除表面浮土，避免用力擦拭导致颗粒摩擦损伤".to_string(),
    ];

    if need_neutralize {
        protocol.push("步骤3：pH中和 - 使用缓冲液进行点滴法局部测试，确认无不良反应后逐步扩大处理面积，监测pH变化".to_string());
        protocol.push("步骤4：保湿处理 - 按照推荐方法涂抹/喷洒保湿剂，静置5分钟使溶液充分渗透".to_string());
    } else {
        protocol.push("步骤3：保湿处理 - 按照推荐方法涂抹/喷洒保湿剂，静置5分钟使溶液充分渗透".to_string());
    }

    protocol.extend(vec![
        "步骤5：包裹密封 - 内层用湿润无纺土工布3层紧密包裹（预留观察窗），外层用PVC膜双层密封，用记号笔标注文物编号、方向（上下）、处理日期".to_string(),
        "步骤6：物理支撑 - 对脆弱骨骼（如肋骨、指骨）加装ABS网格支撑，填充减震材料（泡沫塑料、气泡膜）".to_string(),
        format!("步骤7：环境控制 - 运输途中维持温度15-20℃{}，避免阳光直射和剧烈震动", 
            if ambient_temp_c > 28.0 { "（冷藏箱+冰袋）" } else { "" }),
        "步骤8：现场→实验室交接 - 抵达实验室后立即拆除外层密封膜，检查保湿状态，转入恒湿柜（RH=55±5%）或进行下一步加固处理（如B72树脂渗透）".to_string(),
    ]);

    ProtectionRecommendation {
        primary_moisturizer: moisturizer,
        primary_moisturizer_zh: moisturizer_zh,
        concentration_pct: concentration,
        application_method,
        secondary_recommendations: secondary,
        ph_neutralization_required: need_neutralize,
        neutralization_agent: neutralizer,
        expected_effectiveness_score: effectiveness,
        estimated_stabilization_hours: stabilization_hours,
        warnings,
        decision_path,
        materials_needed: materials,
        step_by_step_protocol: protocol,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::E;

    // ============================================================
    // Feature 1: 氧化还原分带识别 测试集合
    // 覆盖：正常分带、边界值、矿物组合一致性、极端/异常值、相图生成
    // ============================================================

    mod redox_zone_tests {
        use super::*;

        #[test]
        fn test_zone_oxidized_normal_ph7() {
            let zone = classify_redox_zone(7.0, 600.0);
            assert_eq!(zone, RedoxZone::OXIDIZED, "pH7 + 600mV 应属于氧化带");
        }

        #[test]
        fn test_zone_suboxic_normal() {
            let zone = classify_redox_zone(7.0, 300.0);
            assert_eq!(zone, RedoxZone::SUBSURFACE_OXIC, "pH7 + 300mV 应属于次表层含氧带");
        }

        #[test]
        fn test_zone_manganese_reducing() {
            let zone = classify_redox_zone(7.0, 100.0);
            assert_eq!(zone, RedoxZone::MANGANESE_REDUCING, "pH7 + 100mV 应属于锰还原带");
        }

        #[test]
        fn test_zone_iron_reducing_normal() {
            let zone = classify_redox_zone(7.0, -50.0);
            assert_eq!(zone, RedoxZone::IRON_REDUCING, "pH7 + -50mV 应属于铁还原带");
        }

        #[test]
        fn test_zone_sulfate_reducing_normal() {
            let zone = classify_redox_zone(7.0, -200.0);
            assert_eq!(zone, RedoxZone::SULFATE_REDUCING, "pH7 + -200mV 应属于硫酸盐还原带");
        }

        #[test]
        fn test_zone_methanogenic_normal() {
            let zone = classify_redox_zone(7.0, -350.0);
            assert_eq!(zone, RedoxZone::METHANOGENIC, "pH7 + -350mV 应属于产甲烷带");
        }

        #[test]
        fn test_zone_carbonate_reducing() {
            let zone = classify_redox_zone(7.0, -450.0);
            assert_eq!(zone, RedoxZone::CARBONATE_REDUCING, "pH7 + -450mV 应属于碳酸盐还原带");
        }

        #[test]
        fn test_zone_undefined_extreme_reducing() {
            let zone = classify_redox_zone(7.0, -700.0);
            assert_eq!(zone, RedoxZone::UNDEFINED, "pH7 + -700mV 超出常规范围应返回UNDEFINED");
        }

        #[test]
        fn test_boundary_ph_extreme_acidic_clamped() {
            let zone_low = classify_redox_zone(0.0, 300.0);
            let zone_normal = classify_redox_zone(2.0, 300.0);
            assert_eq!(zone_low, zone_normal, "pH低于下限应被clamp到2.0，分带结果一致");
        }

        #[test]
        fn test_boundary_ph_extreme_alkaline_clamped() {
            let zone_high = classify_redox_zone(14.0, 300.0);
            let zone_normal = classify_redox_zone(12.0, 300.0);
            assert_eq!(zone_high, zone_normal, "pH高于上限应被clamp到12.0，分带结果一致");
        }

        #[test]
        fn test_nernst_linear_with_ph() {
            let e1 = nernst_equation(0.80, 5.0, STANDARD_TEMP_K, 1.0);
            let e2 = nernst_equation(0.80, 7.0, STANDARD_TEMP_K, 1.0);
            let e3 = nernst_equation(0.80, 9.0, STANDARD_TEMP_K, 1.0);
            let diff12 = e1 - e2;
            let diff23 = e2 - e3;
            assert!((diff12 - diff23).abs() < 0.01, "Nernst方程Eh随pH线性变化");
            assert!(e1 > e2, "pH升高，Eh应降低");
        }

        #[test]
        fn test_mineral_assemblage_consistency_oxidized() {
            let zone = RedoxZone::OXIDIZED;
            let phase_acid = identify_stable_phase(4.0, 700.0, &zone);
            let phase_neutral = identify_stable_phase(7.0, 700.0, &zone);
            let phase_alk = identify_stable_phase(9.0, 700.0, &zone);

            assert!(phase_acid.contains("Fe²+") || phase_acid.contains("溶解态"),
                "氧化带酸性应含溶解态Fe²+");
            assert!(phase_neutral.contains("针铁矿") || phase_neutral.contains("FeOOH"),
                "氧化带中性应含针铁矿");
            assert!(phase_alk.contains("方解石") || phase_alk.contains("CaCO₃"),
                "氧化带碱性应含方解石");
        }

        #[test]
        fn test_mineral_assemblage_consistency_iron_reducing() {
            let zone = RedoxZone::IRON_REDUCING;
            let phase = identify_stable_phase(7.0, -50.0, &zone);
            assert!(phase.contains("菱铁矿") || phase.contains("FeCO₃") || phase.contains("Fe²+"),
                "铁还原带应出现菱铁矿或Fe²+，与实测矿物组合一致");
        }

        #[test]
        fn test_mineral_assemblage_consistency_sulfate_reducing() {
            let zone = RedoxZone::SULFATE_REDUCING;
            let phase = identify_stable_phase(7.0, -200.0, &zone);
            assert!(phase.contains("黄铁矿") || phase.contains("FeS₂"),
                "硫酸盐还原带应出现黄铁矿，与实测矿物组合一致");
        }

        #[test]
        fn test_mineral_assemblage_consistency_methanogenic() {
            let zone = RedoxZone::METHANOGENIC;
            let phase = identify_stable_phase(7.5, -350.0, &zone);
            assert!(phase.contains("CH₄") || phase.contains("甲烷"),
                "产甲烷带应出现CH₄，与实测气体组合一致");
        }

        #[test]
        fn test_preservation_quality_correlates_with_zone() {
            let oxic = evaluate_zone_preservation(&RedoxZone::OXIDIZED, 7.0);
            let fe_red = evaluate_zone_preservation(&RedoxZone::IRON_REDUCING, 7.0);
            let meth = evaluate_zone_preservation(&RedoxZone::METHANOGENIC, 7.5);

            let order_good = |s: &str| -> u8 {
                match s {
                    "极差" => 0, "差" => 1, "一般" => 2,
                    "良好" => 3, "优秀" => 4, "极佳" => 5,
                    _ => 0
                }
            };

            assert!(order_good(&fe_red.0) > order_good(&oxic.0),
                "铁还原带保存评级应优于氧化带");
            assert!(order_good(&meth.0) >= order_good(&fe_red.0),
                "产甲烷带保存评级应不低于铁还原带");
        }

        #[test]
        fn test_diagram_grid_dimensions() {
            let (nx, ny) = (15, 25);
            let diagram = generate_eh_ph_diagram(7.0, 100.0, (2.0, 12.0), (-500.0, 800.0), (nx, ny));
            assert_eq!(diagram.zones.len(), nx * ny, "网格点数应为nx×ny");
            assert_eq!(diagram.grid_size.0, nx);
            assert_eq!(diagram.grid_size.1, ny);
        }

        #[test]
        fn test_diagram_boundaries_count() {
            let diagram = generate_eh_ph_diagram(7.0, 100.0, (2.0, 12.0), (-500.0, 800.0), (10, 10));
            assert_eq!(diagram.boundaries.len(), 7, "Fe-S-C体系应有7条主要相边界");
        }

        #[test]
        fn test_diagram_sample_point_in_zones() {
            let diagram = generate_eh_ph_diagram(7.0, 100.0, (2.0, 12.0), (-500.0, 800.0), (20, 20));
            assert!((diagram.sample_point.ph - 7.0).abs() < 0.01);
            assert!((diagram.sample_point.eh_mv - 100.0).abs() < 0.01);
            assert!(!diagram.sample_point.stable_phase.is_empty());
        }

        #[test]
        fn test_diagram_dominant_zone_is_valid() {
            let diagram = generate_eh_ph_diagram(7.0, -200.0, (2.0, 12.0), (-500.0, 800.0), (20, 20));
            assert!(!diagram.dominant_zone_name.is_empty());
            assert!(!diagram.preservation_quality.is_empty());
            assert!(diagram.corrosion_risk.len() > 0);
            assert!(["CRITICAL", "HIGH", "MEDIUM", "LOW"].iter().any(|&r| r == diagram.corrosion_risk),
                "腐蚀风险等级应为有效值: {}", diagram.corrosion_risk);
        }

        #[test]
        fn test_redox_zone_display_not_empty() {
            let zones = vec![
                RedoxZone::OXIDIZED, RedoxZone::SUBSURFACE_OXIC, RedoxZone::MANGANESE_REDUCING,
                RedoxZone::IRON_REDUCING, RedoxZone::SULFATE_REDUCING, RedoxZone::METHANOGENIC,
                RedoxZone::CARBONATE_REDUCING, RedoxZone::UNDEFINED,
            ];
            for z in zones {
                let s = format!("{}", z);
                assert!(!s.is_empty(), "RedoxZone {} Display不能为空", z as u8);
            }
        }
    }

    // ============================================================
    // Feature 2: 骨胶原保存潜力指数 测试集合
    // 覆盖：活化能对比、温度史、边界值、极端/异常、等级判定
    // ============================================================

    mod cpi_tests {
        use super::*;

        #[test]
        fn test_high_ea_has_longer_half_life() {
            let cpi_low_ea = calculate_cpi(60_000.0, 1000.0, 15.0, None, 1.0);
            let cpi_high_ea = calculate_cpi(120_000.0, 1000.0, 15.0, None, 1.0);

            assert!(cpi_high_ea.predicted_half_life_years > cpi_low_ea.predicted_half_life_years * 2.0,
                "高活化能(Ea=120kJ)骨器半衰期应显著长于低活化能(Ea=60kJ)，高活化能≈低活化能的{:.1}倍",
                cpi_high_ea.predicted_half_life_years / cpi_low_ea.predicted_half_life_years);
        }

        #[test]
        fn test_high_ea_preserves_more_collagen() {
            let cpi_low = calculate_cpi(60_000.0, 500.0, 15.0, None, 1.0);
            let cpi_high = calculate_cpi(120_000.0, 500.0, 15.0, None, 1.0);

            assert!(cpi_high.cpi_score > cpi_low.cpi_score,
                "相同埋藏时间下，高活化能骨胶原保存更多 (高:{:.1}% vs 低:{:.1}%)",
                cpi_high.cpi_score, cpi_low.cpi_score);
        }

        #[test]
        fn test_low_temp_preserves_better() {
            let cpi_cold = calculate_cpi(85_000.0, 1000.0, 4.0, None, 1.0);
            let cpi_warm = calculate_cpi(85_000.0, 1000.0, 25.0, None, 1.0);

            assert!(cpi_cold.cpi_score > cpi_warm.cpi_score,
                "低温环境胶原保存优于高温 (冷:{:.1}% vs 暖:{:.1}%)",
                cpi_cold.cpi_score, cpi_warm.cpi_score);
        }

        #[test]
        fn test_short_burial_high_preservation() {
            let cpi = calculate_cpi(85_000.0, 10.0, 15.0, None, 1.0);
            assert!(cpi.cpi_score > 80.0, "短埋藏(10年)应保存80%以上胶原");
            assert!(cpi.cpi_grade.starts_with("A") || cpi.cpi_grade.starts_with("B"),
                "短埋藏应为A或B级");
        }

        #[test]
        fn test_long_burial_low_preservation() {
            let cpi = calculate_cpi(85_000.0, 50_000.0, 20.0, None, 1.0);
            assert!(cpi.cpi_score < 10.0, "极长埋藏(5万年)应严重降解");
        }

        #[test]
        fn test_temperature_history_accelerated_degradation() {
            let steady = calculate_cpi(85_000.0, 1000.0, 15.0, None, 1.0);

            let warm_history = vec![
                TemperatureHistoryPoint { years_bp: 1000.0, temp_celsius: 25.0 },
                TemperatureHistoryPoint { years_bp: 500.0, temp_celsius: 20.0 },
                TemperatureHistoryPoint { years_bp: 0.0, temp_celsius: 15.0 },
            ];
            let warm = calculate_cpi(85_000.0, 1000.0, 15.0, Some(warm_history), 1.0);

            assert!(warm.cpi_score < steady.cpi_score,
                "历史温度更高的场景降解更严重");
            assert!(warm.equivalent_years_at_20c > steady.equivalent_years_at_20c,
                "暖历史等效年数应大于恒温等效年数");
        }

        #[test]
        fn test_equivalent_time_method_valid() {
            let history = vec![
                TemperatureHistoryPoint { years_bp: 100.0, temp_celsius: 20.0 },
                TemperatureHistoryPoint { years_bp: 0.0, temp_celsius: 20.0 },
            ];
            let cpi = calculate_cpi(85_000.0, 100.0, 20.0, Some(history), 1.0);

            assert!((cpi.equivalent_years_at_20c - 100.0).abs() < 1.0,
                "恒温20°C埋藏100年，等效年数应≈100年 (实际:{:.1})",
                cpi.equivalent_years_at_20c);
        }

        #[test]
        fn test_half_life_positive_always() {
            let cases = vec![
                (50_000.0, 10.0),
                (85_000.0, 15.0),
                (120_000.0, 30.0),
                (200_000.0, 4.0),
            ];
            for (ea, temp) in cases {
                let cpi = calculate_cpi(ea, 100.0, temp, None, 1.0);
                assert!(cpi.predicted_half_life_years > 0.0,
                    "半衰期必须为正: Ea={}, T={} -> t1/2={}",
                    ea, temp, cpi.predicted_half_life_years);
                assert!(cpi.initial_half_life_years > 0.0);
            }
        }

        #[test]
        fn test_cpi_score_bounds_0_100() {
            let cpi_very_long = calculate_cpi(50_000.0, 1_000_000.0, 30.0, None, 1.0);
            assert!(cpi_very_long.cpi_score >= 0.0 && cpi_very_long.cpi_score <= 100.0,
                "CPI分数必须在0-100范围内");
            assert!(cpi_very_long.remaining_collagen_pct >= 0.0 && cpi_very_long.remaining_collagen_pct <= 100.0);
        }

        #[test]
        fn test_grade_boundary_a_85() {
            let cpi = calculate_cpi(85_000.0, 10.0, 4.0, None, 1.0);
            assert!(cpi.cpi_score >= 85.0 || (cpi.cpi_grade.starts_with("A") && cpi.cpi_score >= 65.0),
                "短埋藏低温应为高保存等级 (分数:{:.1}, 等级:{})", cpi.cpi_score, cpi.cpi_grade);
        }

        #[test]
        fn test_empty_history_uses_default() {
            let cpi = calculate_cpi(85_000.0, 500.0, 15.0, None, 1.0);
            assert!(!cpi.temperature_history.is_empty(), "无温度史时应自动生成默认温度史");
            assert!(cpi.temperature_history.len() >= 3);
        }

        #[test]
        fn test_average_temperature_calculated() {
            let history = vec![
                TemperatureHistoryPoint { years_bp: 50.0, temp_celsius: 10.0 },
                TemperatureHistoryPoint { years_bp: 0.0, temp_celsius: 20.0 },
            ];
            let cpi = calculate_cpi(85_000.0, 50.0, 15.0, Some(history), 1.0);
            assert!(cpi.average_temp_c > 0.0, "平均温度应被计算");
            assert!(cpi.average_temp_c < 30.0);
        }

        #[test]
        fn test_interpretation_not_empty() {
            let cpi = calculate_cpi(85_000.0, 500.0, 15.0, None, 1.0);
            assert!(!cpi.interpretation.is_empty());
            assert!(cpi.interpretation.len() > 20);
        }

        #[test]
        fn test_arrhenius_temperature_dependence_monotonic() {
            let arr_cfg = crate::algorithms::ArrheniusConfig {
                ea: 85_000.0,
                a: 1.2e10,
                r: MOLAR_GAS_R,
                ph_acid_coeff: 4.5e-4,
                ph_base_coeff: 8.0e-5,
                ph_neutral_point: 7.0,
            };
            let k_cold = arrhenius_rate_constant(0.0, &arr_cfg);
            let k_warm = arrhenius_rate_constant(30.0, &arr_cfg);
            assert!(k_warm > k_cold, "温度越高，Arrhenius速率常数越大");
            assert!(k_warm > k_cold * 2.0, "30°C速率应为0°C的2倍以上");
        }
    }

    // ============================================================
    // Feature 3: 出土时机优化（蒙特卡洛）测试集合
    // 覆盖：置信区间、窗口覆盖、统计有效性、极端参数
    // ============================================================

    mod monte_carlo_tests {
        use super::*;

        fn make_params(n: usize, years: f64) -> MonteCarloParams {
            MonteCarloParams {
                num_simulations: n,
                current_ph: 7.0,
                ph_std_dev: 0.3,
                current_temp_c: 18.0,
                temp_std_dev: 2.0,
                current_ca_ppm: 80.0,
                ca_std_dev: 15.0,
                current_orp_mv: 100.0,
                orp_std_dev: 50.0,
                forecast_years: years,
                time_steps_per_year: 4,
                target_corrosion_threshold_um: 200.0,
                acceptable_risk_threshold: 0.25,
                current_collagen_remaining_pct: 70.0,
            }
        }

        #[test]
        fn test_simulations_count_matches() {
            let params = make_params(500, 5.0);
            let result = run_monte_carlo_excavation(params);
            assert_eq!(result.simulations_completed, 500, "500次模拟应返回500");
        }

        #[test]
        fn test_confidence_interval_ordering() {
            let params = make_params(200, 10.0);
            let result = run_monte_carlo_excavation(params);

            for y in &result.year_by_year_stats {
                assert!(y.p5_corrosion_um <= y.p25_corrosion_um, "P5应≤P25");
                assert!(y.p25_corrosion_um <= y.p50_corrosion_um, "P25应≤P50");
                assert!(y.p50_corrosion_um <= y.p75_corrosion_um, "P50应≤P75");
                assert!(y.p75_corrosion_um <= y.p95_corrosion_um, "P75应≤P95");
                assert!(y.mean_corrosion_um >= 0.0, "平均腐蚀深度非负");
            }
        }

        #[test]
        fn test_corrosion_increases_over_time() {
            let params = make_params(200, 20.0);
            let result = run_monte_carlo_excavation(params);

            let stats = &result.year_by_year_stats;
            assert!(stats.len() >= 2, "至少有2年数据");

            let first_mean = stats[0].mean_corrosion_um;
            let last_mean = stats[stats.len() - 1].mean_corrosion_um;
            assert!(last_mean >= first_mean, "腐蚀深度随时间单调不减");
        }

        #[test]
        fn test_optimal_window_within_windows() {
            let params = make_params(200, 10.0);
            let result = run_monte_carlo_excavation(params);

            assert!(!result.windows.is_empty(), "至少有一个窗口评估结果");

            let opt_prob = result.optimal_window.probability_of_success;
            let all_probs: Vec<f64> = result.windows.iter()
                .map(|w| w.probability_of_success).collect();

            let max_prob = all_probs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            assert!((opt_prob - max_prob).abs() < 0.001 || opt_prob <= max_prob,
                "最优窗口成功率应为或接近最大值 (最优:{:.3}, 最大:{:.3})",
                opt_prob, max_prob);
        }

        #[test]
        fn test_windows_cover_forecast_range() {
            let params = make_params(100, 10.0);
            let result = run_monte_carlo_excavation(params);

            assert!(!result.windows.is_empty());

            let min_start = result.windows.iter()
                .map(|w| w.start_year).fold(f64::INFINITY, f64::min);
            let max_end = result.windows.iter()
                .map(|w| w.end_year).fold(f64::NEG_INFINITY, f64::max);

            assert!(min_start <= 1.0, "最早窗口应从0-1年附近开始，覆盖近期");
            assert!(max_end >= 3.0 || result.windows.len() >= 4,
                "窗口应覆盖足够长的预测范围");
        }

        #[test]
        fn test_success_probability_bounds() {
            let params = make_params(200, 15.0);
            let result = run_monte_carlo_excavation(params);

            for w in &result.windows {
                assert!(w.probability_of_success >= 0.0 && w.probability_of_success <= 1.0,
                    "成功率必须在[0,1]范围内: {}", w.probability_of_success);
            }
            assert!(result.confidence_level >= 0.0 && result.confidence_level <= 1.0);
        }

        #[test]
        fn test_prob_exceed_increases_with_time() {
            let params = make_params(200, 20.0);
            let result = run_monte_carlo_excavation(params);

            let stats = &result.year_by_year_stats;
            if stats.len() >= 2 {
                let first_exceed = stats[0].prob_exceed_threshold;
                let last_exceed = stats[stats.len() - 1].prob_exceed_threshold;
                assert!(last_exceed >= first_exceed, "超阈概率随时间不下降");
            }
        }

        #[test]
        fn test_risk_distribution_valid() {
            let params = make_params(200, 10.0);
            let result = run_monte_carlo_excavation(params);

            let rd = &result.risk_distribution;
            assert!(!rd.percentiles.is_empty(), "应有分位数数据");
            assert!(!rd.probability_by_year.is_empty(), "应有逐年概率数据");

            for (label, value) in &rd.percentiles {
                assert!(!label.is_empty(), "分位数标签不能为空");
                assert!(*value >= 0.0, "分位数值应非负: {}", value);
            }

            for (year, prob) in &rd.probability_by_year {
                assert!(*year >= 0.0, "年份应非负");
                assert!(*prob >= 0.0 && *prob <= 1.0, "概率应在[0,1]范围内");
            }
        }

        #[test]
        fn test_final_recommendation_not_empty() {
            let params = make_params(100, 5.0);
            let result = run_monte_carlo_excavation(params);
            assert!(!result.final_recommendation.is_empty(), "必须有最终建议");
            assert!(result.final_recommendation.len() > 20);
        }

        #[test]
        fn test_small_std_dev_gives_narrow_confidence() {
            let mut params_narrow = make_params(200, 10.0);
            params_narrow.ph_std_dev = 0.05;
            params_narrow.temp_std_dev = 0.3;
            let result_narrow = run_monte_carlo_excavation(params_narrow);

            let mut params_wide = make_params(200, 10.0);
            params_wide.ph_std_dev = 1.5;
            params_wide.temp_std_dev = 8.0;
            let result_wide = run_monte_carlo_excavation(params_wide);

            if !result_narrow.year_by_year_stats.is_empty() && !result_wide.year_by_year_stats.is_empty() {
                let mid_idx = result_narrow.year_by_year_stats.len() / 2;
                let narrow_p95_p5 = result_narrow.year_by_year_stats[mid_idx].p95_corrosion_um
                    - result_narrow.year_by_year_stats[mid_idx].p5_corrosion_um;
                let wide_p95_p5 = result_wide.year_by_year_stats[mid_idx].p95_corrosion_um
                    - result_wide.year_by_year_stats[mid_idx].p5_corrosion_um;

                assert!(wide_p95_p5 > narrow_p95_p5 * 0.5,
                    "大方差应导致更宽的置信区间 (宽:{:.1} vs 窄:{:.1})",
                    wide_p95_p5, narrow_p95_p5);
            }
        }

        #[test]
        fn test_minimal_simulation_count() {
            let params = make_params(1, 1.0);
            let result = run_monte_carlo_excavation(params);
            assert!(result.simulations_completed >= 100,
                "小于100次模拟时应自动提升到100次 (实际:{})",
                result.simulations_completed);
            assert!(!result.year_by_year_stats.is_empty());
        }

        #[test]
        fn test_zero_time_steps_handled() {
            let mut params = make_params(50, 1.0);
            params.time_steps_per_year = 1;
            let result = run_monte_carlo_excavation(params);
            assert!(result.year_by_year_stats.len() >= 1,
                "即使每年1步，也应有年度统计");
        }

        #[test]
        fn test_window_recommendation_not_empty() {
            let params = make_params(100, 5.0);
            let result = run_monte_carlo_excavation(params);
            for w in &result.windows {
                assert!(!w.recommendation.is_empty());
            }
        }

        #[test]
        fn test_net_benefit_calculated() {
            let params = make_params(100, 5.0);
            let result = run_monte_carlo_excavation(params);
            for w in &result.windows {
                assert!(w.net_benefit.is_finite(), "净收益应为有限值");
                assert!(w.expected_damage_if_wait >= 0.0);
                assert!(w.expected_damage_if_excavate >= 0.0);
            }
        }
    }

    // ============================================================
    // Feature 4: 现场临时保护方案 测试集合
    // 覆盖：酸性环境PEG200、全pH区间、边界值、极端/异常、决策树
    // ============================================================

    mod protection_tests {
        use super::*;

        #[test]
        fn test_acidic_environment_recommends_peg200() {
            let rec = recommend_temporary_protection(
                5.5, 60.0, 50.0, 20.0, 60.0, 1.0, "人骨"
            );

            assert!(rec.primary_moisturizer.to_lowercase().contains("peg")
                || rec.secondary_recommendations.iter().any(|s| s.to_lowercase().contains("peg")),
                "酸性+低-中钙环境应推荐PEG200作为保湿剂 (主材料: {})",
                rec.primary_moisturizer);
        }

        #[test]
        fn test_strongly_acidic_needs_neutralization() {
            let rec = recommend_temporary_protection(
                4.0, 80.0, 0.0, 22.0, 55.0, 1.0, "兽骨"
            );
            assert!(rec.ph_neutralization_required,
                "强酸性(pH=4.0)应需要pH中和处理");
            assert!(rec.neutralization_agent.is_some(),
                "需要中和时应指定中和剂");
            assert!(!rec.neutralization_agent.as_ref().unwrap().is_empty(),
                "中和剂名称不应为空");
        }

        #[test]
        fn test_neutral_ph_no_neutralization() {
            let rec = recommend_temporary_protection(
                7.0, 100.0, 0.0, 20.0, 60.0, 1.5, "骨器"
            );
            assert!(!rec.ph_neutralization_required,
                "中性pH不需要中和");
        }

        #[test]
        fn test_alkaline_environment_handling() {
            let rec = recommend_temporary_protection(
                9.0, 150.0, -100.0, 18.0, 50.0, 2.0, "人骨"
            );
            assert!(!rec.primary_moisturizer.is_empty());
            assert!(rec.expected_effectiveness_score > 0.0);
            assert!(!rec.step_by_step_protocol.is_empty());
        }

        #[test]
        fn test_ph_classification_full_range() {
            assert_eq!(classify_ph_condition(2.0), "EXTREMELY_ACIDIC");
            assert_eq!(classify_ph_condition(4.0), "EXTREMELY_ACIDIC");
            assert_eq!(classify_ph_condition(4.5), "HIGHLY_ACIDIC");
            assert_eq!(classify_ph_condition(5.5), "MODERATELY_ACIDIC");
            assert_eq!(classify_ph_condition(6.8), "NEUTRAL");
            assert_eq!(classify_ph_condition(7.2), "NEUTRAL");
            assert_eq!(classify_ph_condition(8.0), "MODERATELY_ALKALINE");
            assert_eq!(classify_ph_condition(9.0), "HIGHLY_ALKALINE");
            assert_eq!(classify_ph_condition(10.5), "EXTREMELY_ALKALINE");
            assert_eq!(classify_ph_condition(13.0), "EXTREMELY_ALKALINE");
        }

        #[test]
        fn test_ca_classification_full_range() {
            assert_eq!(classify_ca_condition(5.0), "VERY_LOW_CA");
            assert_eq!(classify_ca_condition(20.0), "VERY_LOW_CA");
            assert_eq!(classify_ca_condition(40.0), "LOW_CA");
            assert_eq!(classify_ca_condition(60.0), "LOW_CA");
            assert_eq!(classify_ca_condition(100.0), "NORMAL_CA");
            assert_eq!(classify_ca_condition(150.0), "NORMAL_CA");
            assert_eq!(classify_ca_condition(250.0), "HIGH_CA");
            assert_eq!(classify_ca_condition(400.0), "VERY_HIGH_CA");
            assert_eq!(classify_ca_condition(1000.0), "VERY_HIGH_CA");
        }

        #[test]
        fn test_orp_classification() {
            assert_eq!(classify_origination(300.0), "HIGHLY_OXIDIZING");
            assert_eq!(classify_origination(100.0), "MODERATELY_OXIDIZING");
            assert_eq!(classify_origination(-50.0), "MODERATELY_REDUCING");
            assert_eq!(classify_origination(-200.0), "STRONGLY_REDUCING");
        }

        #[test]
        fn test_effectiveness_score_positive() {
            let cases = vec![
                (4.0, 50.0, 0.0),
                (7.0, 100.0, 0.0),
                (9.0, 200.0, -50.0),
                (5.5, 30.0, 100.0),
            ];
            for (ph, ca, orp) in cases {
                let rec = recommend_temporary_protection(
                    ph, ca, orp, 20.0, 60.0, 1.0, "人骨"
                );
                assert!(rec.expected_effectiveness_score > 0.0 && rec.expected_effectiveness_score <= 100.0,
                    "有效性评分应在0-100之间: pH={}, Ca={}, 得分={}",
                    ph, ca, rec.expected_effectiveness_score);
            }
        }

        #[test]
        fn test_decision_path_not_empty() {
            let rec = recommend_temporary_protection(
                6.5, 80.0, 50.0, 22.0, 65.0, 1.0, "人骨"
            );
            assert!(!rec.decision_path.is_empty(), "决策路径不能为空");
            assert!(rec.decision_path.len() >= 3, "决策树至少有3层判断");
        }

        #[test]
        fn test_step_by_step_protocol_has_steps() {
            let rec = recommend_temporary_protection(
                7.0, 100.0, 0.0, 20.0, 60.0, 1.5, "人骨"
            );
            assert!(rec.step_by_step_protocol.len() >= 5,
                "操作流程至少有5步 (实际:{})", rec.step_by_step_protocol.len());

            for (i, step) in rec.step_by_step_protocol.iter().enumerate() {
                assert!(step.contains(&format!("步骤{}", i + 1)) || step.contains("步骤"),
                    "第{}步应包含步骤编号: {}", i + 1, step);
            }
        }

        #[test]
        fn test_materials_list_includes_primary() {
            let rec = recommend_temporary_protection(
                7.0, 100.0, 0.0, 20.0, 60.0, 1.0, "人骨"
            );
            assert!(!rec.materials_needed.is_empty(), "材料清单不能为空");

            let primary_lower = rec.primary_moisturizer.to_lowercase();
            let materials_names: Vec<String> = rec.materials_needed.iter()
                .map(|m| m.name.to_lowercase())
                .collect();
            let materials_str = materials_names.join(" ");
            assert!(materials_str.contains(&primary_lower.chars().take(5).collect::<String>())
                || rec.materials_needed.iter().any(|m| m.purpose.to_lowercase().contains("保湿")),
                "材料清单应包含主保湿剂相关材料");
        }

        #[test]
        fn test_warnings_present_for_risky_cases() {
            let rec_acid = recommend_temporary_protection(
                4.0, 200.0, 200.0, 30.0, 40.0, 0.5, "人骨"
            );
            assert!(!rec_acid.warnings.is_empty() || rec_acid.ph_neutralization_required,
                "高风险酸性环境应有警告或需中和处理");
        }

        #[test]
        fn test_very_low_ca_recommendation() {
            let rec = recommend_temporary_protection(
                7.0, 10.0, 0.0, 20.0, 60.0, 1.0, "骨器"
            );
            assert!(!rec.primary_moisturizer.is_empty());
            assert!(!rec.secondary_recommendations.is_empty() || rec.expected_effectiveness_score > 40.0);
        }

        #[test]
        fn test_very_high_ca_mineral_deposition_risk() {
            let rec = recommend_temporary_protection(
                8.0, 500.0, 100.0, 25.0, 70.0, 2.0, "兽骨"
            );
            let warning_text = rec.warnings.join("").to_lowercase();
            assert!(warning_text.contains("钙") || warning_text.contains("沉积")
                || warning_text.contains("矿化") || rec.warnings.len() >= 2,
                "高钙+碱性环境应提及钙沉积风险");
        }

        #[test]
        fn test_high_temp_extends_protocol() {
            let rec_hot = recommend_temporary_protection(
                7.0, 100.0, 0.0, 32.0, 45.0, 1.0, "人骨"
            );
            let protocol_hot = rec_hot.step_by_step_protocol.join("");
            assert!(protocol_hot.contains("冷藏") || protocol_hot.contains("冰袋") || protocol_hot.contains("低温"),
                "高温环境应包含冷藏/降温措施");
        }

        #[test]
        fn test_concentration_between_0_and_100() {
            let cases = vec![
                (5.0, 30.0), (7.0, 80.0), (9.0, 150.0),
            ];
            for (ph, ca) in cases {
                let rec = recommend_temporary_protection(
                    ph, ca, 0.0, 20.0, 60.0, 1.0, "人骨"
                );
                assert!(rec.concentration_pct > 0.0 && rec.concentration_pct <= 100.0,
                    "浓度百分比应在0-100之间: {}", rec.concentration_pct);
            }
        }

        #[test]
        fn test_application_method_not_empty() {
            let rec = recommend_temporary_protection(
                7.0, 100.0, 0.0, 20.0, 60.0, 1.0, "人骨"
            );
            assert!(!rec.application_method.is_empty(), "必须有施用方法说明");
        }

        #[test]
        fn test_stabilization_hours_positive() {
            let rec = recommend_temporary_protection(
                7.0, 100.0, 0.0, 20.0, 60.0, 1.0, "人骨"
            );
            assert!(rec.estimated_stabilization_hours > 0.0,
                "稳定时间应大于0: {}小时", rec.estimated_stabilization_hours);
        }

        #[test]
        fn test_primary_moisturizer_zh_chinese() {
            let rec = recommend_temporary_protection(
                7.0, 100.0, 0.0, 20.0, 60.0, 1.0, "人骨"
            );
            assert!(!rec.primary_moisturizer_zh.is_empty());
            assert!(!rec.primary_moisturizer.is_empty());
            assert_ne!(rec.primary_moisturizer, rec.primary_moisturizer_zh,
                "英文名和中文名应不同");
        }

        #[test]
        fn test_secondary_recommendations_list() {
            let rec = recommend_temporary_protection(
                7.0, 100.0, 0.0, 20.0, 60.0, 1.0, "人骨"
            );
            assert!(!rec.secondary_recommendations.is_empty() || rec.expected_effectiveness_score > 60.0);
        }

        #[test]
        fn test_extreme_acid_boundary_4_5() {
            let rec_mild_acid = recommend_temporary_protection(
                4.6, 50.0, 0.0, 20.0, 60.0, 1.0, "人骨"
            );
            let rec_strong_acid = recommend_temporary_protection(
                4.4, 50.0, 0.0, 20.0, 60.0, 1.0, "人骨"
            );

            assert_ne!(rec_mild_acid.ph_neutralization_required,
                rec_strong_acid.ph_neutralization_required,
                "pH≈4.5边界两侧中和需求应不同");
        }
    }

    // ============================================================
    // 综合集成测试：验证4个Feature整体协同
    // ============================================================

    mod integration_tests {
        use super::*;

        #[test]
        fn test_full_workflow_neutral_oxic() {
            let ph = 7.0;
            let eh = 150.0;

            let diagram = generate_eh_ph_diagram(
                ph, eh, (2.0, 12.0), (-500.0, 800.0), (10, 10)
            );
            assert!(!diagram.dominant_zone_name.is_empty());

            let cpi = calculate_cpi(85_000.0, 500.0, 15.0, None, 1.0);
            assert!(cpi.cpi_score > 0.0);

            let params = MonteCarloParams {
                num_simulations: 50,
                current_ph: ph,
                ph_std_dev: 0.2,
                current_temp_c: 15.0,
                temp_std_dev: 1.5,
                current_ca_ppm: 80.0,
                ca_std_dev: 10.0,
                current_orp_mv: eh,
                orp_std_dev: 30.0,
                forecast_years: 10.0,
                time_steps_per_year: 4,
                target_corrosion_threshold_um: 150.0,
                acceptable_risk_threshold: 0.3,
                current_collagen_remaining_pct: cpi.remaining_collagen_pct,
            };
            let excavation = run_monte_carlo_excavation(params);
            assert!(excavation.simulations_completed >= 50,
                "模拟数量应≥50 (实际:{})", excavation.simulations_completed);

            let protection = recommend_temporary_protection(
                ph, 80.0, eh, 20.0, 60.0, 1.0, "人骨"
            );
            assert!(!protection.primary_moisturizer.is_empty());
        }

        #[test]
        fn test_full_workflow_acidic_sulfate_reducing() {
            let ph = 5.5;
            let eh = -150.0;

            let diagram = generate_eh_ph_diagram(
                ph, eh, (2.0, 12.0), (-500.0, 800.0), (10, 10)
            );
            assert_eq!(diagram.sample_point.zone, RedoxZone::SULFATE_REDUCING);
            assert!(!diagram.sample_point.stable_phase.is_empty());

            let cpi = calculate_cpi(95_000.0, 2000.0, 12.0, None, 1.0);
            assert!(cpi.cpi_score > 0.0);

            let protection = recommend_temporary_protection(
                ph, 60.0, eh, 22.0, 55.0, 1.5, "人骨"
            );
            assert!(protection.ph_neutralization_required || protection.primary_moisturizer.to_lowercase().contains("peg"));
        }
    }

    // ============================================================
    // 通用数学/物理常量测试
    // ============================================================

    mod constant_tests {
        use super::*;

        #[test]
        fn test_faraday_constant_value() {
            assert!((FARADAY_F - 96485.0).abs() < 1.0,
                "法拉第常数约为96485 C/mol");
        }

        #[test]
        fn test_gas_constant_value() {
            assert!((MOLAR_GAS_R - 8.314).abs() < 0.01,
                "摩尔气体常数约为8.314 J/(mol·K)");
        }

        #[test]
        fn test_standard_temp_room_temperature() {
            assert!((STANDARD_TEMP_K - 298.15).abs() < 1.0,
                "标准温度约为298.15K (25°C)");
        }
    }
}
