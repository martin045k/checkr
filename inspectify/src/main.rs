use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    extract::State,
    http::{header::CONTENT_TYPE, HeaderValue, Method, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use checkr::{
    driver::{Driver, DriverError},
    env::{
        graph::{GraphEnv, GraphEnvInput},
        pv::ProgramVerificationEnv,
        Analysis, Environment, InterpreterEnv, SecurityEnv, SignEnv, ToMarkdown,
    },
    pg::Determinism,
};
use clap::Parser;
use color_eyre::eyre::Context;
use notify_debouncer_mini::DebounceEventResult;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

#[typeshare::typeshare]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisRequest {
    pub analysis: Analysis,
    pub src: String,
    pub input: String,
}

#[typeshare::typeshare]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResponse {
    pub stdout: String,
    pub stderr: String,
    pub parsed_markdown: Option<String>,
    pub took: Duration,
    pub validation_result: Option<ValidationResult>,
}

#[typeshare::typeshare]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "content")]
pub enum ValidationResult {
    CorrectTerminated,
    CorrectNonTerminated { iterations: u32 },
    Mismatch { reason: String },
    TimeOut,
}

impl From<checkr::env::ValidationResult> for ValidationResult {
    fn from(r: checkr::env::ValidationResult) -> Self {
        use checkr::env::ValidationResult as VR;

        match r {
            VR::CorrectTerminated => ValidationResult::CorrectTerminated,
            VR::CorrectNonTerminated { iterations } => ValidationResult::CorrectNonTerminated {
                iterations: iterations as _,
            },
            VR::Mismatch { reason } => ValidationResult::Mismatch { reason },
            VR::TimeOut => ValidationResult::TimeOut,
        }
    }
}

async fn analyze(
    State(state): State<ApplicationState>,
    Json(body): Json<AnalysisRequest>,
) -> Json<AnalysisResponse> {
    let cmds = body.src;
    let output = match body.analysis {
        Analysis::Graph => run(state.driver, GraphEnv, cmds, body.input).await,
        Analysis::Sign => run(state.driver, SignEnv, cmds, body.input).await,
        Analysis::Interpreter => run(state.driver, InterpreterEnv, cmds, body.input).await,
        Analysis::Security => run(state.driver, SecurityEnv, cmds, body.input).await,
        Analysis::ProgramVerification => {
            run(state.driver, ProgramVerificationEnv, cmds, body.input).await
        }
    };
    return Json(output);

    async fn run<E: Environment>(
        driver: Arc<Mutex<Driver>>,
        env: E,
        cmds: String,
        input: String,
    ) -> AnalysisResponse {
        let input: E::Input = serde_json::from_str(&input).expect("failed to parse input");
        let driver = driver.lock().await;
        match driver.exec_raw_cmds::<E>(&cmds, &input).await {
            Ok(exec_output) => {
                let cmds = checkr::parse::parse_commands(&cmds).unwrap();
                let validation_res = env.validate(&cmds, &input, &exec_output.parsed);
                AnalysisResponse {
                    stdout: String::from_utf8(exec_output.output.stdout).unwrap(),
                    stderr: String::from_utf8(exec_output.output.stderr).unwrap(),
                    parsed_markdown: Some(exec_output.parsed.to_markdown()),
                    took: exec_output.took,
                    validation_result: Some(validation_res.into()),
                }
            }
            Err(e) => match e {
                checkr::driver::ExecError::Serialize(_) => todo!(),
                checkr::driver::ExecError::RunExec(_) => todo!(),
                checkr::driver::ExecError::CommandFailed(output, took) => AnalysisResponse {
                    stdout: String::from_utf8(output.stdout).unwrap(),
                    stderr: String::from_utf8(output.stderr).unwrap(),
                    parsed_markdown: None,
                    took,
                    validation_result: None,
                },
                checkr::driver::ExecError::Parse {
                    inner: _,
                    run_output,
                    time,
                } => AnalysisResponse {
                    stdout: String::from_utf8(run_output.stdout).unwrap(),
                    stderr: String::from_utf8(run_output.stderr).unwrap(),
                    parsed_markdown: None,
                    took: time,
                    validation_result: None,
                },
            },
        }
    }
}

