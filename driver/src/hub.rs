use std::{
    ffi::OsStr,
    fmt::Debug,
    path::Path,
    process::Stdio,
    sync::{atomic::AtomicUsize, Arc, RwLock},
    time::Duration,
};

use color_eyre::eyre::Context;
use tokio::{io::AsyncReadExt, sync::Mutex, task::JoinSet};
use tracing::Instrument;

use crate::{
    job::{Job, JobData, JobEvent, JobEventSource, JobInner, JobKind},
    JobId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HubEvent {
    JobAdded(JobId),
}

#[derive(Debug, Clone)]
pub struct Hub<M> {
    next_job_id: Arc<AtomicUsize>,
    jobs: Arc<RwLock<Vec<Job<M>>>>,
    events_tx: Arc<tokio::sync::broadcast::Sender<HubEvent>>,
    events_rx: Arc<tokio::sync::broadcast::Receiver<HubEvent>>,
}

impl<M> PartialEq for Hub<M> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.jobs, &other.jobs)
    }
}

impl<M: Send + Sync + 'static> Hub<M> {
    pub fn new() -> color_eyre::Result<Self> {
        let next_job_id = Arc::new(AtomicUsize::new(0));
        let jobs = Arc::new(RwLock::new(Vec::new()));

        let (events_tx, events_rx) = tokio::sync::broadcast::channel(128);

        Ok(Self {
            next_job_id,
            jobs,
            events_tx: Arc::new(events_tx),
            events_rx: Arc::new(events_rx),
        })
    }

    pub fn events(&self) -> tokio::sync::broadcast::Receiver<HubEvent> {
        self.events_rx.resubscribe()
    }

    fn next_job_id(&self) -> JobId {
        JobId {
            value: self
                .next_job_id
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        }
    }

    #[tracing::instrument(skip_all, fields(?kind))]
    pub fn exec_command(
        &self,
        kind: JobKind,
        cwd: impl AsRef<Path> + Debug,
        meta: M,
        program: impl AsRef<OsStr> + Debug,
        args: impl IntoIterator<Item = impl AsRef<OsStr>> + Debug,
    ) -> color_eyre::Result<Job<M>>
    where
        M: Debug,
    {
        let id = self.next_job_id();

        let mut cmd = tokio::process::Command::new(program);

        cmd.current_dir(cwd);

        cmd.args(args)
            .stderr(Stdio::piped())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());

        cmd.kill_on_drop(true);

        cmd.env("CARGO_TERM_COLOR", "always");

        tracing::debug!(?cmd, "spawning");

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn {:?}", cmd))?;

        let stdin = child.stdin.take().expect("we piped stdin");
        let stderr = child.stderr.take().expect("we piped stderr");
        let stdout = child.stdout.take().expect("we piped stdout");

        let (events_tx, events_rx) = tokio::sync::broadcast::channel(128);

        // Terminate the job if it has been running for longer than the timeout.
        // We give a generous timeout for compilation jobs, and a more strict one for analysis jobs.
        let timeout = match &kind {
            JobKind::Analysis(_) => Duration::from_secs(10),
            JobKind::Compilation => Duration::from_secs(60),
        };
        let data = Arc::new(RwLock::new(JobData::new(kind, meta)));

        let mut join_set = tokio::task::JoinSet::new();
        spawn_reader(
            JobEventSource::Stderr,
            &mut join_set,
            stderr,
            events_tx.clone(),
            {
                let data = data.clone();
                move |bytes| {
                    let mut data = data.write().unwrap();
                    let from = data.stderr.len();
                    data.stderr.extend_from_slice(bytes);
                    let to = data.stderr.len();
                    (from, to)
                }
            },
        );
        spawn_reader(
            JobEventSource::Stdout,
            &mut join_set,
            stdout,
            events_tx.clone(),
            {
                let data = data.clone();
                move |bytes| {
                    let mut data = data.write().unwrap();
                    let from = data.stdout.len();
                    data.stdout.extend_from_slice(bytes);
                    let to = data.stdout.len();
                    (from, to)
                }
            },
        );

        let job = Job::new(
            id,
            JobInner {
                id,
                child: tokio::sync::RwLock::new(Some(child)),
                stdin: Some(stdin),
                events_tx: Arc::new(events_tx),
                events_rx: Arc::new(events_rx),
                join_set: Mutex::new(join_set),
                data,
                wait_lock: Default::default(),
            },
        );

        self.jobs.write().unwrap().push(job.clone());
        self.events_tx.send(HubEvent::JobAdded(id)).unwrap();

        tokio::spawn({
            let job = job.clone();
            async move {
                tokio::time::sleep(timeout).await;
                job.kill();
                // TODO: indicate that it timed out
            }
        });

        Ok(job)
    }
    pub fn jobs(&self, count: Option<usize>) -> Vec<Job<M>> {
        if let Some(count) = count {
            self.jobs.read().unwrap()[self.jobs.read().unwrap().len().saturating_sub(count)..]
                .to_vec()
        } else {
            self.jobs.read().unwrap().clone()
        }
    }

    pub fn get_job(&self, id: JobId) -> Option<Job<M>> {
        self.jobs(None).iter().find(|j| j.id() == id).cloned()
    }

    pub fn add_finished_job(&self, j: JobData<M>) -> Job<M> {
        let id = self.next_job_id();

        let (events_tx, events_rx) = tokio::sync::broadcast::channel(128);
        let inner = JobInner {
            id,
            child: Default::default(),
            stdin: Default::default(),
            events_tx: Arc::new(events_tx),
            events_rx: Arc::new(events_rx),
            join_set: Default::default(),
            data: Arc::new(RwLock::new(j)),
            wait_lock: Default::default(),
        };
        let job = Job::new(id, inner);
        self.jobs.write().unwrap().push(job.clone());
        self.events_tx.send(HubEvent::JobAdded(id)).unwrap();

        job
    }
}

#[tracing::instrument(skip_all, fields(spawn_reader=%src))]
fn spawn_reader(
    src: JobEventSource,
    join_set: &mut JoinSet<()>,
    mut reader: impl AsyncReadExt + Sized + Unpin + Send + 'static,
    event_tx: tokio::sync::broadcast::Sender<JobEvent>,
    mut write: impl FnMut(&[u8]) -> (usize, usize) + 'static + Send + Sync,
) {
    join_set.spawn({
        async move {
            let mut buf = Vec::with_capacity(1024);
            loop {
                buf.clear();
                let read_n = reader.read_buf(&mut buf).await.expect("read failed");
                if read_n == 0 {
                    tracing::debug!("closed");
                    event_tx.send(JobEvent::Closed { src }).unwrap();
                    break;
                }
                let (from, to) = write(&buf);
                event_tx.send(JobEvent::Wrote { src, from, to }).unwrap();
            }
        }
        .in_current_span()
    });
}
