use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub influxdb: InfluxConfig,
    pub arrhenius: ArrheniusConfig,
    pub michaelis_menten: MichaelisMentenConfig,
    pub calcium_phosphate: CalciumPhosphateConfig,
    pub alerts: AlertConfig,
    pub ode_solver: OdeSolverConfig,
    pub lora_downlink: LoraDownlinkConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub frontend_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfluxConfig {
    pub url: String,
    pub database: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArrheniusConfig {
    pub ea: f64,
    pub a_pre: f64,
    pub r: f64,
    pub ph_acid_coeff: f64,
    pub ph_base_coeff: f64,
    pub ph_neutral_point: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    pub ph_low_threshold: f64,
    pub ca_high_threshold: f64,
    pub orp_low_threshold: f64,
    pub orp_high_threshold: f64,
    pub temp_high_threshold: f64,
    pub cooldown_seconds: u64,
    pub dingtalk_webhook: Option<String>,
    pub dingtalk_secret: Option<String>,
    pub sms_api_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OdeSolverConfig {
    pub rtol: f64,
    pub atol: f64,
    pub max_steps: usize,
    pub initial_dt: f64,
    pub max_order: usize,
    pub enforce_non_negative: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoraDownlinkConfig {
    pub max_retries: u32,
    pub retransmit_delay_ms: u64,
    pub ack_timeout_ms: u64,
    pub default_port: u8,
    pub ack_port: u8,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            influxdb: InfluxConfig::default(),
            arrhenius: ArrheniusConfig::default(),
            michaelis_menten: MichaelisMentenConfig::default(),
            calcium_phosphate: CalciumPhosphateConfig::default(),
            alerts: AlertConfig::default(),
            ode_solver: OdeSolverConfig::default(),
            lora_downlink: LoraDownlinkConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: env_or("SERVER_HOST", "0.0.0.0".to_string()),
            port: env_or_parse("SERVER_PORT", 8080),
            frontend_dir: env_or("FRONTEND_DIR", "../frontend".to_string()),
        }
    }
}

impl Default for InfluxConfig {
    fn default() -> Self {
        Self {
            url: env_or("INFLUXDB_URL", "http://127.0.0.1:8086".to_string()),
            database: env_or("INFLUXDB_DB", "relic_monitor".to_string()),
            username: env_or("INFLUXDB_USER", "writer".to_string()),
            password: env_or("INFLUXDB_PASS", "writer_relic_2026".to_string()),
        }
    }
}

impl Default for ArrheniusConfig {
    fn default() -> Self {
        Self {
            ea: env_or_f64("ARR_EA", 85_000.0),
            a_pre: env_or_f64("ARR_A", 1.2e10),
            r: env_or_f64("ARR_R", 8.314),
            ph_acid_coeff: env_or_f64("ARR_PH_ACID", 4.5e-4),
            ph_base_coeff: env_or_f64("ARR_PH_BASE", 8.0e-5),
            ph_neutral_point: env_or_f64("ARR_PH_NEUTRAL", 7.0),
        }
    }
}

impl Default for MichaelisMentenConfig {
    fn default() -> Self {
        Self {
            v_max: env_or_f64("MM_VMAX", 3.5e-7),
            km: env_or_f64("MM_KM", 0.012),
            enzyme_concentration: env_or_f64("MM_ENZYME_CONC", 1.0),
            substrate_initial: env_or_f64("MM_SUBSTRATE_INIT", 1.0),
            enzyme_ea: env_or_f64("MM_ENZYME_EA", 55_000.0),
            enzyme_a: env_or_f64("MM_ENZYME_A", 8.0e8),
            ph_optimum: env_or_f64("MM_PH_OPT", 6.8),
            ph_range: env_or_f64("MM_PH_RANGE", 1.5),
            temp_optimum_celsius: env_or_f64("MM_TEMP_OPT", 37.0),
        }
    }
}

impl Default for CalciumPhosphateConfig {
    fn default() -> Self {
        Self {
            stoichiometric_ca_p: env_or_f64("CA_STOICHIO", 1.667),
            initial_bone_mass_g: env_or_f64("CA_BONE_MASS", 100.0),
            mineral_fraction: env_or_f64("CA_MINERAL_FRAC", 0.69),
            organic_fraction: env_or_f64("CA_ORGANIC_FRAC", 0.22),
            water_fraction: env_or_f64("CA_WATER_FRAC", 0.09),
            ca_in_hap_mass_frac: env_or_f64("CA_HAP_CA_FRAC", 0.3989),
            p_in_hap_mass_frac: env_or_f64("CA_HAP_P_FRAC", 0.1850),
            solubility_constant_pksp: env_or_f64("CA_PKSP", 57.0),
        }
    }
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            ph_low_threshold: env_or_f64("ALERT_PH_LOW", 5.5),
            ca_high_threshold: env_or_f64("ALERT_CA_HIGH", 200.0),
            orp_low_threshold: env_or_f64("ALERT_ORP_LOW", -200.0),
            orp_high_threshold: env_or_f64("ALERT_ORP_HIGH", 400.0),
            temp_high_threshold: env_or_f64("ALERT_TEMP_HIGH", 35.0),
            cooldown_seconds: env_or_parse("ALERT_COOLDOWN", 1800),
            dingtalk_webhook: env::var("ALERT_DINGTALK_WEBHOOK").ok(),
            dingtalk_secret: env::var("ALERT_DINGTALK_SECRET").ok(),
            sms_api_url: env::var("ALERT_SMS_URL").ok(),
        }
    }
}

impl Default for OdeSolverConfig {
    fn default() -> Self {
        Self {
            rtol: env_or_f64("ODE_RTOL", 1.0e-6),
            atol: env_or_f64("ODE_ATOL", 1.0e-10),
            max_steps: env_or_parse("ODE_MAX_STEPS", 100_000),
            initial_dt: env_or_f64("ODE_INIT_DT", 0.01),
            max_order: env_or_parse("ODE_MAX_ORDER", 5),
            enforce_non_negative: env_or_parse("ODE_ENFORCE_NN", true),
        }
    }
}

impl Default for LoraDownlinkConfig {
    fn default() -> Self {
        Self {
            max_retries: env_or_parse("LORA_DL_MAX_RETRIES", 5),
            retransmit_delay_ms: env_or_parse("LORA_DL_DELAY_MS", 5000),
            ack_timeout_ms: env_or_parse("LORA_DL_ACK_TIMEOUT", 8000),
            default_port: env_or_parse("LORA_DL_PORT", 100),
            ack_port: env_or_parse("LORA_DL_ACK_PORT", 101),
        }
    }
}

fn env_or(key: &str, default: String) -> String {
    env::var(key).unwrap_or(default)
}

fn env_or_f64(key: &str, default: f64) -> f64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_or_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

impl AppConfig {
    pub fn load() -> Self {
        dotenv::dotenv().ok();
        Self::default()
    }

    pub fn from_file(_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::default())
    }
}
