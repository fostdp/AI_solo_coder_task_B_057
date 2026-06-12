use actix_web::{web, HttpResponse, Responder, get, post, put};
use serde::Deserialize;
use crate::models::{ApiResponse, SensorReading, SensorType, PointCloudPoint, ContourData};
use crate::database::Database;
use crate::alerts::AlertManager;
use crate::algorithms;
use crate::services::LoraIngestService;
use crate::heritage_features::{
    self,
    TemperatureHistoryPoint,
    MonteCarloParams,
};
use chrono::{Utc, Duration};
use rand::Rng;
use log::info;

#[derive(Deserialize)]
pub struct QueryParams {
    pub limit: Option<usize>,
    pub hours: Option<i64>,
    pub sensor_id: Option<String>,
    pub sensor_type: Option<String>,
}

#[derive(Deserialize)]
pub struct AnalysisRequest {
    pub relic_id: String,
    pub grid_x: f64,
    pub grid_y: f64,
    pub ph: f64,
    pub temperature: f64,
    pub ca_ppm: f64,
    pub orp: f64,
    pub elapsed_months: Option<f64>,
}

#[get("/api/health")]
pub async fn health(db: web::Data<Database>) -> impl Responder {
    let db_ok = db.ping().await;
    let data = serde_json::json!({
        "status": "ok",
        "service": "古代骨角质文物埋藏腐蚀界面监测系统",
        "version": "1.0.0",
        "database": if db_ok { "connected" } else { "disconnected" },
        "timestamp": Utc::now().to_rfc3339()
    });
    HttpResponse::Ok().json(ApiResponse::ok(data, "服务运行正常"))
}

#[post("/api/lora/uplink")]
pub async fn lora_uplink(
    db: web::Data<Database>,
    alert_mgr: web::Data<AlertManager>,
    gw: web::Data<crate::lora_gateway::LoraGateway>,
    ingest_svc: web::Data<LoraIngestService>,
    reading: web::Json<SensorReading>,
) -> impl Responder {
    let mut reading = reading.into_inner();
    if reading.timestamp.is_none() {
        reading.timestamp = Some(Utc::now());
    }

    if let Some(sensor_info) = db.get_sensor(&reading.sensor_id) {
        if reading.relic_id.is_none() {
            reading.relic_id = sensor_info.relic_id.clone();
        }
        reading.grid_x = sensor_info.grid_x;
        reading.grid_y = sensor_info.grid_y;
        reading.depth = sensor_info.depth;
        reading.sensor_type = sensor_info.sensor_type;
    }

    let alerts = alert_mgr.check_and_alert(&reading);

    match ingest_svc.ingest_reading(reading.clone()).await {
        Ok(_) => {
            info!("LoRa数据接收成功: {}={:.3} @ ({},{})",
                reading.sensor_type, reading.value, reading.grid_x, reading.grid_y);

            let downlinks = gw.get_pending_for_device(&reading.sensor_id);

            let resp_data = serde_json::json!({
                "received": true,
                "sensor_id": reading.sensor_id,
                "alerts_triggered": alerts.len(),
                "alerts": alerts,
                "downlinks": downlinks,
                "downlink_count": downlinks.len(),
                "pipeline": "lora_ingest → collagen_kinetics → ca_balance → alerter"
            });
            HttpResponse::Ok().json(ApiResponse::ok(resp_data, "LoRa数据已入库，分析管道已启动"))
        }
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(&format!("数据接入失败: {}", e))),
    }
}

