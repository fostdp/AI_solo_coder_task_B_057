use serde::{Serialize, Deserialize};
use std::collections::{HashMap, VecDeque};
use std::time::{Instant, Duration};
use log::{info, warn, debug, error};

pub const DEFAULT_DOWNLINK_PORT: u8 = 100;
pub const DEFAULT_ACK_PORT: u8 = 101;
pub const DEFAULT_RETRANSMIT_DELAY_MS: u64 = 5000;
pub const DEFAULT_MAX_RETRIES: u8 = 5;
pub const DEFAULT_ACK_TIMEOUT_MS: u64 = 8000;
pub const DEFAULT_RX1_DELAY_MS: u64 = 1000;
pub const DEFAULT_RX2_DELAY_MS: u64 = 2000;

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
pub struct PendingDownlink {
    pub frame: DownlinkFrame,
    pub retries_remaining: u8,
    pub next_retry_at: Instant,
    pub sent_at: Instant,
    pub acked: bool,
    pub failed: bool,
    pub last_error: Option<String>,
}

pub struct DownlinkScheduler {
    pending: HashMap<String, VecDeque<PendingDownlink>>,
    max_retries: u8,
    retransmit_delay: Duration,
    ack_timeout: Duration,
    next_fcnt: HashMap<String, u32>,
    sent_history: VecDeque<(String, u32, bool, Instant)>,
    stats: DownlinkStats,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DownlinkStats {
    pub total_sent: u64,
    pub total_acked: u64,
    pub total_failed: u64,
    pub total_retries: u64,
    pub pending_count: usize,
    pub success_rate: f64,
    pub avg_retries_per_msg: f64,
}

impl Default for DownlinkScheduler {
    fn default() -> Self {
        Self::new(
            DEFAULT_MAX_RETRIES,
            Duration::from_millis(DEFAULT_RETRANSMIT_DELAY_MS),
            Duration::from_millis(DEFAULT_ACK_TIMEOUT_MS),
        )
    }
}

impl DownlinkScheduler {
    pub fn new(max_retries: u8, retransmit_delay: Duration, ack_timeout: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            max_retries,
            retransmit_delay,
            ack_timeout,
            next_fcnt: HashMap::new(),
            sent_history: VecDeque::new(),
            stats: DownlinkStats::default(),
        }
    }

    pub fn enqueue(&mut self, dev_eui: &str, command: DownlinkCommand, payload: serde_json::Value) -> u32 {
        let fcnt = self.next_fcnt(dev_eui);
        let frame = DownlinkFrame {
            dev_eui: dev_eui.to_string(),
            fcnt,
            port: DEFAULT_DOWNLINK_PORT,
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
        };

        self.pending
            .entry(dev_eui.to_string())
            .or_insert_with(VecDeque::new)
            .push_back(pending);

        debug!("下行帧入队: dev_eui={}, fcnt={}, cmd={:?}", dev_eui, fcnt, command);
        fcnt
    }

    pub fn process_ack(&mut self, ack: &AckFrame) -> bool {
        let dev_eui = &ack.dev_eui;
        let ack_fcnt = ack.ack_fcnt;

        if let Some(queue) = self.pending.get_mut(dev_eui) {
            if let Some(idx) = queue.iter().position(|p| p.frame.fcnt == ack_fcnt) {
                if ack.success {
                    queue[idx].acked = true;
                    queue[idx].failed = false;
                    self.stats.total_acked += 1;
                    info!("下行帧已确认: dev_eui={}, fcnt={}, 重传次数={}",
                        dev_eui, ack_fcnt,
                        self.max_retries - queue[idx].retries_remaining);
                } else {
                    queue[idx].failed = true;
                    queue[idx].last_error = ack.result.clone();
                    self.stats.total_failed += 1;
                    warn!("下行帧执行失败: dev_eui={}, fcnt={}, 错误={:?}",
                        dev_eui, ack_fcnt, ack.result);
                }
                self.sent_history.push_back((
                    dev_eui.clone(),
                    ack_fcnt,
                    ack.success,
                    Instant::now(),
                ));
                if self.sent_history.len() > 1000 {
                    self.sent_history.pop_front();
                }
                queue.remove(idx);
                self.update_stats();
                return true;
            }
        }
        warn!("收到未知ACK: dev_eui={}, ack_fcnt={}", dev_eui, ack_fcnt);
        false
    }

