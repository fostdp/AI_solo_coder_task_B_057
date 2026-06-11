use crate::config::AppConfig;
use crate::database::Database;
use crate::lora_gateway::LoraGateway;
use crate::models::SensorReading;
use crate::services::ServiceMessage;
use log::{info, warn, error};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct LoraIngestService {
    config: AppConfig,
    db: Database,
    gateway: Arc<Mutex<LoraGateway>>,
    tx: mpsc::Sender<ServiceMessage>,
}

impl LoraIngestService {
    pub fn new(
        config: AppConfig,
        db: Database,
        gateway: LoraGateway,
        tx: mpsc::Sender<ServiceMessage>,
    ) -> Self {
        Self {
            config,
            db,
            gateway: Arc::new(Mutex::new(gateway)),
            tx,
        }
    }

    pub fn gateway(&self) -> Arc<Mutex<LoraGateway>> {
        self.gateway.clone()
    }

    pub fn db(&self) -> Database {
        self.db.clone()
    }

    pub async fn ingest_reading(&self, reading: SensorReading) -> Result<(), String> {
        let sensor_id = reading.sensor_id.clone();
        let sensor_type = reading.sensor_type.to_string();

        if let Err(e) = self.db.write_sensor_reading(&reading).await {
            error!("[LoraIngest] 写入InfluxDB失败 {}: {:?}", sensor_id, e);
            return Err(format!("数据库写入失败: {:?}", e));
        }

        match self.tx.send(ServiceMessage::Reading(reading)).await {
            Ok(_) => {
                info!("[LoraIngest] 已投递 {} ({}) 到分析管道", sensor_id, sensor_type);
                Ok(())
            }
            Err(e) => {
                error!("[LoraIngest] 投递到分析管道失败: {}", e);
                Err(format!("分析管道投递失败: {}", e))
            }
        }
    }

    pub async fn ingest_batch(&self, readings: &[SensorReading]) -> Result<usize, String> {
        let mut success = 0usize;
        for reading in readings {
            match self.ingest_reading(reading.clone()).await {
                Ok(_) => success += 1,
                Err(e) => {
                    warn!("[LoraIngest] 批量处理失败 {}: {}", reading.sensor_id, e);
                }
            }
        }
        Ok(success)
    }

    pub async fn run(&self) {
        info!("[LoraIngest] 服务启动");
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            let gw = self.gateway.lock().await;
            let stats = gw.get_stats();
            info!(
                "[LoraIngest] 心跳 | 待发下行: {} | 已发送: {} | 已确认: {}",
                stats.pending_count, stats.total_sent, stats.total_acked
            );
        }
    }
}
