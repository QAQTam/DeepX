//! DeepX HTTP API server.
//!
//! Architecture:
//!   HTTP request → mpsc channel → agent loop → SSE
//!
//! The agent loop starts immediately (even without API key).
//! Config is checked per-request via Arc<RwLock<Config>>, so the
//! setup screen can save a key and chat works without restart.

use std::io::BufReader;
use std::sync::{mpsc, Arc, RwLock};
use std::thread;

use axum::{routing::{get, post}, Router};
use tokio::sync::broadcast;
use tower_http::{cors::CorsLayer, services::ServeDir};

use dsx_agent::agent::AgentState;
use dsx_agent::config::Config;
use dsx_agent::runner::turn;
use dsx_proto::{Agent2Ui, Ui2Agent};

mod routes;

// ── Types ──

pub type AgentRequest = (Ui2Agent, broadcast::Sender<Agent2Ui>);

pub struct AppState {
    pub input_tx: mpsc::Sender<AgentRequest>,
    pub config: Arc<RwLock<Config>>,
}

// ── Agent loop (always running, checks config per-turn) ──

fn spawn_agent_loop(
    config: Arc<RwLock<Config>>,
    input_rx: mpsc::Receiver<AgentRequest>,
) {
    thread::spawn(move || {
        for (frame, reply_tx) in input_rx {
            match frame {
                Ui2Agent::UserInput { text } => {
                    let (agent_tx, agent_rx) = mpsc::channel::<Agent2Ui>();
                    let bridge_reply = reply_tx.clone();
                    thread::spawn(move || {
                        while let Ok(event) = agent_rx.recv() {
                            let is_done = matches!(event, Agent2Ui::Done);
                            let _ = bridge_reply.send(event);
                            if is_done {
                                break;
                            }
                        }
                    });

                    // Load fresh config + agent state on every turn
                    let cfg = config.read().unwrap().clone();

                    if !cfg.is_ready() {
                        let _ = reply_tx.send(Agent2Ui::Error {
                            message: "No API key configured. Set one in the setup screen.".into(),
                        });
                        let _ = reply_tx.send(Agent2Ui::Done);
                        drop(agent_tx);
                        continue;
                    }

                    // Try HP connection each turn (may have been restarted)
                    let hp_conn = dsx_agent::hp::try_reconnect().map(BufReader::new);

                    if let Some(mut hp) = hp_conn {
                        let mut agent = AgentState::new(cfg);

                        let result = std::panic::catch_unwind(
                            std::panic::AssertUnwindSafe(|| {
                                turn::handle_user_input(&mut agent, &text, &mut hp, &agent_tx);
                            }),
                        );

                        if result.is_err() {
                            let _ = reply_tx.send(Agent2Ui::Error {
                                message: "Agent panicked processing input".into(),
                            });
                            let _ = reply_tx.send(Agent2Ui::Done);
                        }

                        agent.maybe_save_session();
                    } else {
                        let _ = reply_tx.send(Agent2Ui::Error {
                            message: "HP daemon not connected. Check that 'dsx hp' is running.".into(),
                        });
                        let _ = reply_tx.send(Agent2Ui::Done);
                    }

                    drop(agent_tx);
                }

                Ui2Agent::Cancel => {
                    dsx_agent::tools::CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
                    dsx_agent::tools::cancel_current_tool();
                }

                _ => {}
            }
        }

        dsx_agent::tools::shutdown_tools();
        dsx_agent::hp::kill_hp_daemon();
        tracing::info!("dsx-axum: agent loop shut down");
    });
}

// ── Main ──

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dsx=info,dsx_axum=info".into()),
        )
        .init();

    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|a| a.strip_prefix("--port=").map(|p| p.to_string()))
        .or_else(|| std::env::args().nth(2))
        .and_then(|p| p.parse().ok())
        .unwrap_or(9527);

    let config = Arc::new(RwLock::new(Config::load()?));

    let (input_tx, input_rx) = mpsc::channel::<AgentRequest>();
    spawn_agent_loop(config.clone(), input_rx);

    let state = Arc::new(AppState { input_tx, config });

    let app = Router::new()
        .route("/api/chat", post(routes::chat::chat_handler))
        .route("/api/config", get(routes::config::get_config).put(routes::config::put_config))
        .layer(CorsLayer::permissive())
        .with_state(state)
        .fallback_service(
            ServeDir::new("frontend/dist").append_index_html_on_directories(true),
        );

    let addr = format!("127.0.0.1:{port}");
    tracing::info!("DeepX server → http://{addr}");
    println!("DeepX server → http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
