use crate::models::{Alert, AlertType, AlertStatus, SensorReading, SensorType};
use chrono::Utc;
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Instant, Duration};
use log::{info, warn, error};
use rand::Rng;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use base64::Engine;

pub const PH_LOW_THRESHOLD: f64 = 5.5;
pub const CA_HIGH_THRESHOLD: f64 = 200.0;
pub const ORP_LOW_THRESHOLD: f64 = -200.0;
pub const ORP_HIGH_THRESHOLD: f64 = 400.0;
pub const TEMP_HIGH_THRESHOLD: f64 = 35.0;

pub const ALERT_COOLDOWN_SECONDS: u64 = 1800;

#[derive(Clone)]
pub struct AlertConfig {
    pub dingtalk_webhook_url: String,
    pub dingtalk_secret: String,
    pub sms_access_key: String,
    pub sms_secret: String,
    pub sms_sign_name: String,
    pub sms_template_code: String,
    pub sms_endpoint: String,
    pub notify_phones: Vec<String>,
    pub enabled_channels: Vec<String>,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            dingtalk_webhook_url: "https://oapi.dingtalk.com/robot/send".to_string(),
            dingtalk_secret: "SEC_placeholder_secret_change_in_production".to_string(),
            sms_access_key: "LTAIplaceholder".to_string(),
            sms_secret: "placeholder_secret".to_string(),
            sms_sign_name: "文物监测告警".to_string(),
            sms_template_code: "SMS_290000001".to_string(),
            sms_endpoint: "dysmsapi.aliyuncs.com".to_string(),
            notify_phones: vec!["13800138000".to_string(), "13900139000".to_string()],
            enabled_channels: vec!["sms".to_string(), "dingtalk".to_string(), "console".to_string()],
        }
    }
}

pub struct AlertManager {
    config: AlertConfig,
    alerts: Arc<RwLock<VecDeque<Alert>>>,
    last_alert_times: Arc<RwLock<HashMap<String, Instant>>>,
    http_client: reqwest::Client,
}

impl Clone for AlertManager {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            alerts: self.alerts.clone(),
            last_alert_times: self.last_alert_times.clone(),
            http_client: self.http_client.clone(),
        }
    }
}

