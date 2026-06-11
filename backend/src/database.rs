use influxdb::{Client, Error as InfluxError, InfluxDbWriteable, ReadQuery, Timestamp};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use crate::models::{SensorReading, CorrosionAnalysis, RelicInfo, SensorInfo, SensorType};
use chrono::{DateTime, Utc, Duration};
use log::{info, error, warn};

#[derive(Clone)]
pub struct Database {
    client: Client,
    relics: Arc<RwLock<HashMap<String, RelicInfo>>>,
    sensors: Arc<RwLock<HashMap<String, SensorInfo>>>,
}

#[derive(InfluxDbWriteable)]
struct SensorReadingInflux {
    time: Timestamp,
    value: f64,
    #[influxdb(tag)]
    sensor_id: String,
    #[influxdb(tag)]
    sensor_type: String,
    #[influxdb(tag)]
    relic_id: Option<String>,
    #[influxdb(tag)]
    grid_x: f64,
    #[influxdb(tag)]
    grid_y: f64,
    #[influxdb(tag)]
    depth: f64,
    temperature: Option<f64>,
    battery: Option<f64>,
    rssi: Option<i32>,
}

#[derive(InfluxDbWriteable)]
struct CorrosionInflux {
    time: Timestamp,
    ph: f64,
    temperature: f64,
    ca_concentration: f64,
    orp: f64,
    collagen_deg_rate: f64,
    collagen_deg_percent: f64,
    abiotic_rate: f64,
    enzyme_rate: f64,
    enzyme_contribution_pct: f64,
    microbial_biomass: f64,
    ca_p_ratio: f64,
    ca_p_ratio_predicted: f64,
    corrosion_rate: f64,
    corrosion_depth_um: f64,
    #[influxdb(tag)]
    relic_id: String,
    #[influxdb(tag)]
    grid_x: f64,
    #[influxdb(tag)]
    grid_y: f64,
    risk_level: String,
}