#[post("/api/lora/batch")]
pub async fn lora_batch_uplink(
    db: web::Data<Database>,
    alert_mgr: web::Data<AlertManager>,
    ingest_svc: web::Data<LoraIngestService>,
    readings: web::Json<Vec<SensorReading>>,
) -> impl Responder {
    let mut total = 0;
    let mut total_alerts = 0;
    let mut readings = readings.into_inner();
    let mut processed_readings = Vec::new();

    for reading in readings.iter_mut() {
        if reading.timestamp.is_none() {
            reading.timestamp = Some(Utc::now());
        }
        if let Some(sensor_info) = db.get_sensor(&reading.sensor_id) {
            if reading.relic_id.is_none() {
                reading.relic_id = sensor_info.relic_id.clone();
            }
            reading.grid_x = sensor_info.grid_x;
            reading.grid_y = sensor_info.grid_y;
            reading.depth = sensor_info.depth;
            reading.sensor_type = sensor_info.sensor_type;
        }
        let alerts = alert_mgr.check_and_alert(reading);
        total_alerts += alerts.len();
        processed_readings.push(reading.clone());
    }

    match ingest_svc.ingest_batch(&processed_readings).await {
        Ok(count) => {
            total = count;
        }
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(&format!("批量接入失败: {}", e)));
        }
    }

    let data = serde_json::json!({
        "received": total,
        "alerts_triggered": total_alerts,
        "pipeline": "lora_ingest → collagen_kinetics → ca_balance → alerter"
    });
    HttpResponse::Ok().json(ApiResponse::ok(data, &format!("批量接收{}条数据", total)))
}

#[get("/api/relics")]
pub async fn list_relics(db: web::Data<Database>) -> impl Responder {
    let relics = db.get_relics();
    HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(relics), &format!("共{}件文物", relics.len())))
}

#[get("/api/relics/{id}")]
pub async fn get_relic(db: web::Data<Database>, path: web::Path<String>) -> impl Responder {
    match db.get_relic(&path.into_inner()) {
        Some(r) => HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(r), "查询成功")),
        None => HttpResponse::NotFound().json(ApiResponse::<()>::error("文物不存在")),
    }
}

#[get("/api/sensors")]
pub async fn list_sensors(db: web::Data<Database>) -> impl Responder {
    let sensors = db.get_sensors();
    HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(sensors), &format!("共{}个传感器", sensors.len())))
}

#[get("/api/sensors/{id}")]
pub async fn get_sensor(db: web::Data<Database>, path: web::Path<String>) -> impl Responder {
    match db.get_sensor(&path.into_inner()) {
        Some(s) => HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(s), "查询成功")),
        None => HttpResponse::NotFound().json(ApiResponse::<()>::error("传感器不存在")),
    }
}

#[get("/api/sensors/latest")]
pub async fn latest_sensor_values(
    db: web::Data<Database>,
    q: web::Query<QueryParams>,
) -> impl Responder {
    let vals = db.query_latest_sensor_values(q.sensor_type.as_deref()).await.unwrap_or_default();
    HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(vals), &format!("返回{}条最新数据", vals.len())))
}

#[get("/api/sensors/history")]
pub async fn sensor_history(
    db: web::Data<Database>,
    q: web::Query<QueryParams>,
) -> impl Responder {
    let sid = match &q.sensor_id {
        Some(s) => s.clone(),
        None => return HttpResponse::BadRequest().json(ApiResponse::<()>::error("缺少sensor_id参数")),
    };
    let hours = q.hours.unwrap_or(24);
    let end = Utc::now();
    let start = end - Duration::hours(hours);
    match db.query_sensor_history(&sid, start, end).await {
        Ok(data) => {
            let history: Vec<serde_json::Value> = data.iter()
                .map(|(t, v)| serde_json::json!({
                    "time": t.to_rfc3339(),
                    "value": *v
                }))
                .collect();
            HttpResponse::Ok().json(ApiResponse::ok(
                serde_json::json!(history),
                &format!("返回{}条历史数据", data.len())
            ))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&format!("查询失败: {:?}", e))),
    }
}

