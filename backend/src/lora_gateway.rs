use serde::{Deserialize, Serialize};
use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Instant, Duration};
use log::{info, warn, debug, error};

pub const DEFAULT_MAX_RETRIES: u8 = 5;
pub const DEFAULT_RETRANSMIT_DELAY_MS: u64 = 5000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum DownlinkCommand {
    SET_SAMPLE_INTERVAL,
    SET_THRESHOLDS,
    SET_TX_POWER,
    SET_DATARATE,
    RESET_DEVICE,
    CALIBRATE,
    FIRMWARE_UPDATE,
    CONFIG_ACK,
    QUERY_STATUS,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownlinkFrame {
    pub dev_eui: String,
    pub fcnt: u32,
    pub port: u8,
    pub command: DownlinkCommand,
    pub payload: serde_json::Value,
    pub ack_required: bool,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckFrame {
    pub dev_eui: String,
    pub fcnt: u32,
    pub ack_fcnt: u32,
    pub port: u8,
    pub success: bool,
    pub result: Option<String>,
    pub rssi: Option<i32>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone)]
struct PendingDownlink {
    frame: DownlinkFrame,
    retries_remaining: u8,
    next_retry_at: Instant,
    sent_at: Instant,
    acked: bool,
    failed: bool,
    last_error: Option<String>,
    created_at: Instant,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownlinkStats {
    pub total_sent: u64,
    pub total_acked: u64,
    pub total_failed: u64,
    pub total_retries: u64,
    pub pending_count: usize,
    pub success_rate: f64,
    pub avg_retries_per_msg: f64,
}

impl Default for DownlinkStats {
    fn default() -> Self {
        Self {
            total_sent: 0,
            total_acked: 0,
            total_failed: 0,
            total_retries: 0,
            pending_count: 0,
            success_rate: 1.0,
            avg_retries_per_msg: 0.0,
        }
    }
}

pub struct LoraGateway {
    pending: Arc<RwLock<HashMap<String, VecDeque<PendingDownlink>>>>,
    fcnt_counter: Arc<RwLock<HashMap<String, u32>>>,
    max_retries: u8,
    retransmit_delay: Duration,
    stats: Arc<RwLock<DownlinkStats>>,
    history: Arc<RwLock<VecDeque<(String, u32, bool, Instant)>>>,
}

impl Clone for LoraGateway {
    fn clone(&self) -> Self {
        Self {
            pending: self.pending.clone(),
            fcnt_counter: self.fcnt_counter.clone(),
            max_retries: self.max_retries,
            retransmit_delay: self.retransmit_delay,
            stats: self.stats.clone(),
            history: self.history.clone(),
        }
    }
}

impl LoraGateway {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
            fcnt_counter: Arc::new(RwLock::new(HashMap::new())),
            max_retries: DEFAULT_MAX_RETRIES,
            retransmit_delay: Duration::from_millis(DEFAULT_RETRANSMIT_DELAY_MS),
            stats: Arc::new(RwLock::new(DownlinkStats::default())),
            history: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    pub fn enqueue(&self, dev_eui: &str, command: DownlinkCommand, payload: serde_json::Value) -> u32 {
        let fcnt = self.next_fcnt(dev_eui);

        let frame = DownlinkFrame {
            dev_eui: dev_eui.to_string(),
            fcnt,
            port: 100,
            command,
            payload,
            ack_required: true,
            timestamp: None,
        };

        let pending = PendingDownlink {
            frame,
            retries_remaining: self.max_retries,
            next_retry_at: Instant::now(),
            sent_at: Instant::now(),
            acked: false,
            failed: false,
            last_error: None,
            created_at: Instant::now(),
        };

        self.pending.write()
            .entry(dev_eui.to_string())
            .or_insert_with(VecDeque::new)
            .push_back(pending);

        info!("LoRa下行入队: dev_eui={}, fcnt={}, command={:?}", dev_eui, fcnt, command);
        fcnt
    }

    pub fn process_ack(&self, ack: &AckFrame) -> bool {
        let mut pending = self.pending.write();
        let dev_eui = &ack.dev_eui;

        if let Some(queue) = pending.get_mut(dev_eui) {
            if let Some(idx) = queue.iter().position(|p| p.frame.fcnt == ack.ack_fcnt) {
                if ack.success {
                    queue[idx].acked = true;
                    queue[idx].failed = false;
                    self.stats.write().total_acked += 1;
                    info!("LoRa下行已ACK: dev_eui={}, fcnt={}, 重传{}次",
                        dev_eui, ack.ack_fcnt,
                        self.max_retries - queue[idx].retries_remaining);
                } else {
                    queue[idx].failed = true;
                    queue[idx].last_error = ack.result.clone();
                    self.stats.write().total_failed += 1;
                    warn!("LoRa下行执行失败: dev_eui={}, fcnt={}, error={:?}",
                        dev_eui, ack.ack_fcnt, ack.result);
                }

                self.history.write().push_back((
                    dev_eui.clone(),
                    ack.ack_fcnt,
                    ack.success,
                    Instant::now(),
                ));
                if self.history.read().len() > 1000 {
                    self.history.write().pop_front();
                }

                queue.remove(idx);
                self.update_stats();
                return true;
            }
        }
        warn!("收到未知ACK: dev_eui={}, ack_fcnt={}", dev_eui, ack.ack_fcnt);
        false
    }

    pub fn get_pending_for_device(&self, dev_eui: &str) -> Vec<DownlinkFrame> {
        let mut pending = self.pending.write();
        let mut stats = self.stats.write();
        let mut result = Vec::new();

        if let Some(queue) = pending.get_mut(dev_eui) {
            let now = Instant::now();
            let mut to_remove = Vec::new();

            for (i, p) in queue.iter_mut().enumerate() {
                if p.acked || p.failed { continue; }

                if now >= p.next_retry_at {
                    if p.retries_remaining > 0 {
                        p.sent_at = now;
                        p.next_retry_at = now + self.retransmit_delay;
                        p.retries_remaining -= 1;
                        stats.total_retries += 1;
                        result.push(p.frame.clone());

                        if p.retries_remaining < self.max_retries - 1 {
                            warn!("LoRa下行重传: dev_eui={}, fcnt={}, 剩余{}次",
                                dev_eui, p.frame.fcnt, p.retries_remaining);
                        } else {
                            debug!("LoRa下行首次发送: dev_eui={}, fcnt={}", dev_eui, p.frame.fcnt);
                        }
                        stats.total_sent += 1;
                    } else {
                        p.failed = true;
                        p.last_error = Some("ACK超时，重传次数耗尽".to_string());
                        stats.total_failed += 1;
                        to_remove.push(i);
                        error!("LoRa下行超时失败: dev_eui={}, fcnt={}, 已重传{}次",
                            dev_eui, p.frame.fcnt, self.max_retries);
                    }
                }
            }

            for &i in to_remove.iter().rev() {
                queue.remove(i);
            }
        }

        drop(stats);
        drop(pending);
        self.update_stats();
        result
    }

    pub fn poll_all_pending(&self) -> Vec<DownlinkFrame> {
        let devices: Vec<String> = self.pending.read().keys().cloned().collect();
        let mut all = Vec::new();
        for dev in devices {
            all.extend(self.get_pending_for_device(&dev));
        }
        all
    }

    pub fn get_stats(&self) -> DownlinkStats {
        let stats = self.stats.read();
        let mut s = stats.clone();
        s.pending_count = self.pending_count();
        s
    }

    pub fn pending_count(&self) -> usize {
        self.pending.read().values().map(|q| q.len()).sum()
    }

    pub fn device_pending_count(&self, dev_eui: &str) -> usize {
        self.pending.read().get(dev_eui).map(|q| q.len()).unwrap_or(0)
    }

    fn next_fcnt(&self, dev_eui: &str) -> u32 {
        let mut counter = self.fcnt_counter.write();
        let fcnt = counter.entry(dev_eui.to_string()).or_insert(1);
        let current = *fcnt;
        *fcnt += 1;
        current
    }

    fn update_stats(&self) {
        let mut stats = self.stats.write();
        stats.pending_count = self.pending_count();
        let total = stats.total_acked + stats.total_failed;
        stats.success_rate = if total > 0 {
            stats.total_acked as f64 / total as f64
        } else { 1.0 };
        stats.avg_retries_per_msg = if stats.total_sent > 0 {
            stats.total_retries as f64 / stats.total_sent as f64
        } else { 0.0 };
    }

    pub fn list_pending(&self, limit: usize) -> Vec<serde_json::Value> {
        let pending = self.pending.read();
        let mut result = Vec::new();
        for (dev, queue) in pending.iter() {
            for p in queue.iter().take(limit / 10 + 1) {
                result.push(serde_json::json!({
                    "dev_eui": dev,
                    "fcnt": p.frame.fcnt,
                    "command": format!("{:?}", p.frame.command),
                    "retries_remaining": p.retries_remaining,
                    "next_retry_ms": p.next_retry_at.saturating_duration_since(Instant::now()).as_millis(),
                    "acked": p.acked,
                    "failed": p.failed,
                    "payload": p.frame.payload,
                }));
                if result.len() >= limit { break; }
            }
        }
        result
    }
}

impl Default for LoraGateway {
    fn default() -> Self { Self::new() }
}
