use crate::config::AppConfig;
use crate::models::{Alert, AlertStatus, AlertType, CorrosionAnalysis};
use log::{info, warn, error, debug};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

#[derive(Clone)]
pub struct AlerterService {
    config: AppConfig,
    rx: Arc<Mutex<mpsc::Receiver<CorrosionAnalysis>>>,
    cooldowns: Arc<Mutex<HashMap<String, chrono::DateTime<chrono::Utc>>>>,
    alerts: Arc<Mutex<Vec<Alert>>>,
}

impl AlerterService {
    pub fn new(config: AppConfig, rx: mpsc::Receiver<CorrosionAnalysis>) -> Self {
        Self {
            config,
            rx: Arc::new(Mutex::new(rx)),
            cooldowns: Arc::new(Mutex::new(HashMap::new())),
            alerts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn recent_alerts(&self) -> Vec<Alert> {
        let alerts = self.alerts.lock().await;
        alerts.clone()
    }

    fn is_in_cooldown(&self, key: &str, now: chrono::DateTime<chrono::Utc>) -> bool {
        let cooldowns = self.cooldowns.try_lock();
        match cooldowns {
            Ok(map) => {
                if let Some(last) = map.get(key) {
                    let elapsed = (now - *last).num_seconds() as u64;
                    elapsed < self.config.alerts.cooldown_seconds
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }

    async fn mark_cooldown(&self, key: String, ts: chrono::DateTime<chrono::Utc>) {
        let mut cooldowns = self.cooldowns.lock().await;
        cooldowns.insert(key, ts);
    }

    fn check_thresholds(&self, analysis: &CorrosionAnalysis) -> Vec<AlertType> {
        let mut triggered = Vec::new();
        let cfg = &self.config.alerts;

        if analysis.ph < cfg.ph_low_threshold {
            triggered.push(AlertType::PH_LOW);
        }
        if analysis.ca_concentration > cfg.ca_high_threshold {
            triggered.push(AlertType::CA_HIGH);
        }
        if analysis.temperature > cfg.temp_high_threshold {
            triggered.push(AlertType::TEMP_HIGH);
        }
        if analysis.orp < cfg.orp_low_threshold || analysis.orp > cfg.orp_high_threshold {
            triggered.push(AlertType::ORP_ABNORMAL);
        }
        if analysis.risk_level == "高" || analysis.risk_level == "极高" {
            triggered.push(AlertType::CORROSION_RISK);
        }

        triggered
    }

    async fn create_alert(
        &self,
        analysis: &CorrosionAnalysis,
        alert_type: AlertType,
    ) -> Alert {
        let now = chrono::Utc::now();
        let (threshold, actual, message) = match alert_type {
            AlertType::PH_LOW => (
                self.config.alerts.ph_low_threshold,
                analysis.ph,
                format!("pH过低告警: {:.2} < {:.1}", analysis.ph, self.config.alerts.ph_low_threshold),
            ),
            AlertType::CA_HIGH => (
                self.config.alerts.ca_high_threshold,
                analysis.ca_concentration,
                format!("钙离子浓度过高: {:.1} ppm > {:.0} ppm", analysis.ca_concentration, self.config.alerts.ca_high_threshold),
            ),
            AlertType::TEMP_HIGH => (
                self.config.alerts.temp_high_threshold,
                analysis.temperature,
                format!("温度过高: {:.1}°C > {:.0}°C", analysis.temperature, self.config.alerts.temp_high_threshold),
            ),
            AlertType::ORP_ABNORMAL => (
                0.0,
                analysis.orp,
                format!("氧化还原电位异常: {:.0} mV", analysis.orp),
            ),
            AlertType::CORROSION_RISK => (
                0.0,
                analysis.corrosion_depth_um,
                format!("腐蚀风险{}: 深度 {:.2} um", analysis.risk_level, analysis.corrosion_depth_um),
            ),
        };

        let alert_id = format!("alert_{}_{}", alert_type as u32, now.timestamp_millis());

        Alert {
            id: alert_id,
            alert_type,
            sensor_id: analysis.relic_id.clone(),
            relic_id: Some(analysis.relic_id.clone()),
            threshold,
            actual_value: actual,
            message,
            channels: vec!["console".to_string(), "dingtalk".to_string()],
            status: AlertStatus::PENDING,
            created_at: now,
            resolved_at: None,
        }
    }

    async fn push_alert(&self, alert: &Alert) {
        info!(
            "[Alerter] 🔔 {} | {} | 文物: {}",
            alert.alert_type as u32,
            alert.message,
            alert.relic_id.as_deref().unwrap_or("-")
        );

        if let Some(webhook) = &self.config.alerts.dingtalk_webhook {
            debug!("[Alerter] 推送到钉钉: {}", webhook);
        }
        if let Some(sms_url) = &self.config.alerts.sms_api_url {
            debug!("[Alerter] 推送到短信: {}", sms_url);
        }
    }

    pub async fn run(&self) {
        info!(
            "[Alerter] 服务启动 | pH<{:.1} | Ca>{:.0}ppm | 冷却 {}s",
            self.config.alerts.ph_low_threshold,
            self.config.alerts.ca_high_threshold,
            self.config.alerts.cooldown_seconds
        );

        let mut rx = self.rx.lock().await;

        while let Some(analysis) = rx.recv().await {
            let triggered = self.check_thresholds(&analysis);

            for alert_type in triggered {
                let cooldown_key = format!("{}:{}", alert_type as u32, analysis.relic_id);
                let now = chrono::Utc::now();

                if self.is_in_cooldown(&cooldown_key, now) {
                    debug!("[Alerter] {} 冷却中，跳过", cooldown_key);
                    continue;
                }

                let alert = self.create_alert(&analysis, alert_type).await;
                self.push_alert(&alert).await;

                let mut alert_mut = alert.clone();
                alert_mut.status = AlertStatus::SENT;

                {
                    let mut alerts = self.alerts.lock().await;
                    alerts.push(alert_mut);
                    if alerts.len() > 500 {
                        alerts.drain(0..alerts.len() - 500);
                    }
                }

                self.mark_cooldown(cooldown_key, now).await;
            }
        }

        info!("[Alerter] 管道关闭，退出");
    }
}
