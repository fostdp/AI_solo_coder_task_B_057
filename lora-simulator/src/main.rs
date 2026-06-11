use chrono::{DateTime, Utc, Duration};
use clap::Parser;
use log::{info, warn, error, debug};
use rand::Rng;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::thread;
use std::time;

mod lora;

#[derive(Parser, Debug)]
#[command(name = "LoRa电极模拟器", version, about = "模拟pH/ORP/Ca2+电极通过LoRa上报数据")]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    server_url: String,

    #[arg(long, default_value_t = 1800)]
    interval_secs: u64,

    #[arg(long, default_value_t = 50)]
    ph_sensor_count: u32,

    #[arg(long, default_value_t = 50)]
    orp_sensor_count: u32,

    #[arg(long, default_value_t = 30)]
    ca_sensor_count: u32,

    #[arg(long, default_value_t = 1)]
    speed_multiplier: f64,

    #[arg(long, default_value_t = false)]
    batch_mode: bool,

    #[arg(long, default_value_t = 0)]
    total_rounds: u32,

    #[arg(long, default_value_t = 0.1)]
    anomaly_probability: f64,

    #[arg(long, default_value_t = false)]
    only_once: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SensorType {
    #[serde(rename = "pH")]
    PH,
    #[serde(rename = "ORP")]
    ORP,
    #[serde(rename = "Ca2+")]
    CA2,
    #[serde(rename = "Temp")]
    TEMP,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SensorReading {
    sensor_id: String,
    sensor_type: SensorType,
    value: f64,
    relic_id: Option<String>,
    grid_x: f64,
    grid_y: f64,
    depth: f64,
    temperature: Option<f64>,
    timestamp: Option<DateTime<Utc>>,
    battery: Option<f64>,
    rssi: Option<i32>,
}

#[derive(Debug, Clone)]
struct VirtualSensor {
    id: String,
    sensor_type: SensorType,
    grid_x: f64,
    grid_y: f64,
    depth: f64,
    base_value: f64,
    drift_rate: f64,
    noise_std: f64,
    battery: f64,
    accumulated_hours: f64,
    anomaly_mode: bool,
    anomaly_remaining: u32,
    sample_interval_secs: u64,
    tx_power_dbm: i32,
    datarate: String,
}

impl VirtualSensor {
    fn new(id: String, stype: SensorType, gx: f64, gy: f64, depth: f64) -> Self {
        let mut rng = rand::thread_rng();
        let (base, noise, drift) = match stype {
            SensorType::PH => {
                (6.5 + rng.gen_range(-0.8..0.8), 0.08, rng.gen_range(-0.01..0.02))
            }
            SensorType::ORP => {
                (150.0 + rng.gen_range(-100.0..100.0), 15.0, rng.gen_range(-2.0..3.0))
            }
            SensorType::CA2 => {
                (60.0 + rng.gen_range(-20.0..80.0), 8.0, rng.gen_range(-1.0..3.0))
            }
            SensorType::TEMP => {
                (16.0 + rng.gen_range(-3.0..4.0), 0.3, rng.gen_range(-0.05..0.08))
            }
        };
        Self {
            id, sensor_type: stype,
            grid_x: gx, grid_y: gy, depth,
            base_value: base,
            drift_rate: drift,
            noise_std: noise,
            battery: 95.0 + rng.gen_range(0.0..5.0),
            accumulated_hours: 0.0,
            anomaly_mode: false,
            anomaly_remaining: 0,
            sample_interval_secs: 1800,
            tx_power_dbm: 14,
            datarate: "SF7BW125".to_string(),
        }
    }

    fn next_value(&mut self, interval_hours: f64, anomaly_prob: f64) -> f64 {
        let mut rng = rand::thread_rng();

        if self.anomaly_mode && self.anomaly_remaining > 0 {
            self.anomaly_remaining -= 1;
        } else if self.anomaly_mode && self.anomaly_remaining == 0 {
            self.anomaly_mode = false;
            debug!("{}: 异常模式结束", self.id);
        } else if rng.gen::<f64>() < anomaly_prob {
            self.anomaly_mode = true;
            self.anomaly_remaining = rng.gen_range(2..8);
            warn!("{}: 进入异常模式(持续{}次采样)", self.id, self.anomaly_remaining);
        }

        self.accumulated_hours += interval_hours;

        let diurnal = match self.sensor_type {
            SensorType::TEMP => {
                let phase = (self.accumulated_hours % 24.0) / 24.0 * 2.0 * std::f64::consts::PI;
                phase.sin() * 2.5
            }
            SensorType::PH => {
                let phase = (self.accumulated_hours % 24.0) / 24.0 * 2.0 * std::f64::consts::PI;
                phase.sin() * 0.15
            }
            _ => 0.0,
        };

        let drift = self.drift_rate * (self.accumulated_hours / 24.0);
        let noise: f64 = (0..5).map(|_| rng.gen::<f64>() - 0.5).sum::<f64>() / 5.0 * self.noise_std * 2.0;

        let normal_value = self.base_value + drift + diurnal + noise;

        if self.anomaly_mode {
            match self.sensor_type {
                SensorType::PH => normal_value.min(5.0) - rng.gen_range(0.0..1.5),
                SensorType::CA2 => normal_value.max(180.0) + rng.gen_range(30.0..120.0),
                SensorType::ORP => {
                    if rng.gen_bool(0.5) { normal_value - rng.gen_range(100.0..250.0) }
                    else { normal_value + rng.gen_range(100.0..250.0) }
                }
                SensorType::TEMP => normal_value + rng.gen_range(10.0..25.0),
            }
        } else {
            match self.sensor_type {
                SensorType::PH => normal_value.clamp(4.0, 9.5),
                SensorType::CA2 => normal_value.clamp(5.0, 350.0),
                SensorType::ORP => normal_value.clamp(-350.0, 600.0),
                SensorType::TEMP => normal_value.clamp(0.0, 45.0),
            }
        }
    }

    fn discharge_battery(&mut self, interval_hours: f64) {
        let daily_discharge = match self.sensor_type {
            SensorType::PH => 0.35,
            SensorType::ORP => 0.32,
            SensorType::CA2 => 0.40,
            SensorType::TEMP => 0.20,
        };
        self.battery -= daily_discharge * (interval_hours / 24.0);
        self.battery = self.battery.max(0.0);
    }

    fn rssi_value(&self) -> i32 {
        let mut rng = rand::thread_rng();
        let base = -(95 + (self.depth * 10.0) as i32);
        base + rng.gen_range(-10..5)
    }

    fn process_downlink(&mut self, frame: &lora::downlink::DownlinkFrame) -> bool {
        use lora::downlink::DownlinkCommand;
        match frame.command {
            DownlinkCommand::SET_SAMPLE_INTERVAL => {
                if let Some(ival) = frame.payload.get("interval_secs")
                    .and_then(|v| v.as_u64())
                {
                    self.sample_interval_secs = ival;
                    info!("{}: 下行配置已更新 - 采样间隔={}s", self.id, ival);
                    true
                } else { false }
            }
            DownlinkCommand::SET_TX_POWER => {
                if let Some(pw) = frame.payload.get("power_dbm")
                    .and_then(|v| v.as_i64())
                {
                    self.tx_power_dbm = pw as i32;
                    info!("{}: 下行配置已更新 - 发射功率={}dBm", self.id, pw);
                    true
                } else { false }
            }
            DownlinkCommand::SET_DATARATE => {
                if let Some(dr) = frame.payload.get("datarate")
                    .and_then(|v| v.as_str())
                {
                    self.datarate = dr.to_string();
                    info!("{}: 下行配置已更新 - 数据速率={}", self.id, dr);
                    true
                } else { false }
            }
            DownlinkCommand::QUERY_STATUS => {
                info!("{}: 状态查询 - 电量={:.1}%, 间隔={}s", self.id, self.battery, self.sample_interval_secs);
                true
            }
            DownlinkCommand::RESET_DEVICE => {
                warn!("{}: 收到复位命令，模拟复位", self.id);
                self.sample_interval_secs = 1800;
                self.tx_power_dbm = 14;
                self.datarate = "SF7BW125".to_string();
                true
            }
            _ => {
                warn!("{}: 未知下行命令: {:?}", self.id, frame.command);
                false
            }
        }
    }

    fn read(&mut self, interval_secs: u64, anomaly_prob: f64) -> SensorReading {
        let hours = interval_secs as f64 / 3600.0;
        let value = self.next_value(hours, anomaly_prob);
        self.discharge_battery(hours);

        let temp_reading = match self.sensor_type {
            SensorType::PH => {
                let mut rng = rand::thread_rng();
                Some((16.5 + rng.gen_range(-2.5..3.5) + (self.accumulated_hours % 24.0 / 24.0 * 2.0 * std::f64::consts::PI).sin() * 2.0).max(0.0))
            }
            _ => None,
        };

        SensorReading {
            sensor_id: self.id.clone(),
            sensor_type: self.sensor_type.clone(),
            value,
            relic_id: None,
            grid_x: self.grid_x,
            grid_y: self.grid_y,
            depth: self.depth,
            temperature: temp_reading,
            timestamp: Some(Utc::now()),
            battery: Some(self.battery),
            rssi: Some(self.rssi_value()),
        }
    }
}

fn build_sensors(args: &Args) -> Vec<VirtualSensor> {
    let mut sensors = Vec::new();
    let mut rng = rand::thread_rng();

    for i in 1..=args.ph_sensor_count {
        let id = format!("PHR-{:03}", i);
        let gx = rng.gen_range(0.5..49.5);
        let gy = rng.gen_range(0.5..49.5);
        let depth = rng.gen_range(0.3..2.5);
        sensors.push(VirtualSensor::new(id, SensorType::PH, gx, gy, depth));
    }
    for i in 1..=args.orp_sensor_count {
        let id = format!("ORP-{:03}", i);
        let gx = rng.gen_range(0.5..49.5);
        let gy = rng.gen_range(0.5..49.5);
        let depth = rng.gen_range(0.3..2.5);
        sensors.push(VirtualSensor::new(id, SensorType::ORP, gx, gy, depth));
    }
    for i in 1..=args.ca_sensor_count {
        let id = format!("CA-{:03}", i);
        let gx = rng.gen_range(0.5..49.5);
        let gy = rng.gen_range(0.5..49.5);
        let depth = rng.gen_range(0.3..2.5);
        sensors.push(VirtualSensor::new(id, SensorType::CA2, gx, gy, depth));
    }
    sensors
}

fn send_batch(url: &str, readings: &[SensorReading]) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(time::Duration::from_secs(30))
        .build()?;
    let endpoint = format!("{}/api/lora/batch", url);
    let resp = client.post(&endpoint).json(readings).send()?;
    if !resp.status().is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("HTTP {}: {}", resp.status(), body).into());
    }
    Ok(())
}