impl Database {
    pub fn new(url: &str, db: &str, user: &str, pass: &str) -> Self {
        let client = Client::new(url, db).with_auth(user, pass);
        Self {
            client,
            relics: Arc::new(RwLock::new(HashMap::new())),
            sensors: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn verify_connection(&self) -> Result<(), String> {
        match self.client.ping().await {
            Ok((build, version)) => {
                info!("InfluxDB连接验证成功: build={}, version={}", build, version);
                Ok(())
            }
            Err(InfluxError::FetchError { status, .. }) => {
                let msg = match status {
                    Some(401) => "InfluxDB认证失败(401): 用户名或密码错误，请检查INFLUXDB_USER/INFLUXDB_PASS环境变量".to_string(),
                    Some(403) => "InfluxDB权限不足(403): 当前用户无写入权限".to_string(),
                    Some(404) => "InfluxDB数据库不存在(404): 请先创建数据库或检查INFLUXDB_DB配置".to_string(),
                    Some(code) => format!("InfluxDB返回错误状态码({}): 请检查服务配置", code),
                    None => "InfluxDB连接失败: 无法建立到服务器的连接，请检查INFLUXDB_URL和InfluxDB服务状态".to_string(),
                };
                error!("{}", msg);
                Err(msg)
            }
            Err(e) => {
                let msg = format!("InfluxDB连接异常: {:?}", e);
                error!("{}", msg);
                Err(msg)
            }
        }
    }

    fn classify_write_error(&self, err: &InfluxError) -> String {
        match err {
            InfluxError::FetchError { status: Some(401), .. } => {
                "InfluxDB认证失败(401): 写入凭证无效".to_string()
            }
            InfluxError::FetchError { status: Some(403), .. } => {
                "InfluxDB权限不足(403): 当前用户无写入权限，请授予writer角色".to_string()
            }
            InfluxError::FetchError { status: Some(404), .. } => {
                "InfluxDB数据库不存在(404): 写入目标数据库不存在".to_string()
            }
            InfluxError::FetchError { status: Some(400), .. } => {
                "InfluxDB请求格式错误(400): 数据模型与Schema不匹配".to_string()
            }
            InfluxError::FetchError { status: Some(code), .. } if *code >= 500 => {
                format!("InfluxDB服务端错误({}): 数据库服务可能过载或异常", code)
            }
            InfluxError::FetchError { status: None, .. } => {
                "InfluxDB连接中断: 写入时无法连接服务器".to_string()
            }
            _ => format!("InfluxDB写入失败: {:?}", err),
        }
    }

    pub async fn init_default_data(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut relics = self.relics.write();
        for i in 1..=500 {
            let id = format!("RLC-{:04}", i);
            let grid_x = (rand::random::<f64>() * 49.0).round() + 0.5;
            let grid_y = (rand::random::<f64>() * 49.0).round() + 0.5;
            let depth = 0.3 + rand::random::<f64>() * 2.2;
            let category = match i % 5 {
                0 => "骨针",
                1 => "骨铲",
                2 => "骨锥",
                3 => "角器",
                _ => "牙制品",
            };
            relics.insert(id.clone(), RelicInfo {
                id: id.clone(),
                name: format!("{}-{}号", category, i),
                category: category.to_string(),
                grid_x,
                grid_y,
                burial_depth: depth,
                discovered_date: "2024-03-15".to_string(),
                initial_condition: match i % 3 {
                    0 => "完整".to_string(),
                    1 => "轻微残损".to_string(),
                    _ => "部分破碎".to_string(),
                },
                description: None,
            });
        }
        info!("已初始化500件文物信息");

        let mut sensors = self.sensors.write();
        for i in 1..=50 {
            let id = format!("PHR-{:03}", i);
            let grid_x = (rand::random::<f64>() * 49.0).round() + 0.5;
            let grid_y = (rand::random::<f64>() * 49.0).round() + 0.5;
            let depth = 0.3 + rand::random::<f64>() * 2.2;
            sensors.insert(id.clone(), SensorInfo {
                id,
                sensor_type: SensorType::PH,
                relic_id: None,
                grid_x,
                grid_y,
                depth,
                install_date: "2025-12-01".to_string(),
                status: "active".to_string(),
            });

            let id2 = format!("ORP-{:03}", i);
            sensors.insert(id2.clone(), SensorInfo {
                id: id2,
                sensor_type: SensorType::ORP,
                relic_id: None,
                grid_x,
                grid_y,
                depth,
                install_date: "2025-12-01".to_string(),
                status: "active".to_string(),
            });
        }

        for i in 1..=30 {
            let id = format!("CA-{:03}", i);
            let grid_x = (rand::random::<f64>() * 49.0).round() + 0.5;
            let grid_y = (rand::random::<f64>() * 49.0).round() + 0.5;
            let depth = 0.3 + rand::random::<f64>() * 2.2;
            sensors.insert(id.clone(), SensorInfo {
                id,
                sensor_type: SensorType::CA2,
                relic_id: None,
                grid_x,
                grid_y,
                depth,
                install_date: "2025-12-01".to_string(),
                status: "active".to_string(),
            });
        }
        info!("已初始化130个电极: 50 pH+50 ORP+30 Ca2+");
        Ok(())
    }

    pub async fn write_sensor_reading(&self, reading: &SensorReading) -> Result<(), InfluxError> {
        let ts = reading.timestamp.unwrap_or_else(Utc::now).into();
        let influx_reading = SensorReadingInflux {
            time: ts,
            value: reading.value,
            sensor_id: reading.sensor_id.clone(),
            sensor_type: reading.sensor_type.to_string(),
            relic_id: reading.relic_id.clone(),
            grid_x: reading.grid_x,
            grid_y: reading.grid_y,
            depth: reading.depth,
            temperature: reading.temperature,
            battery: reading.battery,
            rssi: reading.rssi,
        };
        let query = influx_reading.into_query("sensor_data");
        match self.client.query(query).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let classified = self.classify_write_error(&e);
                error!("写入传感器数据失败: {} | 原始错误: {:?}", classified, e);
                Err(e)
            }
        }
    }

