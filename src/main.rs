use std::{
    collections::HashSet,
    convert::Infallible,
    mem,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use anyhow::Error;
use binance_async::{websocket::usdm::WebsocketMessage, BinanceWebsocket};
use clap::Parser;
use futures::StreamExt;
use hyper::{
    server::Server,
    service::{make_service_fn, service_fn},
    Body, Response,
};
use lazy_static::lazy_static;
use log::{error, info};
use prometheus::{gather, register_gauge_vec, Encoder, GaugeVec, TextEncoder};
use rust_decimal::prelude::*;
use tokio::{
    spawn,
    time::{sleep, timeout},
};

lazy_static! {
    pub static ref PRICE: GaugeVec = register_gauge_vec!(
        "price",
        "The price for a given symbol",
        &["exchange", "symbol"]
    )
    .unwrap();
}

#[derive(Parser, Clone, Debug)]
struct Cli {
    // #[arg(long, env)]
    // pub api_key: String,
    // #[arg(long, env)]
    // pub secret_key: String,
    #[arg(long, env)]
    pub symbol: Vec<String>,

    #[arg(long, env, default_value = "9090")]
    pub port: u16,

    #[arg(long, env, default_value_t = 60)]
    pub timeout: u64,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::init();
    let mut cli = Cli::parse();
    if cli.symbol.is_empty() {
        error!("symbol cannot be empty");
        return Ok(());
    }

    let metrics_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), cli.port);
    spawn(start_metrics_server(metrics_addr));

    let symbols: HashSet<String> = HashSet::from_iter(mem::take(&mut cli.symbol));

    loop {
        let mut ws: BinanceWebsocket<WebsocketMessage> =
            BinanceWebsocket::new(&["!bookTicker"]).await?;

        loop {
            let msg = match timeout(Duration::from_secs(cli.timeout), ws.next()).await {
                Ok(Some(Ok(m))) => m,
                Ok(None) => {
                    error!("Websocket exited");
                    sleep(Duration::from_secs(1)).await;
                    break;
                }
                Ok(Some(Err(e))) => {
                    error!("Websocket exited: {e:?}");
                    sleep(Duration::from_secs(1)).await;
                    break;
                }
                Err(_) => {
                    error!("Timeout");
                    sleep(Duration::from_secs(1)).await;
                    break;
                }
            };

            match msg {
                WebsocketMessage::BookTicker(msg) => {
                    if !symbols.contains(&msg.symbol) {
                        continue;
                    }

                    PRICE.with_label_values(&["Binance", &msg.symbol]).set(
                        ((msg.best_bid + msg.best_ask) / Decimal::TWO)
                            .to_f64()
                            .unwrap_or(0.),
                    )
                }
                WebsocketMessage::Ping => ws.pong().await?,
                m => error!("Unknown message: {m:?}"),
            }
        }
    }
}

pub async fn start_metrics_server(addr: SocketAddr) {
    info!("Prometheus exporter running on {addr:?}");

    let make_service = make_service_fn(|_| async {
        Ok::<_, Infallible>(service_fn(|_req| async {
            let mut buffer = vec![];
            let encoder = TextEncoder::new();
            let metric_families = gather();
            encoder.encode(&metric_families, &mut buffer).unwrap();

            Ok::<_, Infallible>(Response::new(Body::from(buffer)))
        }))
    });

    let server = Server::bind(&addr).serve(make_service);

    if let Err(e) = server.await {
        error!("[Server] Exit with error: {}", e);
    }
}