    pub fn tick(&mut self, now: Instant) -> Vec<DownlinkFrame> {
        let mut to_send = Vec::new();

        for queue in self.pending.values_mut() {
            for pending in queue.iter_mut() {
                if !pending.acked && !pending.failed && now >= pending.next_retry_at {
                    if pending.retries_remaining > 0 {
                        pending.sent_at = now;
                        pending.next_retry_at = now + self.retransmit_delay;
                        pending.retries_remaining -= 1;
                        self.stats.total_retries += 1;
                        to_send.push(pending.frame.clone());

                        if pending.retries_remaining < self.max_retries {
                            warn!("重传下行帧: dev_eui={}, fcnt={}, 剩余重传={}",
                                pending.frame.dev_eui, pending.frame.fcnt, pending.retries_remaining);
                        } else {
                            debug!("首次发送下行帧: dev_eui={}, fcnt={}",
                                pending.frame.dev_eui, pending.frame.fcnt);
                        }
                        self.stats.total_sent += 1;
                    } else {
                        pending.failed = true;
                        pending.last_error = Some("超时重传次数耗尽".to_string());
                        self.stats.total_failed += 1;
                        error!("下行帧超时失败: dev_eui={}, fcnt={}, 已重传{}次",
                            pending.frame.dev_eui, pending.frame.fcnt, self.max_retries);
                    }
                }
            }

            queue.retain(|p| !p.failed);
        }

        self.update_stats();
        to_send
    }

    pub fn get_stats(&self) -> DownlinkStats {
        self.stats.clone()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.values().map(|q| q.len()).sum()
    }

    fn next_fcnt(&mut self, dev_eui: &str) -> u32 {
        let fcnt = self.next_fcnt.entry(dev_eui.to_string()).or_insert(1);
        let current = *fcnt;
        *fcnt += 1;
        current
    }

    fn update_stats(&mut self) {
        self.stats.pending_count = self.pending_count();
        let total = self.stats.total_acked + self.stats.total_failed;
        self.stats.success_rate = if total > 0 {
            self.stats.total_acked as f64 / total as f64
        } else { 1.0 };
        self.stats.avg_retries_per_msg = if self.stats.total_sent > 0 {
            self.stats.total_retries as f64 / self.stats.total_sent as f64
        } else { 0.0 };
    }

    pub fn clear_expired(&mut self, max_age: Duration) {
        let now = Instant::now();
        self.sent_history.retain(|(_, _, _, t)| now.duration_since(*t) < max_age);
    }
}

pub struct DeviceDownlinkHandler {
    dev_eui: String,
    last_fcnt_seen: u32,
    pending_acks: VecDeque<AckFrame>,
    rx_window_open: bool,
    rx1_remaining_ms: u64,
    rx2_remaining_ms: u64,
    received_commands: Vec<(u32, DownlinkCommand, serde_json::Value)>,
}

impl DeviceDownlinkHandler {
    pub fn new(dev_eui: String) -> Self {
        Self {
            dev_eui,
            last_fcnt_seen: 0,
            pending_acks: VecDeque::new(),
            rx_window_open: false,
            rx1_remaining_ms: 0,
            rx2_remaining_ms: 0,
            received_commands: Vec::new(),
        }
    }

    pub fn open_rx_windows(&mut self, rx1_duration_ms: u64, rx2_duration_ms: u64) {
        self.rx_window_open = true;
        self.rx1_remaining_ms = rx1_duration_ms;
        self.rx2_remaining_ms = rx2_duration_ms;
        debug!("设备 {} 打开接收窗口 RX1={}ms RX2={}ms",
            self.dev_eui, rx1_duration_ms, rx2_duration_ms);
    }

    pub fn close_rx_windows(&mut self) {
        self.rx_window_open = false;
        debug!("设备 {} 关闭接收窗口", self.dev_eui);
    }

    pub fn receive_downlink(&mut self, frame: &DownlinkFrame, loss_probability: f64) -> bool {
        if !self.rx_window_open {
            warn!("设备 {} 在接收窗口外收到下行帧 fcnt={}，丢弃",
                self.dev_eui, frame.fcnt);
            return false;
        }

        use rand::Rng;
        let mut rng = rand::thread_rng();
        if rng.gen::<f64>() < loss_probability {
            warn!("设备 {} 下行帧 fcnt={} 丢失 (模拟信道丢包率{:.0}%)",
                self.dev_eui, frame.fcnt, loss_probability * 100.0);
            return false;
        }

        if frame.fcnt <= self.last_fcnt_seen && self.last_fcnt_seen > 0 {
            warn!("设备 {} 收到重复帧 fcnt={} (已处理至{})，去重丢弃",
                self.dev_eui, frame.fcnt, self.last_fcnt_seen);
            return true;
        }

        self.last_fcnt_seen = frame.fcnt;
        self.received_commands.push((frame.fcnt, frame.command, frame.payload.clone()));

        if frame.ack_required {
            let ack = AckFrame {
                dev_eui: self.dev_eui.clone(),
                fcnt: self.last_fcnt_seen,
                ack_fcnt: frame.fcnt,
                port: DEFAULT_ACK_PORT,
                success: true,
                result: Some("OK".to_string()),
                rssi: Some(-85 - rng.gen_range(0..20)),
                timestamp: None,
            };
            self.pending_acks.push_back(ack);
            debug!("设备 {} 处理下行 fcnt={} 命令={:?}，生成ACK",
                self.dev_eui, frame.fcnt, frame.command);
        }
        true
    }