#[get("/api/grid/ph")]
pub async fn grid_ph(db: web::Data<Database>, q: web::Query<QueryParams>) -> impl Responder {
    let hours = q.hours.unwrap_or(1);
    match db.query_grid_data("pH", hours).await {
        Ok(data) => {
            let contours: Vec<ContourData> = data.iter().map(|(x,y,v)| ContourData {
                x: *x, y: *y, value: *v, label: format!("pH={:.2}", v)
            }).collect();
            HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(contours), &format!("pH网格数据{}个点", contours.len())))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&format!("查询失败: {:?}", e))),
    }
}

#[get("/api/grid/ca")]
pub async fn grid_ca(db: web::Data<Database>, q: web::Query<QueryParams>) -> impl Responder {
    let hours = q.hours.unwrap_or(1);
    match db.query_grid_data("Ca2+", hours).await {
        Ok(data) => {
            let contours: Vec<ContourData> = data.iter().map(|(x,y,v)| ContourData {
                x: *x, y: *y, value: *v, label: format!("Ca={:.1}ppm", v)
            }).collect();
            HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(contours), &format!("Ca网格数据{}个点", contours.len())))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&format!("查询失败: {:?}", e))),
    }
}

#[get("/api/grid/corrosion")]
pub async fn grid_corrosion(db: web::Data<Database>, q: web::Query<QueryParams>) -> impl Responder {
    let hours = q.hours.unwrap_or(24);
    match db.query_corrosion_grid(hours).await {
        Ok(data) => {
            let result: Vec<serde_json::Value> = data.iter().map(|(x,y,depth,coll,cap)| serde_json::json!({
                "x": x, "y": y, "corrosion_depth_um": depth,
                "collagen_deg": coll, "ca_p_ratio": cap,
                "label": format!("腐蚀={:.1}μm", depth)
            })).collect();
            HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(result), &format!("腐蚀数据{}个点", result.len())))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&format!("查询失败: {:?}", e))),
    }
}

#[get("/api/pointcloud/{relic_id}")]
pub async fn get_pointcloud(
    path: web::Path<String>,
    q: web::Query<QueryParams>,
) -> impl Responder {
    let relic_id = path.into_inner();
    let _limit = q.limit.unwrap_or(5000);
    let mut rng = rand::thread_rng();
    let num_points = 3000 + rng.gen_range(0..2000);
    let mut points = Vec::with_capacity(num_points);

    for _ in 0..num_points {
        let theta = rng.gen::<f64>() * 2.0 * std::f64::consts::PI;
        let phi = (rng.gen::<f64>() - 0.5) * std::f64::consts::PI;
        let r = 1.0 + (rng.gen::<f64>() - 0.5) * 0.3;
        let x = r * theta.cos() * phi.cos() * 5.0;
        let y = r * theta.sin() * phi.cos() * 8.0;
        let z = r * phi.sin() * 2.5;

        let dist_from_surface = (x.powi(2) + y.powi(2) + z.powi(2)).sqrt();
        let corrosion_base = (dist_from_surface - 3.0).max(0.0) * 15.0;
        let noise = (rng.gen::<f64>() - 0.3) * 20.0;
        let corrosion_depth = (corrosion_base + noise).max(0.0).min(300.0);
        let collagen = (corrosion_depth / 3.0).min(100.0);

        points.push(PointCloudPoint {
            x, y, z,
            corrosion_depth,
            collagen_deg: collagen,
        });
    }

    HttpResponse::Ok().json(ApiResponse::ok(
        serde_json::json!({
            "relic_id": relic_id,
            "num_points": points.len(),
            "points": points
        }),
        &format!("生成{}个点云数据", points.len())
    ))
}

#[post("/api/analysis/calculate")]
pub async fn calculate_analysis(
    db: web::Data<Database>,
    req: web::Json<AnalysisRequest>,
) -> impl Responder {
    let elapsed = req.elapsed_months.unwrap_or(6.0);
    let analysis = algorithms::perform_full_analysis(
        &req.relic_id, req.grid_x, req.grid_y,
        req.ph, req.temperature, req.ca_ppm, req.orp, elapsed
    );
    let _ = db.write_corrosion_analysis(&analysis).await;
    HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(analysis), "腐蚀分析计算完成"))
}