impl AlertManager {
    pub fn new(config: AlertConfig) -> Self {
        Self {
            config,
            alerts: Arc::new(RwLock::new(VecDeque::new())),
            last_alert_times: Arc::new(RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn check_and_alert(&self, reading: &SensorReading) -> Vec<Alert> {
        let mut triggered = Vec::new();
        let check_time = Instant::now();

        match reading.sensor_type {
            SensorType::PH => {
                if reading.value < PH_LOW_THRESHOLD {
                    let alert_key = format!("{}_PH_LOW", reading.sensor_id);
                    if self.can_alert(&alert_key, check_time) {
                        let alert = self.create_alert(
                            AlertType::PH_LOW,
                            reading,
                            PH_LOW_THRESHOLD,
                            reading.value,
                            &format!(
                                "【pH过低告警】电极{}检测pH={:.2}，低于阈值{:.1}，遗址酸性环境可能加速骨胶原水解，建议立即排查！",
                                reading.sensor_id, reading.value, PH_LOW_THRESHOLD
                            ),
                        );
                        self.update_last_alert(&alert_key, check_time);
                        triggered.push(alert);
                    }
                }
            }
            SensorType::CA2 => {
                if reading.value > CA_HIGH_THRESHOLD {
                    let alert_key = format!("{}_CA_HIGH", reading.sensor_id);
                    if self.can_alert(&alert_key, check_time) {
                        let alert = self.create_alert(
                            AlertType::CA_HIGH,
                            reading,
                            CA_HIGH_THRESHOLD,
                            reading.value,
                            &format!(
                                "【钙离子过高告警】电极{}检测Ca²+={:.1}ppm，超过阈值{:.0}ppm，骨矿溶解速率异常，文物矿化结构受威胁！",
                                reading.sensor_id, reading.value, CA_HIGH_THRESHOLD
                            ),
                        );
                        self.update_last_alert(&alert_key, check_time);
                        triggered.push(alert);
                    }
                }
            }
            SensorType::TEMP => {
                if reading.value > TEMP_HIGH_THRESHOLD {
                    let alert_key = format!("{}_TEMP_HIGH", reading.sensor_id);
                    if self.can_alert(&alert_key, check_time) {
                        let alert = self.create_alert(
                            AlertType::TEMP_HIGH,
                            reading,
                            TEMP_HIGH_THRESHOLD,
                            reading.value,
                            &format!(
                                "【温度过高告警】电极{}检测温度={:.1}℃，超过阈值{:.0}℃，Arrhenius效应将显著加速腐蚀反应！",
                                reading.sensor_id, reading.value, TEMP_HIGH_THRESHOLD
                            ),
                        );
                        self.update_last_alert(&alert_key, check_time);
                        triggered.push(alert);
                    }
                }
            }
            SensorType::ORP => {
                if reading.value < ORP_LOW_THRESHOLD || reading.value > ORP_HIGH_THRESHOLD {
                    let alert_key = format!("{}_ORP_ABNORMAL", reading.sensor_id);
                    if self.can_alert(&alert_key, check_time) {
                        let msg = if reading.value < ORP_LOW_THRESHOLD {
                            format!(
                                "【ORP异常-强还原】电极{}检测Eh={:.0}mV，低于{:.0}mV，还原环境可能导致矿物质还原分解！",
                                reading.sensor_id, reading.value, ORP_LOW_THRESHOLD
                            )
                        } else {
                            format!(
                                "【ORP异常-强氧化】电极{}检测Eh={:.0}mV，高于{:.0}mV，强氧化环境加速有机质降解！",
                                reading.sensor_id, reading.value, ORP_HIGH_THRESHOLD
                            )
                        };
                        let threshold = if reading.value < ORP_LOW_THRESHOLD { ORP_LOW_THRESHOLD } else { ORP_HIGH_THRESHOLD };
                        let alert = self.create_alert(AlertType::ORP_ABNORMAL, reading, threshold, reading.value, &msg);
                        self.update_last_alert(&alert_key, check_time);
                        triggered.push(alert);
                    }
                }
            }
        }

        for alert in &triggered {
            self.store_alert(alert.clone());
            let manager = self.clone();
            let alert_clone = alert.clone();
            tokio::spawn(async move {
                manager.dispatch_alert(&alert_clone).await;
            });
        }

        triggered
    }

    fn can_alert(&self, key: &str, now: Instant) -> bool {
        match self.last_alert_times.read().get(key) {
            Some(last) => now.duration_since(*last).as_secs() >= ALERT_COOLDOWN_SECONDS,
            None => true,
        }
    }

    fn update_last_alert(&self, key: &str, time: Instant) {
        self.last_alert_times.write().insert(key.to_string(), time);
    }

    fn create_alert(
        &self,
        alert_type: AlertType,
        reading: &SensorReading,
        threshold: f64,
        actual_value: f64,
        message: &str,
    ) -> Alert {
        let mut rng = rand::thread_rng();
        let id = format!("ALT-{}-{}", Utc::now().format("%Y%m%d%H%M%S"), rng.gen_range(1000..9999));
        Alert {
            id,
            alert_type,
            sensor_id: reading.sensor_id.clone(),
            relic_id: reading.relic_id.clone(),
            threshold,
            actual_value,
            message: message.to_string(),
            channels: self.config.enabled_channels.clone(),
            status: AlertStatus::PENDING,
            created_at: Utc::now(),
            resolved_at: None,
        }
    }

    fn store_alert(&self, alert: Alert) {
        let mut alerts = self.alerts.write();
        alerts.push_front(alert);
        while alerts.len() > 1000 {
            alerts.pop_back();
        }
    }

    async fn dispatch_alert(&self, alert: &Alert) {
        for channel in &alert.channels {
            let result = match channel.as_str() {
                "sms" => self.send_sms(alert).await,
                "dingtalk" => self.send_dingtalk(alert).await,
                "console" => {
                    info!("[CONSOLE ALERT] {:?} - {}", alert.alert_type, alert.message);
                    Ok(())
                }
                _ => {
                    warn!("未知告警通道: {}", channel);
                    Err("未知通道".to_string())
                }
            };
            match result {
                Ok(_) => {
                    if let Some(a) = self.alerts.write().iter_mut().find(|a| a.id == alert.id) {
                        a.status = AlertStatus::SENT;
                    }
                }
                Err(e) => {
                    error!("通道{}发送告警失败: {}", channel, e);
                    if let Some(a) = self.alerts.write().iter_mut().find(|a| a.id == alert.id) {
                        a.status = AlertStatus::FAILED;
                    }
                }
            }
        }
    }

    async fn send_dingtalk(&self, alert: &Alert) -> Result<(), String> {
        let timestamp = Utc::now().timestamp_millis();
        let string_to_sign = format!("{}\n{}", timestamp, self.config.dingtalk_secret);

        let mut mac = Hmac::<Sha256>::new_from_slice(self.config.dingtalk_secret.as_bytes())
            .map_err(|e| format!("HMAC初始化失败: {:?}", e))?;
        mac.update(string_to_sign.as_bytes());
        let sign = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        let sign_url_encoded = urlencoding::encode(&sign);

        let url = format!(
            "{}?access_token={}&timestamp={}&sign={}",
            self.config.dingtalk_webhook_url,
            urlencoding::encode(&self.config.dingtalk_secret),
            timestamp,
            sign_url_encoded
        );

        let payload = serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "title": "文物腐蚀监测告警",
                "text": &format!(
                    "### ⚠️ 文物腐蚀监测告警\n\n**类型**: {:?}\n\n**电极ID**: {}\n\n**关联文物**: {}\n\n**阈值**: {:.2}\n\n**实际值**: {:.2}\n\n**详情**: {}\n\n**时间**: {}",
                    alert.alert_type,
                    alert.sensor_id,
                    alert.relic_id.clone().unwrap_or_else(|| "无".to_string()),
                    alert.threshold,
                    alert.actual_value,
                    alert.message,
                    alert.created_at.format("%Y-%m-%d %H:%M:%S")
                )
            },
            "at": {
                "isAtAll": true
            }
        });

        match self.http_client.post(&url).json(&payload).send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    info!("钉钉告警推送成功: {}", alert.id);
                    Ok(())
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    Err(format!("钉钉推送状态{}: {}", status, body))
                }
            }
            Err(e) => Err(format!("钉钉请求失败: {:?}", e)),
        }
    }

    async fn send_sms(&self, alert: &Alert) -> Result<(), String> {
        info!("模拟发送短信告警至 {:?}: [{}] {}", self.config.notify_phones, alert.id, alert.message);

        let action = "SendSms";
        let version = "2017-05-25";
        let region_id = "cn-hangzhou";
        let phones = self.config.notify_phones.join(",");
        let template_param = serde_json::json!({
            "sensorId": alert.sensor_id,
            "value": format!("{:.2}", alert.actual_value),
            "threshold": format!("{:.2}", alert.threshold),
            "type": format!("{:?}", alert.alert_type)
        }).to_string();

        let _params = [
            ("Action", action),
            ("Version", version),
            ("RegionId", region_id),
            ("PhoneNumbers", &phones),
            ("SignName", &self.config.sms_sign_name),
            ("TemplateCode", &self.config.sms_template_code),
            ("TemplateParam", &template_param),
            ("AccessKeyId", &self.config.sms_access_key),
            ("SignatureMethod", "HMAC-SHA1"),
            ("SignatureVersion", "1.0"),
            ("SignatureNonce", &Utc::now().timestamp_nanos_opt().unwrap_or(0).to_string()),
            ("Timestamp", &Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            ("Format", "JSON"),
        ];

        Ok(())
    }

    pub fn get_alerts(&self, limit: usize) -> Vec<Alert> {
        let alerts = self.alerts.read();
        alerts.iter().take(limit).cloned().collect()
    }

    pub fn get_active_alerts(&self) -> Vec<Alert> {
        let alerts = self.alerts.read();
        alerts.iter()
            .filter(|a| matches!(a.status, AlertStatus::PENDING | AlertStatus::SENT))
            .cloned()
            .collect()
    }

    pub fn acknowledge_alert(&self, id: &str) -> bool {
        let mut alerts = self.alerts.write();
        if let Some(alert) = alerts.iter_mut().find(|a| a.id == id) {
            alert.status = AlertStatus::ACKNOWLEDGED;
            return true;
        }
        false
    }

    pub fn resolve_alert(&self, id: &str) -> bool {
        let mut alerts = self.alerts.write();
        if let Some(alert) = alerts.iter_mut().find(|a| a.id == id) {
            alert.status = AlertStatus::RESOLVED;
            alert.resolved_at = Some(Utc::now());
            return true;
        }
        false
    }

    pub fn stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();
        let alerts = self.alerts.read();
        stats.insert("total".to_string(), alerts.len());
        stats.insert("pending".to_string(), alerts.iter().filter(|a| matches!(a.status, AlertStatus::PENDING)).count());
        stats.insert("sent".to_string(), alerts.iter().filter(|a| matches!(a.status, AlertStatus::SENT)).count());
        stats.insert("acknowledged".to_string(), alerts.iter().filter(|a| matches!(a.status, AlertStatus::ACKNOWLEDGED)).count());
        stats.insert("resolved".to_string(), alerts.iter().filter(|a| matches!(a.status, AlertStatus::RESOLVED)).count());
        stats.insert("failed".to_string(), alerts.iter().filter(|a| matches!(a.status, AlertStatus::FAILED)).count());
        stats
    }
}