#[typeshare::typeshare]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRequest {
    src: String,
    deterministic: bool,
}

#[typeshare::typeshare]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphResponse {
    dot: Option<String>,
}

async fn graph(
    State(state): State<ApplicationState>,
    Json(body): Json<GraphRequest>,
) -> Json<GraphResponse> {
    match state
        .driver
        .lock()
        .await
        .exec_raw_cmds::<GraphEnv>(
            &body.src,
            &GraphEnvInput {
                determinism: match body.deterministic {
                    true => Determinism::Deterministic,
                    false => Determinism::NonDeterministic,
                },
            },
        )
        .await
    {
        Ok(output) => Json(GraphResponse {
            dot: Some(output.parsed.dot),
        }),
        Err(_) => Json(GraphResponse { dot: None }),
    }
}

#[typeshare::typeshare]
#[derive(Debug, Clone, Serialize, Deserialize)]
enum CompilerState {
    Compiling,
    Compiled,
    CompileError,
}

#[typeshare::typeshare]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompilationStatus {
    compiled_at: u32,
    state: CompilerState,
}

impl CompilationStatus {
    fn new(state: CompilerState) -> Self {
        Self {
            compiled_at: std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as _,
            state,
        }
    }
}

async fn get_compilation_status(State(state): State<ApplicationState>) -> Json<CompilationStatus> {
    Json(state.compilation_status.lock().await.clone())
}

#[axum::debug_handler]
async fn static_dir(uri: axum::http::Uri) -> impl axum::response::IntoResponse {
    static UI_DIR: include_dir::Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/ui/dist/");

    if uri.path() == "/" {
        return Html(
            UI_DIR
                .get_file("index.html")
                .unwrap()
                .contents_utf8()
                .unwrap(),
        )
        .into_response();
    }

    match UI_DIR.get_file(&uri.path()[1..]) {
        Some(file) => {
            let mime_type = mime_guess::from_path(uri.path())
                .first_raw()
                .map(HeaderValue::from_static)
                .unwrap_or_else(|| {
                    HeaderValue::from_str(mime::APPLICATION_OCTET_STREAM.as_ref()).unwrap()
                });
            (
                [(axum::http::header::CONTENT_TYPE, mime_type)],
                file.contents(),
            )
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Debug, Parser)]
struct Cli {
    #[clap(short, long, default_value_t = false)]
    open: bool,
    #[clap(default_value = ".")]
    dir: PathBuf,
}

#[derive(Clone)]
struct ApplicationState {
    driver: Arc<Mutex<Driver>>,
    compilation_status: Arc<Mutex<CompilationStatus>>,
}

fn spawn_watcher(
    shared_driver: &Arc<Mutex<Driver>>,
    shared_compilation_status: &Arc<Mutex<CompilationStatus>>,
    dir: PathBuf,
    run: checko::RunOption,
) -> Result<(), color_eyre::Report> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let driver = Arc::clone(shared_driver);
    let compilation_status = Arc::clone(shared_compilation_status);

    let matches = run
        .watch
        .iter()
        .map(|p| glob::Pattern::new(p).wrap_err_with(|| format!("{p:?} was not a valid glob")))
        .collect::<Result<Vec<glob::Pattern>, color_eyre::Report>>()?;
    notify_debouncer_mini::new_debouncer(
        Duration::from_millis(200),
        None,
        move |res: DebounceEventResult| match res {
            Ok(events) => {
                if !events
                    .iter()
                    .any(|e| matches.iter().any(|p| p.matches_path(&e.path)))
                {
                    return;
                }

                tx.send(()).expect("sending to file watcher failed");
            }
            Err(errors) => errors.iter().for_each(|e| eprintln!("Error {e:?}")),
        },
    )?
    .watcher()
    .watch(&dir, notify::RecursiveMode::Recursive)?;

    tokio::spawn(async move {
        while let Some(()) = rx.recv().await {
            info!("recompiling due to changes!");
            let compile_start = std::time::Instant::now();
            *compilation_status.lock().await = CompilationStatus::new(CompilerState::Compiling);
            let new_driver = if let Some(compile) = &run.compile {
                match Driver::compile(&dir, compile, &run.run) {
                    Ok(driver) => driver,
                    Err(DriverError::CompileFailure(output)) => {
                        error!("failed to compile:");
                        eprintln!("{}", std::str::from_utf8(&output.stderr).unwrap());
                        eprintln!("{}", std::str::from_utf8(&output.stdout).unwrap());
                        *compilation_status.lock().await =
                            CompilationStatus::new(CompilerState::CompileError);
                        return;
                    }
                    Err(DriverError::RunCompile(err)) => {
                        error!("run compile failed:");
                        eprintln!("{err}");
                        *compilation_status.lock().await =
                            CompilationStatus::new(CompilerState::CompileError);
                        return;
                    }
                }
            } else {
                Driver::new(&dir, &run.run)
            };
            info!("compiled in {:?}", compile_start.elapsed());
            *compilation_status.lock().await = CompilationStatus::new(CompilerState::Compiled);
            *driver.lock().await = new_driver;
        }
    });
    Ok(())
}

