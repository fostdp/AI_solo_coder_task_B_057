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

    #[test]
    fn test_eh_ph_zone_classification_oxidized() {
        let zone = classify_redox_zone(7.0, 500.0);
        assert!(matches!(zone, RedoxZone::OXIDIZED | RedoxZone::SUBSURFACE_OXIC));
    }

    #[test]
    fn test_eh_ph_zone_classification_iron_reducing() {
        let zone = classify_redox_zone(7.0, -50.0);
        assert!(matches!(zone, RedoxZone::IRON_REDUCING | RedoxZone::MANGANESE_REDUCING));
    }

    #[test]
    fn test_eh_ph_zone_classification_sulfate_reducing() {
        let zone = classify_redox_zone(7.0, -200.0);
        assert!(matches!(zone, RedoxZone::SULFATE_REDUCING));
    }

    #[test]
    fn test_generate_eh_ph_diagram() {
        let diagram = generate_eh_ph_diagram(7.0, 100.0, (2.0, 12.0), (-500.0, 800.0), (20, 20));
        assert_eq!(diagram.zones.len(), 400);
        assert_eq!(diagram.boundaries.len(), 7);
        assert!(!diagram.dominant_zone_name.is_empty());
    }

    #[test]
    fn test_calculate_cpi_high_collagen() {
        let cpi = calculate_cpi(85_000.0, 100.0, 10.0, None, 1.0);
        assert!(cpi.cpi_score > 50.0);
        assert!(cpi.remaining_collagen_pct > 0.0 && cpi.remaining_collagen_pct <= 100.0);
        assert!(cpi.predicted_half_life_years > 0.0);
    }

    #[test]
    fn test_calculate_cpi_low_temp_long_burial() {
        let cpi = calculate_cpi(85_000.0, 1000.0, 4.0, None, 1.0);
        assert!(cpi.cpi_score > 0.0);
        assert!(cpi.predicted_half_life_years > 100.0);
    }

    #[test]
    fn test_monte_carlo_basic() {
        let params = MonteCarloParams {
            num_simulations: 100,
            current_ph: 7.0,
            ph_std_dev: 0.1,
            current_temp_c: 15.0,
            temp_std_dev: 1.0,
            current_ca_ppm: 100.0,
            ca_std_dev: 10.0,
            current_orp_mv: 50.0,
            orp_std_dev: 20.0,
            forecast_years: 5.0,
            time_steps_per_year: 4,
            target_corrosion_threshold_um: 100.0,
            acceptable_risk_threshold: 0.3,
            current_collagen_remaining_pct: 80.0,
        };
        let result = run_monte_carlo_excavation(params);
        assert_eq!(result.simulations_completed, 100);
        assert!(!result.windows.is_empty());
        assert!(!result.year_by_year_stats.is_empty());
        assert!(result.confidence_level >= 0.0 && result.confidence_level <= 1.0);
    }

    #[test]
    fn test_protection_recommendation_acidic_low_ca() {
        let rec = recommend_temporary_protection(
            5.0, 25.0, -50.0, 22.0, 50.0, 1.0, "人骨"
        );
        assert!(rec.ph_neutralization_required || rec.primary_moisturizer.contains("PEG"));
        assert!(!rec.step_by_step_protocol.is_empty());
        assert!(rec.expected_effectiveness_score > 50.0);
    }

    #[test]
    fn test_protection_recommendation_neutral_normal() {
        let rec = recommend_temporary_protection(
            7.0, 100.0, 0.0, 20.0, 60.0, 1.5, "兽骨"
        );
        assert!(!rec.ph_neutralization_required);
        assert!(rec.primary_moisturizer.contains("DI_Water") || rec.primary_moisturizer.contains("PEG"));
    }

    #[test]
    fn test_ph_classification() {
        assert_eq!(classify_ph_condition(3.0), "EXTREMELY_ACIDIC");
        assert_eq!(classify_ph_condition(5.0), "HIGHLY_ACIDIC");
        assert_eq!(classify_ph_condition(6.0), "MODERATELY_ACIDIC");
        assert_eq!(classify_ph_condition(7.0), "NEUTRAL");
        assert_eq!(classify_ph_condition(8.0), "MODERATELY_ALKALINE");
        assert_eq!(classify_ph_condition(9.0), "HIGHLY_ALKALINE");
        assert_eq!(classify_ph_condition(10.0), "EXTREMELY_ALKALINE");
    }

    #[test]
    fn test_ca_classification() {
        assert_eq!(classify_ca_condition(10.0), "VERY_LOW_CA");
        assert_eq!(classify_ca_condition(50.0), "LOW_CA");
        assert_eq!(classify_ca_condition(100.0), "NORMAL_CA");
        assert_eq!(classify_ca_condition(300.0), "HIGH_CA");
        assert_eq!(classify_ca_condition(500.0), "VERY_HIGH_CA");
    }
}
