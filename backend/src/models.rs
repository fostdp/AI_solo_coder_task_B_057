use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorReading {
    pub sensor_id: String,
    pub sensor_type: SensorType,
    pub value: f64,
    pub relic_id: Option<String>,
    pub grid_x: f64,
    pub grid_y: f64,
    pub depth: f64,
    pub temperature: Option<f64>,
    pub timestamp: Option<DateTime<Utc>>,
    pub battery: Option<f64>,
    pub rssi: Option<i32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SensorType {
    #[serde(rename = "pH")]
    PH,
    #[serde(rename = "ORP")]
    ORP,
    #[serde(rename = "Ca2+")]
    CA2,
    #[serde(rename = "Temp")]
    TEMP,
}

impl std::fmt::Display for SensorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SensorType::PH => write!(f, "pH"),
            SensorType::ORP => write!(f, "ORP"),
            SensorType::CA2 => write!(f, "Ca2+"),
            SensorType::TEMP => write!(f, "Temp"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelicInfo {
    pub id: String,
    pub name: String,
    pub category: String,
    pub grid_x: f64,
    pub grid_y: f64,
    pub burial_depth: f64,
    pub discovered_date: String,
    pub initial_condition: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorInfo {
    pub id: String,
    pub sensor_type: SensorType,
    pub relic_id: Option<String>,
    pub grid_x: f64,
    pub grid_y: f64,
    pub depth: f64,
    pub install_date: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrosionAnalysis {
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
    pub ca_p_ratio: f64,
    pub ca_p_ratio_predicted: f64,
    pub corrosion_rate: f64,
    pub corrosion_depth_um: f64,
    pub risk_level: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub alert_type: AlertType,
    pub sensor_id: String,
    pub relic_id: Option<String>,
    pub threshold: f64,
    pub actual_value: f64,
    pub message: String,
    pub channels: Vec<String>,
    pub status: AlertStatus,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum AlertType {
    PH_LOW,
    CA_HIGH,
    TEMP_HIGH,
    ORP_ABNORMAL,
    CORROSION_RISK,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AlertStatus {
    PENDING,
    SENT,
    ACKNOWLEDGED,
    RESOLVED,
    FAILED,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub message: String,
    pub data: Option<T>,
    pub timestamp: DateTime<Utc>,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T, message: &str) -> Self {
        Self {
            success: true,
            message: message.to_string(),
            data: Some(data),
            timestamp: Utc::now(),
        }
    }

    pub fn error(message: &str) -> Self {
        Self {
            success: false,
            message: message.to_string(),
            data: None,
            timestamp: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointCloudPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub corrosion_depth: f64,
    pub collagen_deg: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContourData {
    pub x: f64,
    pub y: f64,
    pub value: f64,
    pub label: String,
}