#[get("/api/alerts")]
pub async fn list_alerts(
    alert_mgr: web::Data<AlertManager>,
    q: web::Query<QueryParams>,
) -> impl Responder {
    let limit = q.limit.unwrap_or(50);
    let alerts = alert_mgr.get_alerts(limit);
    HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!({
        "total": alerts.len(),
        "alerts": alerts,
        "stats": alert_mgr.stats()
    }), &format!("返回{}条告警记录", alerts.len())))
}

#[get("/api/alerts/active")]
pub async fn active_alerts(alert_mgr: web::Data<AlertManager>) -> impl Responder {
    let alerts = alert_mgr.get_active_alerts();
    HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!({
        "count": alerts.len(),
        "alerts": alerts
    }), &format!("当前活跃告警{}条", alerts.len())))
}

#[derive(Deserialize)]
pub struct AlertActionRequest {
    pub action: String,
}

#[post("/api/alerts/{id}/action")]
pub async fn alert_action(
    alert_mgr: web::Data<AlertManager>,
    path: web::Path<String>,
    req: web::Json<AlertActionRequest>,
) -> impl Responder {
    let id = path.into_inner();
    let ok = match req.action.as_str() {
        "acknowledge" => alert_mgr.acknowledge_alert(&id),
        "resolve" => alert_mgr.resolve_alert(&id),
        _ => return HttpResponse::BadRequest().json(ApiResponse::<()>::error("无效action，可选acknowledge/resolve")),
    };
    if ok {
        HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!({"id": id, "action": req.action}), "操作成功"))
    } else {
        HttpResponse::NotFound().json(ApiResponse::<()>::error("告警不存在"))
    }
}

#[get("/api/stats/summary")]
pub async fn stats_summary(
    db: web::Data<Database>,
    alert_mgr: web::Data<AlertManager>,
) -> impl Responder {
    let relics = db.get_relics();
    let sensors = db.get_sensors();
    let latest_ph = db.query_latest_sensor_values(Some("pH")).await.unwrap_or_default();
    let latest_ca = db.query_latest_sensor_values(Some("Ca2+")).await.unwrap_or_default();
    let latest_orp = db.query_latest_sensor_values(Some("ORP")).await.unwrap_or_default();

    let avg_ph: f64 = if latest_ph.is_empty() { 0.0 } else { latest_ph.iter().map(|r| r.value).sum::<f64>() / latest_ph.len() as f64 };
    let avg_ca: f64 = if latest_ca.is_empty() { 0.0 } else { latest_ca.iter().map(|r| r.value).sum::<f64>() / latest_ca.len() as f64 };
    let avg_orp: f64 = if latest_orp.is_empty() { 0.0 } else { latest_orp.iter().map(|r| r.value).sum::<f64>() / latest_orp.len() as f64 };

    let alert_stats = alert_mgr.stats();
    let ph_low_count = latest_ph.iter().filter(|r| r.value < 5.5).count();
    let ca_high_count = latest_ca.iter().filter(|r| r.value > 200.0).count();

    let data = serde_json::json!({
        "relics": {
            "total": relics.len(),
            "at_risk": 0
        },
        "sensors": {
            "total": sensors.len(),
            "ph_sensors": sensors.iter().filter(|s| matches!(s.sensor_type, SensorType::PH)).count(),
            "orp_sensors": sensors.iter().filter(|s| matches!(s.sensor_type, SensorType::ORP)).count(),
            "ca_sensors": sensors.iter().filter(|s| matches!(s.sensor_type, SensorType::CA2)).count(),
        },
        "environment": {
            "avg_ph": avg_ph,
            "avg_ca_ppm": avg_ca,
            "avg_orp_mv": avg_orp,
            "ph_alarm_count": ph_low_count,
            "ca_alarm_count": ca_high_count,
        },
        "alerts": alert_stats,
        "last_update": Utc::now().to_rfc3339()
    });

    HttpResponse::Ok().json(ApiResponse::ok(data, "系统统计汇总"))
}