async fn run() -> color_eyre::Result<()> {
    let cli = Cli::parse();
    let run = checko::RunOption::from_file(cli.dir.join("run.toml"))
        .wrap_err_with(|| format!("could not read {:?}", cli.dir.join("run.toml")))?;

    let driver = if let Some(compile) = &run.compile {
        Driver::compile(&cli.dir, compile, &run.run)
            .wrap_err_with(|| format!("compiling using config: {run:?}"))?
    } else {
        Driver::new(&cli.dir, &run.run)
    };
    let driver = Arc::new(Mutex::new(driver));
    let compilation_status = Arc::new(Mutex::new(CompilationStatus::new(CompilerState::Compiled)));

    spawn_watcher(&driver, &compilation_status, cli.dir, run)?;

    let app = Router::new()
        .route("/analyze", post(analyze))
        .route("/graph", post(graph))
        .route("/compilation-status", get(get_compilation_status))
        .with_state(ApplicationState {
            driver,
            compilation_status,
        })
        .fallback(static_dir)
        .layer(
            CorsLayer::new()
                .allow_origin("*".parse::<HeaderValue>().unwrap())
                .allow_headers([CONTENT_TYPE])
                .allow_methods([Method::GET, Method::POST]),
        );
    // NOTE: Enable for HTTP logging
    // .layer(TraceLayer::new_for_http());

    if cli.open {
        tokio::task::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            open::that("http://localhost:3000").unwrap();
        });
    }

    {
        use crossterm::{
            cursor,
            style::{self, Stylize},
            terminal, ExecutableCommand,
        };
        use std::io::stdout;

        stdout()
            .execute(terminal::Clear(terminal::ClearType::All))?
            .execute(cursor::MoveTo(3, 2))?
            .execute(style::PrintStyledContent("Inspectify".bold().green()))?
            .execute(style::PrintStyledContent(" is running".green()))?
            .execute(cursor::MoveTo(3, 4))?
            .execute(style::Print("  ➜  "))?
            .execute(style::PrintStyledContent("Local:".bold()))?
            .execute(style::PrintStyledContent("   http://localhost:".cyan()))?
            .execute(style::PrintStyledContent("3000".cyan().bold()))?
            .execute(style::PrintStyledContent("/".cyan()))?
            .execute(cursor::MoveTo(0, 7))?;
    }

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    tracing_subscriber::fmt::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .without_time()
        .init();

    run().await
}