fn poll_downlinks(url: &str, sensor_ids: &[String]) -> Vec<(String, Vec<lora::downlink::DownlinkFrame>)> {
    let client = match reqwest::blocking::Client::builder()
        .timeout(time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut results = Vec::new();
    for id in sensor_ids.iter().take(20) {
        let endpoint = format!("{}/api/lora/downlink/for-device/{}", url, id);
        match client.get(&endpoint).send() {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(body) = resp.text() {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                        let frames: Vec<lora::downlink::DownlinkFrame> = v
                            .pointer("/data/downlinks")
                            .and_then(|d| serde_json::from_value(d.clone()).ok())
                            .unwrap_or_default();
                        if !frames.is_empty() {
                            results.push((id.clone(), frames));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    results
}

fn send_single(url: &str, reading: &SensorReading) -> Result<Vec<lora::downlink::DownlinkFrame>, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(time::Duration::from_secs(15))
        .build()?;
    let endpoint = format!("{}/api/lora/uplink", url);
    let resp = client.post(&endpoint).json(reading).send()?;
    if !resp.status().is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("HTTP {}: {}", resp.status(), body).into());
    }
    let body = resp.text().unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&body)?;
    let downlinks: Vec<lora::downlink::DownlinkFrame> = v
        .pointer("/data/downlinks")
        .and_then(|d| serde_json::from_value(d.clone()).ok())
        .unwrap_or_default();
    Ok(downlinks)
}

fn send_ack(url: &str, ack: &lora::downlink::AckFrame) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(time::Duration::from_secs(10))
        .build()?;
    let endpoint = format!("{}/api/lora/ack", url);
    let resp = client.post(&endpoint).json(ack).send()?;
    if !resp.status().is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("ACK HTTP {}: {}", resp.status(), body).into());
    }
    Ok(())
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    println!("============================================================");
    println!("  LoRa 电极数据模拟器");
    println!("============================================================");
    println!("  服务器:       {}", args.server_url);
    println!("  上报周期:     {} 秒 (×{}倍速 = {}秒)",
        args.interval_secs,
        args.speed_multiplier,
        (args.interval_secs as f64 / args.speed_multiplier) as u64);
    println!("  pH 电极:      {} 台", args.ph_sensor_count);
    println!("  ORP 电极:     {} 台", args.orp_sensor_count);
    println!("  Ca²+ 电极:    {} 台", args.ca_sensor_count);
    println!("  总电极数:     {} 台", args.ph_sensor_count + args.orp_sensor_count + args.ca_sensor_count);
    println!("  异常概率:     {:.1}%", args.anomaly_probability * 100.0);
    println!("  批量模式:     {}", args.batch_mode);
    println!("  总轮数:       {} (0=无限)", args.total_rounds);
    println!("============================================================");

    let sleep_duration = time::Duration::from_millis(
        (args.interval_secs as f64 * 1000.0 / args.speed_multiplier) as u64
    );

    let mut sensors = build_sensors(&args);
    info!("已初始化 {} 个虚拟电极", sensors.len());

    let mut round: u32 = 0;
    let mut anomaly_stats: HashMap<String, u32> = HashMap::new();

    loop {
        round += 1;
        let now = Utc::now();

        info!("======== 第 {} 轮上报  [{}] ========", round, now.format("%Y-%m-%d %H:%M:%S"));

        let mut readings = Vec::with_capacity(sensors.len());
        let mut anomalies_this_round = 0u32;

        for sensor in &mut sensors {
            let reading = sensor.read(args.interval_secs, args.anomaly_probability);
            if sensor.anomaly_mode {
                anomalies_this_round += 1;
                *anomaly_stats.entry(sensor.id.clone()).or_insert(0) += 1;
            }
            readings.push(reading);
        }

        info!("本轮采集 {} 条数据，异常 {} 条", readings.len(), anomalies_this_round);

        let send_result = if args.batch_mode {
            info!("批量发送到 {} ...", args.server_url);
            let batch_result = send_batch(&args.server_url, &readings);

            let sensor_ids: Vec<String> = readings.iter().map(|r| r.sensor_id.clone()).collect();
            let downlink_results = poll_downlinks(&args.server_url, &sensor_ids);
            let mut total_dl = 0u32;
            for (dev_eui, frames) in downlink_results {
                if let Some(sensor) = sensors.iter_mut().find(|s| s.id == dev_eui) {
                    for frame in &frames {
                        let success = sensor.process_downlink(frame);
                        if success {
                            total_dl += 1;
                            let ack = lora::downlink::AckFrame {
                                dev_eui: sensor.id.clone(),
                                fcnt: frame.fcnt,
                                ack_fcnt: frame.fcnt,
                                port: frame.port,
                                success: true,
                                result: Some("OK".to_string()),
                                rssi: Some(sensor.rssi_value()),
                                timestamp: Some(Utc::now().to_rfc3339()),
                            };
                            if let Err(e) = send_ack(&args.server_url, &ack) {
                                warn!("批量模式ACK失败 FCNT#{}: {}", frame.fcnt, e);
                            }
                        }
                    }
                }
            }
            if total_dl > 0 {
                info!("批量模式处理下行{}条", total_dl);
            }

            batch_result
        } else {
            let mut oks = 0u32;
            let mut errs = 0u32;
            let mut total_downlinks = 0u32;
            for (i, reading) in readings.iter().enumerate() {
                let delay = time::Duration::from_millis(rand::thread_rng().gen_range(0..15));
                thread::sleep(delay);
                match send_single(&args.server_url, reading) {
                    Ok(downlinks) => {
                        oks += 1;
                        if !downlinks.is_empty() {
                            if let Some(sensor) = sensors.iter_mut().find(|s| s.id == reading.sensor_id) {
                                for frame in &downlinks {
                                    let success = sensor.process_downlink(frame);
                                    if success {
                                        total_downlinks += 1;
                                        let ack = lora::downlink::AckFrame {
                                            dev_eui: sensor.id.clone(),
                                            fcnt: frame.fcnt,
                                            ack_fcnt: frame.fcnt,
                                            port: frame.port,
                                            success: true,
                                            result: Some("OK".to_string()),
                                            rssi: Some(sensor.rssi_value()),
                                            timestamp: Some(Utc::now().to_rfc3339()),
                                        };
                                        if let Err(e) = send_ack(&args.server_url, &ack) {
                                            warn!("ACK发送失败 FCNT#{}: {}", frame.fcnt, e);
                                        } else {
                                            debug!("ACK已发送 FCNT#{}", frame.fcnt);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        errs += 1;
                        if errs <= 5 {
                            error!("[{}/{}] 发送失败 {}: {}", i+1, readings.len(), reading.sensor_id, e);
                        }
                    }
                }
            }
            info!("单条发送完成: 成功{} / 失败{}，处理下行{}条", oks, errs, total_downlinks);
            if errs == 0 { Ok(()) } else { Err(format!("{}条失败", errs).into()) }
        };

        if let Err(e) = send_result {
            error!("发送过程异常: {}", e);
        }

        let interval_hours = args.interval_secs as f64 / 3600.0;
        let simulated = Duration::seconds(((interval_hours * round as f64) * 3600.0) as i64);
        info!("累计模拟时间: {} (约{:.1}天)", simulated, interval_hours * round as f64 / 24.0);

        if !anomaly_stats.is_empty() {
            let total_anom: u32 = anomaly_stats.values().sum();
            info!("历史异常总数: {} 次 (涉及{}个电极)", total_anom, anomaly_stats.len());
        }

        if args.only_once {
            info!("单次模式运行结束");
            break;
        }

        if args.total_rounds > 0 && round >= args.total_rounds {
            info!("达到总轮数限制，退出");
            break;
        }

        info!("等待 {:?} 后进行下一轮...", sleep_duration);
        thread::sleep(sleep_duration);
    }

    println!("\n==== 模拟结束 ====");
    println!("总轮数: {}", round);
    if !anomaly_stats.is_empty() {
        println!("异常统计 Top 10:");
        let mut items: Vec<_> = anomaly_stats.iter().collect();
        items.sort_by(|a, b| b.1.cmp(a.1));
        for (k, v) in items.iter().take(10) {
            println!("  {}: {} 次", k, v);
        }
    }
}