#[derive(Debug, Deserialize)]
pub struct DownlinkRequest {
    pub dev_eui: String,
    pub command: String,
    pub payload: serde_json::Value,
}

#[post("/api/lora/downlink")]
pub async fn enqueue_downlink(
    gw: web::Data<crate::lora_gateway::LoraGateway>,
    req: web::Json<DownlinkRequest>,
) -> impl Responder {
    use crate::lora_gateway::DownlinkCommand;

    let cmd = match req.command.to_uppercase().as_str() {
        "SET_SAMPLE_INTERVAL" => DownlinkCommand::SET_SAMPLE_INTERVAL,
        "SET_THRESHOLDS" => DownlinkCommand::SET_THRESHOLDS,
        "SET_TX_POWER" => DownlinkCommand::SET_TX_POWER,
        "SET_DATARATE" => DownlinkCommand::SET_DATARATE,
        "RESET_DEVICE" => DownlinkCommand::RESET_DEVICE,
        "CALIBRATE" => DownlinkCommand::CALIBRATE,
        "FIRMWARE_UPDATE" => DownlinkCommand::FIRMWARE_UPDATE,
        "CONFIG_ACK" => DownlinkCommand::CONFIG_ACK,
        "QUERY_STATUS" => DownlinkCommand::QUERY_STATUS,
        _ => return HttpResponse::BadRequest().json(ApiResponse::<()>::error("未知命令类型")),
    };

    let fcnt = gw.enqueue(&req.dev_eui, cmd, req.payload.clone());
    HttpResponse::Ok().json(ApiResponse::ok(
        serde_json::json!({ "dev_eui": req.dev_eui, "fcnt": fcnt, "pending": gw.pending_count() }),
        &format!("下行命令已入队，帧序号={}", fcnt)
    ))
}

#[post("/api/lora/ack")]
pub async fn receive_ack(
    gw: web::Data<crate::lora_gateway::LoraGateway>,
    ack: web::Json<crate::lora_gateway::AckFrame>,
) -> impl Responder {
    let ok = gw.process_ack(&ack);
    if ok {
        HttpResponse::Ok().json(ApiResponse::ok(
            serde_json::json!({ "processed": true, "remaining_pending": gw.pending_count() }),
            "ACK处理成功"
        ))
    } else {
        HttpResponse::NotFound().json(ApiResponse::<()>::error("未找到对应下行帧"))
    }
}

#[get("/api/lora/downlink/pending")]
pub async fn list_pending_downlinks(
    gw: web::Data<crate::lora_gateway::LoraGateway>,
    q: web::Query<QueryParams>,
) -> impl Responder {
    let limit = q.limit.unwrap_or(50);
    let list = gw.list_pending(limit);
    let stats = gw.get_stats();
    HttpResponse::Ok().json(ApiResponse::ok(
        serde_json::json!({
            "total_pending": gw.pending_count(),
            "items": list,
            "stats": stats,
        }),
        &format!("待确认下行帧共{}条", gw.pending_count())
    ))
}

#[get("/api/lora/downlink/stats")]
pub async fn downlink_stats(
    gw: web::Data<crate::lora_gateway::LoraGateway>,
) -> impl Responder {
    let stats = gw.get_stats();
    HttpResponse::Ok().json(ApiResponse::ok(stats, "下行链路统计"))
}

#[get("/api/lora/downlink/for-device/{dev_eui}")]
pub async fn get_downlink_for_device(
    gw: web::Data<crate::lora_gateway::LoraGateway>,
    path: web::Path<String>,
) -> impl Responder {
    let dev_eui = path.into_inner();
    let frames = gw.get_pending_for_device(&dev_eui);
    HttpResponse::Ok().json(ApiResponse::ok(
        serde_json::json!({ "dev_eui": dev_eui, "downlinks": frames, "count": frames.len() }),
        &format!("返回{}条待发下行帧", frames.len())
    ))
}

