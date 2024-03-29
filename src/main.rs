#[macro_use]
extern crate tracing;

mod request;

use std::sync::Arc;

use clap::Parser;
use color_eyre::{eyre::eyre, Result};
use common_x::{configure, log};
use indicatif::ProgressBar;
use parking_lot::RwLock;
use request::Request;
use reqwest::Client;
use serde_json::Value;

use crate::request::Job;

fn clap_about() -> String {
    let name = env!("CARGO_PKG_NAME").to_string();
    let version = env!("CARGO_PKG_VERSION");
    let authors = env!("CARGO_PKG_AUTHORS");
    name + " " + version + "\n" + authors
}

#[derive(Parser)]
#[clap(version, about = clap_about())]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
enum SubCommand {
    #[clap(name = "run")]
    Run(RunOpts),
}

/// A subcommand for run
#[derive(Parser, Debug)]
struct RunOpts {
    #[clap(short = 'w', default_value = "0")]
    worker: usize,
    #[clap(short = 'c', default_value = "1")]
    concurrency: u64,
    #[clap(short = 'n', default_value = "0")]
    total_num: u64,
    #[clap(short = 't', default_value = "10000")]
    timeout: u64,
    #[clap(short = 'g', default_value = "50")]
    granularity: u64,
    #[clap(short = 'i', default_value = "0")]
    init_seq_num: u64,
    #[clap(short = 'j', default_value = "job.toml")]
    job: String,
    #[clap(short = 'd', default_value = "false")]
    debug: bool,
}

fn main() {
    ::std::env::set_var("RUST_BACKTRACE", "full");

    let opts: Opts = Opts::parse();

    match opts.subcmd {
        SubCommand::Run(opts) => {
            if opts.debug {
                log::init_log_filter("debug,hyper=info");
            } else {
                log::init_log_filter("info");
            }
            let runtime = if opts.worker > 0 {
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(opts.worker)
                    .enable_all()
                    .build()
                    .unwrap()
            } else {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
            };
            if let Err(e) = runtime.block_on(run(opts)) {
                error!("err: {:?}", e);
            }
        }
    }
}

#[inline]
fn categorize(elapsed_vec: &mut Vec<usize>, granularity: u64, elapsed: f64) {
    let i = (elapsed as u64 / granularity) as usize;
    if i >= elapsed_vec.len() {
        for _ in 0..=(i - elapsed_vec.len()) {
            elapsed_vec.push(0);
        }
    }
    elapsed_vec[i] += 1;
}

async fn run(opts: RunOpts) -> Result<()> {
    let total = opts.total_num;
    let pb = ProgressBar::new(total);
    let record = Arc::new(RwLock::new(Record {
        success: 0,
        elapsed_sum: 0.0,
        elapsed_vec: vec![],
    }));

    let (tx, rv) = flume::bounded(total as usize);

    let mut job: Job = configure::file_config(&opts.job).unwrap();
    job.init_seq_num = opts.init_seq_num;
    for i in 0..opts.total_num {
        let request = job.get(i);
        tx.send(request).ok();
    }

    let mut workers = vec![];

    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(opts.concurrency as usize)
        .connect_timeout(std::time::Duration::from_millis(opts.timeout))
        .timeout(std::time::Duration::from_millis(opts.timeout))
        .build()?;

    let begin_time = std::time::SystemTime::now();

    for _ in 0..opts.concurrency {
        let record = record.clone();
        let rv = rv.clone();
        let pb = pb.clone();
        let http_client = http_client.clone();
        let granularity = opts.granularity;
        let worker = tokio::spawn(async move {
            while let Ok(key) = rv.try_recv() {
                if let Err(e) = subtask(key, record.clone(), http_client.clone(), granularity).await
                {
                    warn!("task err: {:?}", e);
                }
                pb.inc(1);
            }
        });
        workers.push(worker);
    }

    for worker in workers {
        worker.await.ok();
    }
    pb.finish_with_message("done");

    let elapsed_total = begin_time.elapsed().unwrap().as_secs_f64();
    let record = record.read();
    let elapsed_avg = (record.elapsed_sum / record.elapsed_num() as f64) * 1000.;
    let speed = total as f64 / elapsed_total;
    for (i, num) in record.elapsed_vec.iter().enumerate() {
        if *num > 0 {
            info!(
                "elapsed [{}, {}) ms: {num}",
                opts.granularity * i as u64,
                opts.granularity * (i + 1) as u64,
            );
        }
    }
    info!("elapsed_avg: {elapsed_avg} ms");
    info!("elapsed_total: {elapsed_total} s");
    info!("speed: {speed} /s");
    info!(
        "total: {total}, success: {}, failed: {}",
        record.success,
        total - record.success
    );

    Ok(())
}

struct Record {
    success: u64,
    elapsed_sum: f64,
    elapsed_vec: Vec<usize>,
}

impl Record {
    #[inline]
    pub fn elapsed_num(&self) -> usize {
        self.elapsed_vec.iter().sum()
    }
}

#[inline]
fn recording(tr: TaskResult, record: Arc<RwLock<Record>>, granularity: u64) {
    let mut record = record.write();
    if tr.status_code == 200 {
        debug!("completed: {tr:?}");
        record.success += 1;
    } else {
        warn!("task err: {tr:?}");
    }
    record.elapsed_sum += tr.elapsed;
    categorize(&mut record.elapsed_vec, granularity, tr.elapsed * 1000.0);
}

#[derive(Debug)]
pub struct TaskResult {
    pub req: Request,
    pub status_code: u16,
    pub elapsed: f64,
    pub rsp: String,
}

#[inline]
async fn subtask(
    request: Request,
    record: Arc<RwLock<Record>>,
    http_client: Client,
    granularity: u64,
) -> Result<()> {
    let begin_time = std::time::SystemTime::now();
    let mut req_builder = http_client.post(&request.url).json(&request.body);

    if let Some(headers) = request.headers.as_object().cloned() {
        for header in headers {
            req_builder = req_builder.header(header.0, header.1.as_str().unwrap());
        }
    }

    let rsp = req_builder
        .send()
        .await
        .map_err(|e| eyre!("send {request:?} err: {e}"))?;

    let elapsed = begin_time.elapsed().unwrap().as_secs_f64();
    let status_code = rsp.status().as_u16();
    let json = rsp
        .json::<Value>()
        .await
        .map_err(|e| eyre!("decode json err {request:?}: {e}"))?;
    recording(
        TaskResult {
            req: request,
            status_code,
            elapsed,
            rsp: json.to_string(),
        },
        record,
        granularity,
    );
    Ok(())
}
