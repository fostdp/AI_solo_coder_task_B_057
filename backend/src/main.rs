mod models;
mod database;
mod algorithms;
mod kinetics;
mod ode;
mod alerts;
mod lora;
mod lora_gateway;
mod handlers;
mod config;
mod services;
mod heritage_features;

use actix_web::{App, HttpServer, middleware, web};
use actix_cors::Cors;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use actix_files as fs;
use env_logger::Env;
use log::info;
use tokio::sync::mpsc;
use crate::config::AppConfig;
use crate::services::{LoraIngestService, CollagenKineticsService, CaBalanceService, AlerterService};
use crate::services::KineticsPartial;
use crate::services::ServiceMessage;
use crate::models::CorrosionAnalysis;

#[actix_web::main]
async fn main() -> io::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info,relic_monitor_backend=debug")).init();

    let app_config = AppConfig::load();

    let db = database::Database::new(
        &app_config.influxdb.url,
        &app_config.influxdb.database,
        &app_config.influxdb.username,
        &app_config.influxdb.password,
    );

    if let Err(e) = db.verify_connection().await {
        eprintln!("⚠️  InfluxDB连接验证失败: {}", e);
        eprintln!("    系统将继续启动，但数据写入功能将不可用");
    }

    if let Err(e) = db.init_default_data().await {
        eprintln!("初始化默认数据警告: {:?}", e);
    }

    let lora_gw = lora_gateway::LoraGateway::new();

    let (tx_ingest, rx_kinetics) = mpsc::channel::<ServiceMessage>(1000);
    let (tx_kinetics, rx_ca) = mpsc::channel::<KineticsPartial>(500);
    let (tx_ca, rx_alert) = mpsc::channel::<CorrosionAnalysis>(200);

    let ingest_svc = LoraIngestService::new(
        app_config.clone(),
        db.clone(),
        lora_gw.clone(),
        tx_ingest,
    );

    let kinetics_svc = CollagenKineticsService::new(
        app_config.clone(),
        rx_kinetics,
        tx_kinetics,
    );

    let ca_svc = CaBalanceService::new(
        app_config.clone(),
        db.clone(),
        rx_ca,
        tx_ca,
    );

    let alert_svc = AlerterService::new(
        app_config.clone(),
        rx_alert,
    );

    let alert_manager = alerts::AlertManager::default();

    let ingest_data = web::Data::new(ingest_svc.clone());
    let alert_data = web::Data::new(alert_manager.clone());
    let gw_data = web::Data::new(lora_gw.clone());
    let db_data = web::Data::new(db.clone());
    let config_data = web::Data::new(app_config.clone());
    let alerter_svc_data = web::Data::new(Arc::new(alert_svc.clone()));

    let frontend_dir = PathBuf::from(app_config.server.frontend_dir.clone());
    let bind_addr = format!("{}:{}", app_config.server.host, app_config.server.port);

    println!("============================================================");
    println!("  古代骨角质文物埋藏腐蚀界面监测系统 - 后端服务");
    println!("  Rust Actix-Web v4  |  InfluxDB 1.8  |  微服务架构");
    println!("============================================================");
    println!("  服务地址:   http://{}", bind_addr);
    println!("  InfluxDB:   {} ({})", app_config.influxdb.url, app_config.influxdb.database);
    println!("  服务管道:   lora_ingest → collagen_kinetics → ca_balance → alerter");
    println!("  传感器:     50 pH + 50 ORP + 30 Ca²+ = 130台");
    println!("  文物数量:   500件 (旧石器时代骨角质)");
    println!("  采集周期:   30分钟/次 (LoRa)");
    println!("  告警阈值:   pH<{:.1} | Ca²+>{:.0}ppm",
        app_config.alerts.ph_low_threshold,
        app_config.alerts.ca_high_threshold);
    println!("  ODE求解:    BDF 隐式刚性求解器 (1-5阶自适应)");
    println!("============================================================");

    let kinetics_handle = tokio::spawn(async move {
        let svc = kinetics_svc;
        svc.run().await;
    });

    let ca_handle = tokio::spawn(async move {
        let svc = ca_svc;
        svc.run().await;
    });

    let alert_handle = tokio::spawn(async move {
        let svc = alert_svc;
        svc.run().await;
    });

    let _ingest_handle = tokio::spawn(async move {
        ingest_svc.run().await;
    });

    info!("全部4个微服务启动完成");

    let server = HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .wrap(middleware::Logger::default())
            .wrap(middleware::Compress::default())
            .wrap(cors)
            .app_data(db_data.clone())
            .app_data(alert_data.clone())
            .app_data(gw_data.clone())
            .app_data(config_data.clone())
            .app_data(ingest_data.clone())
            .app_data(alerter_svc_data.clone())
            .configure(handlers::configure_routes)
            .service(
                fs::Files::new("/", frontend_dir.clone())
                    .index_file("index.html")
                    .use_last_modified(true)
            )
    })
    .bind(&bind_addr)?
    .workers(num_cpus::get())
    .run();

    let server_result = server.await;

    let _ = kinetics_handle.abort();
    let _ = ca_handle.abort();
    let _ = alert_handle.abort();

    server_result
}