#[derive(Debug, Deserialize)]
pub struct BatchDownlinkRequest {
    pub devices: Vec<String>,
    pub command: String,
    pub payload: serde_json::Value,
}

#[post("/api/lora/downlink/batch")]
pub async fn batch_enqueue_downlink(
    gw: web::Data<crate::lora_gateway::LoraGateway>,
    req: web::Json<BatchDownlinkRequest>,
) -> impl Responder {
    use crate::lora_gateway::DownlinkCommand;

    let cmd = match req.command.to_uppercase().as_str() {
        "SET_SAMPLE_INTERVAL" => DownlinkCommand::SET_SAMPLE_INTERVAL,
        "SET_THRESHOLDS" => DownlinkCommand::SET_THRESHOLDS,
        "SET_TX_POWER" => DownlinkCommand::SET_TX_POWER,
        "SET_DATARATE" => DownlinkCommand::SET_DATARATE,
        "RESET_DEVICE" => DownlinkCommand::RESET_DEVICE,
        "CALIBRATE" => DownlinkCommand::CALIBRATE,
        _ => return HttpResponse::BadRequest().json(ApiResponse::<()>::error("未知命令")),
    };

    let mut results = Vec::new();
    for dev in &req.devices {
        let fcnt = gw.enqueue(dev, cmd, req.payload.clone());
        results.push(serde_json::json!({ "dev_eui": dev, "fcnt": fcnt }));
    }
    HttpResponse::Ok().json(ApiResponse::ok(
        serde_json::json!({ "count": results.len(), "results": results }),
        &format!("批量下发{}条下行命令", results.len())
    ))
}

#[derive(Debug, Deserialize)]
pub struct EhPhDiagramRequest {
    pub ph: f64,
    pub eh_mv: f64,
    pub ph_min: Option<f64>,
    pub ph_max: Option<f64>,
    pub eh_min: Option<f64>,
    pub eh_max: Option<f64>,
    pub grid_x: Option<usize>,
    pub grid_y: Option<usize>,
}

#[post("/api/heritage/eh-ph-diagram")]
pub async fn calculate_eh_ph_diagram(req: web::Json<EhPhDiagramRequest>) -> impl Responder {
    let ph_range = (req.ph_min.unwrap_or(2.0), req.ph_max.unwrap_or(12.0));
    let eh_range = (req.eh_min.unwrap_or(-500.0), req.eh_max.unwrap_or(800.0));
    let grid_res = (req.grid_x.unwrap_or(20), req.grid_y.unwrap_or(20));

    let result = heritage_features::generate_eh_ph_diagram(
        req.ph, req.eh_mv, ph_range, eh_range, grid_res
    );
    HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(result),
        &format!("Eh-pH相图生成成功，识别分带: {}", result.dominant_zone_name)))
}

#[derive(Debug, Deserialize)]
pub struct CpiRequest {
    pub activation_energy: Option<f64>,
    pub burial_years: f64,
    pub current_temp_c: f64,
    pub temperature_history: Option<Vec<TemperatureHistoryPoint>>,
    pub initial_collagen_fraction: Option<f64>,
}

#[post("/api/heritage/collagen-preservation-index")]
pub async fn calculate_collagen_preservation_index(req: web::Json<CpiRequest>) -> impl Responder {
    let ea = req.activation_energy.unwrap_or(85_000.0);
    let init_frac = req.initial_collagen_fraction.unwrap_or(1.0);

    let result = heritage_features::calculate_cpi(
        ea,
        req.burial_years,
        req.current_temp_c,
        req.temperature_history.clone(),
        init_frac,
    );
    HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(result),
        &format!("骨胶原保存潜力指数计算完成: CPI={:.1}, {}", result.cpi_score, result.cpi_grade)))
}