    pub fn pop_ack(&mut self) -> Option<AckFrame> {
        self.pending_acks.pop_front()
    }

    pub fn ack_count(&self) -> usize {
        self.pending_acks.len()
    }

    pub fn pop_command(&mut self) -> Option<(u32, DownlinkCommand, serde_json::Value)> {
        if self.received_commands.is_empty() { None } else { self.received_commands.pop() }
    }

    pub fn rx_window_tick(&mut self, elapsed_ms: u64) -> bool {
        if !self.rx_window_open { return false; }
        if self.rx1_remaining_ms > 0 {
            self.rx1_remaining_ms = self.rx1_remaining_ms.saturating_sub(elapsed_ms);
        }
        if self.rx2_remaining_ms > 0 {
            self.rx2_remaining_ms = self.rx2_remaining_ms.saturating_sub(elapsed_ms);
        }
        if self.rx1_remaining_ms == 0 && self.rx2_remaining_ms == 0 {
            self.rx_window_open = false;
            debug!("设备 {} 接收窗口超时关闭", self.dev_eui);
            false
        } else {
            true
        }
    }

    pub fn is_rx_open(&self) -> bool {
        self.rx_window_open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enqueue_and_ack() {
        let mut sched = DownlinkScheduler::default();
        let fcnt = sched.enqueue("test-001", DownlinkCommand::SET_SAMPLE_INTERVAL,
            serde_json::json!({"interval": 1800}));
        assert_eq!(fcnt, 1);
        assert_eq!(sched.pending_count(), 1);

        let frames = sched.tick(Instant::now());
        assert_eq!(frames.len(), 1);

        let ack = AckFrame {
            dev_eui: "test-001".to_string(),
            fcnt: 1,
            ack_fcnt: 1,
            port: 101,
            success: true,
            result: None,
            rssi: None,
            timestamp: None,
        };
        assert!(sched.process_ack(&ack));
        assert_eq!(sched.pending_count(), 0);
        assert_eq!(sched.get_stats().total_acked, 1);
    }

    #[test]
    fn test_retransmit_and_failure() {
        let mut sched = DownlinkScheduler::new(
            2,
            Duration::from_millis(10),
            Duration::from_millis(5),
        );
        sched.enqueue("dev-1", DownlinkCommand::RESET_DEVICE, serde_json::json!({}));

        for _ in 0..5 {
            let frames = sched.tick(Instant::now());
            if frames.is_empty() { break; }
        }
        assert!(sched.get_stats().total_failed > 0);
    }

    #[test]
    fn test_device_handler() {
        let mut handler = DeviceDownlinkHandler::new("dev-001".to_string());
        handler.open_rx_windows(5000, 5000);

        let frame = DownlinkFrame {
            dev_eui: "dev-001".to_string(),
            fcnt: 1,
            port: 100,
            command: DownlinkCommand::SET_SAMPLE_INTERVAL,
            payload: serde_json::json!({"interval": 900}),
            ack_required: true,
            timestamp: None,
        };
        assert!(handler.receive_downlink(&frame, 0.0));
        assert_eq!(handler.ack_count(), 1);

        let ack = handler.pop_ack().unwrap();
        assert_eq!(ack.ack_fcnt, 1);
        assert!(ack.success);
    }

    #[test]
    fn test_duplicate_frame_dedup() {
        let mut handler = DeviceDownlinkHandler::new("dev-001".to_string());
        handler.open_rx_windows(5000, 5000);

        let frame = DownlinkFrame {
            dev_eui: "dev-001".to_string(),
            fcnt: 5,
            port: 100,
            command: DownlinkCommand::SET_THRESHOLDS,
            payload: serde_json::json!({}),
            ack_required: true,
            timestamp: None,
        };
        assert!(handler.receive_downlink(&frame, 0.0));
        assert_eq!(handler.ack_count(), 1);

        assert!(handler.receive_downlink(&frame, 0.0));
        assert_eq!(handler.ack_count(), 1);
    }
}