    pub async fn write_corrosion_analysis(&self, analysis: &CorrosionAnalysis) -> Result<(), InfluxError> {
        let influx_data = CorrosionInflux {
            time: analysis.timestamp.into(),
            ph: analysis.ph,
            temperature: analysis.temperature,
            ca_concentration: analysis.ca_concentration,
            orp: analysis.orp,
            collagen_deg_rate: analysis.collagen_deg_rate,
            collagen_deg_percent: analysis.collagen_deg_percent,
            abiotic_rate: analysis.abiotic_rate,
            enzyme_rate: analysis.enzyme_rate,
            enzyme_contribution_pct: analysis.enzyme_contribution_pct,
            microbial_biomass: analysis.microbial_biomass,
            ca_p_ratio: analysis.ca_p_ratio,
            ca_p_ratio_predicted: analysis.ca_p_ratio_predicted,
            corrosion_rate: analysis.corrosion_rate,
            corrosion_depth_um: analysis.corrosion_depth_um,
            relic_id: analysis.relic_id.clone(),
            grid_x: analysis.grid_x,
            grid_y: analysis.grid_y,
            risk_level: analysis.risk_level.clone(),
        };
        let query = influx_data.into_query("corrosion_analysis");
        match self.client.query(query).await {
            Ok(_) => Ok(()),
            Err(e) => {
                let classified = self.classify_write_error(&e);
                error!("写入腐蚀分析失败: {} | 原始错误: {:?}", classified, e);
                Err(e)
            }
        }
    }