#[derive(Debug, Deserialize)]
pub struct ExcavationOptimizationRequest {
    #[serde(flatten)]
    pub params: Option<MonteCarloParams>,
    pub num_simulations: Option<usize>,
    pub current_ph: Option<f64>,
    pub current_temp_c: Option<f64>,
    pub current_ca_ppm: Option<f64>,
    pub current_orp_mv: Option<f64>,
    pub forecast_years: Option<f64>,
    pub target_corrosion_threshold_um: Option<f64>,
    pub current_collagen_remaining_pct: Option<f64>,
}

#[post("/api/heritage/excavation-optimization")]
pub async fn run_excavation_optimization(req: web::Json<ExcavationOptimizationRequest>) -> impl Responder {
    let mut params = req.params.clone().unwrap_or_default();
    if let Some(n) = req.num_simulations { params.num_simulations = n; }
    if let Some(v) = req.current_ph { params.current_ph = v; }
    if let Some(v) = req.current_temp_c { params.current_temp_c = v; }
    if let Some(v) = req.current_ca_ppm { params.current_ca_ppm = v; }
    if let Some(v) = req.current_orp_mv { params.current_orp_mv = v; }
    if let Some(v) = req.forecast_years { params.forecast_years = v; }
    if let Some(v) = req.target_corrosion_threshold_um { params.target_corrosion_threshold_um = v; }
    if let Some(v) = req.current_collagen_remaining_pct { params.current_collagen_remaining_pct = v; }

    let result = heritage_features::run_monte_carlo_excavation(params);
    HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(result),
        &format!("蒙特卡洛发掘优化完成 ({}次模拟), 置信度={:.0}%",
            result.simulations_completed, result.confidence_level * 100.0)))
}

#[derive(Debug, Deserialize)]
pub struct ProtectionRecommendationRequest {
    pub ph: f64,
    pub ca_ppm: f64,
    pub orp_mv: f64,
    pub ambient_temp_c: Option<f64>,
    pub ambient_rh_pct: Option<f64>,
    pub burial_depth_m: Option<f64>,
    pub relic_category: Option<String>,
}

#[post("/api/heritage/temporary-protection")]
pub async fn recommend_temporary_protection_scheme(req: web::Json<ProtectionRecommendationRequest>) -> impl Responder {
    let ambient_temp = req.ambient_temp_c.unwrap_or(22.0);
    let ambient_rh = req.ambient_rh_pct.unwrap_or(55.0);
    let burial_depth = req.burial_depth_m.unwrap_or(1.5);
    let relic_category = req.relic_category.clone().unwrap_or_else(|| "人骨".to_string());

    let result = heritage_features::recommend_temporary_protection(
        req.ph, req.ca_ppm, req.orp_mv,
        ambient_temp, ambient_rh, burial_depth, &relic_category
    );
    HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!(result),
        &format!("临时保护方案推荐完成: {} (有效度={:.0}%)",
            result.primary_moisturizer_zh, result.expected_effectiveness_score)))
}

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg
        .service(health)
        .service(lora_uplink)
        .service(lora_batch_uplink)
        .service(enqueue_downlink)
        .service(receive_ack)
        .service(list_pending_downlinks)
        .service(downlink_stats)
        .service(get_downlink_for_device)
        .service(batch_enqueue_downlink)
        .service(list_relics)
        .service(get_relic)
        .service(list_sensors)
        .service(get_sensor)
        .service(latest_sensor_values)
        .service(sensor_history)
        .service(grid_ph)
        .service(grid_ca)
        .service(grid_corrosion)
        .service(get_pointcloud)
        .service(calculate_analysis)
        .service(list_alerts)
        .service(active_alerts)
        .service(alert_action)
        .service(stats_summary)
        .service(calculate_eh_ph_diagram)
        .service(calculate_collagen_preservation_index)
        .service(run_excavation_optimization)
        .service(recommend_temporary_protection_scheme);
}