    pub async fn query_sensor_history(
        &self,
        sensor_id: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<Vec<(DateTime<Utc>, f64)>, Box<dyn std::error::Error>> {
        let query = ReadQuery::new(format!(
            "SELECT time, value FROM sensor_data WHERE sensor_id = '{}' AND time >= '{}' AND time <= '{}' ORDER BY time ASC",
            sensor_id,
            start_time.to_rfc3339(),
            end_time.to_rfc3339()
        ));
        let result = self.client.json_query(query).await?;
        let mut readings = Vec::new();
        if let Some(group) = result.deserialize_next::<serde_json::Value>().ok().and_then(|r| r.series.first().cloned()) {
            if let Some(values) = group.values.as_array() {
                for v in values {
                    if let (Some(time_str), Some(val)) = (v.get(0).and_then(|t| t.as_str()), v.get(1).and_then(|x| x.as_f64())) {
                        if let Ok(dt) = DateTime::parse_from_rfc3339(time_str) {
                            readings.push((dt.with_timezone(&Utc), val));
                        }
                    }
                }
            }
        }
        Ok(readings)
    }

    pub async fn query_latest_sensor_values(
        &self,
        sensor_type: Option<&str>,
    ) -> Result<Vec<SensorReading>, Box<dyn std::error::Error>> {
        let type_filter = sensor_type
            .map(|t| format!("AND sensor_type = '{}'", t))
            .unwrap_or_default();
        let query_str = format!(
            "SELECT LAST(*) FROM sensor_data {} GROUP BY sensor_id",
            type_filter
        );
        let query = ReadQuery::new(query_str);
        let result = self.client.json_query(query).await?;
        let mut readings = Vec::new();
        let sensors = self.sensors.read();
        if let Ok(grouped) = result.deserialize_next::<serde_json::Value>() {
            for group in grouped.series {
                if let Some(sensor_id) = group.tags.get("sensor_id") {
                    if let Some(values) = group.values.as_array().and_then(|v| v.first()) {
                        if let Some(val) = values.get(2).and_then(|x| x.as_f64()) {
                            if let Some(sensor) = sensors.get(sensor_id) {
                                readings.push(SensorReading {
                                    sensor_id: sensor_id.clone(),
                                    sensor_type: sensor.sensor_type,
                                    value: val,
                                    relic_id: sensor.relic_id.clone(),
                                    grid_x: sensor.grid_x,
                                    grid_y: sensor.grid_y,
                                    depth: sensor.depth,
                                    temperature: None,
                                    timestamp: None,
                                    battery: None,
                                    rssi: None,
                                });
                            }
                        }
                    }
                }
            }
        }
        Ok(readings)
    }

    pub async fn query_grid_data(
        &self,
        sensor_type: &str,
        hours: i64,
    ) -> Result<Vec<(f64, f64, f64)>, Box<dyn std::error::Error>> {
        let since = Utc::now() - Duration::hours(hours);
        let query_str = format!(
            "SELECT MEAN(value) FROM sensor_data WHERE sensor_type = '{}' AND time >= '{}' GROUP BY grid_x, grid_y",
            sensor_type,
            since.to_rfc3339()
        );
        let query = ReadQuery::new(query_str);
        let result = self.client.json_query(query).await?;
        let mut data = Vec::new();
        if let Ok(grouped) = result.deserialize_next::<serde_json::Value>() {
            for group in grouped.series {
                let gx: f64 = group.tags.get("grid_x").and_then(|x| x.parse().ok()).unwrap_or(0.0);
                let gy: f64 = group.tags.get("grid_y").and_then(|x| x.parse().ok()).unwrap_or(0.0);
                if let Some(values) = group.values.as_array().and_then(|v| v.first()) {
                    if let Some(val) = values.get(1).and_then(|x| x.as_f64()) {
                        data.push((gx, gy, val));
                    }
                }
            }
        }
        Ok(data)
    }

    pub async fn query_corrosion_grid(
        &self,
        hours: i64,
    ) -> Result<Vec<(f64, f64, f64, f64, f64)>, Box<dyn std::error::Error>> {
        let since = Utc::now() - Duration::hours(hours);
        let query_str = format!(
            "SELECT MEAN(corrosion_depth_um), MEAN(collagen_deg_percent), MEAN(ca_p_ratio) \
             FROM corrosion_analysis WHERE time >= '{}' GROUP BY relic_id, grid_x, grid_y",
            since.to_rfc3339()
        );
        let query = ReadQuery::new(query_str);
        let result = self.client.json_query(query).await?;
        let mut data = Vec::new();
        if let Ok(grouped) = result.deserialize_next::<serde_json::Value>() {
            for group in grouped.series {
                let gx: f64 = group.tags.get("grid_x").and_then(|x| x.parse().ok()).unwrap_or(0.0);
                let gy: f64 = group.tags.get("grid_y").and_then(|x| x.parse().ok()).unwrap_or(0.0);
                if let Some(values) = group.values.as_array().and_then(|v| v.first()) {
                    let depth = values.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0);
                    let collagen = values.get(2).and_then(|x| x.as_f64()).unwrap_or(0.0);
                    let cap = values.get(3).and_then(|x| x.as_f64()).unwrap_or(0.0);
                    data.push((gx, gy, depth, collagen, cap));
                }
            }
        }
        Ok(data)
    }

    pub fn get_relics(&self) -> Vec<RelicInfo> {
        self.relics.read().values().cloned().collect()
    }

    pub fn get_sensors(&self) -> Vec<SensorInfo> {
        self.sensors.read().values().cloned().collect()
    }

    pub fn get_sensor(&self, id: &str) -> Option<SensorInfo> {
        self.sensors.read().get(id).cloned()
    }

    pub fn get_relic(&self, id: &str) -> Option<RelicInfo> {
        self.relics.read().get(id).cloned()
    }

    pub async fn ping(&self) -> bool {
        match self.client.ping().await {
            Ok(_) => true,
            Err(e) => {
                warn!("InfluxDB ping失败: {:?}", e);
                false
            }
        }
    }
}
