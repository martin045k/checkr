use std::{
    marker::PhantomData,
    path::Path,
    sync::{Arc, Mutex},
};

use ce_shell::Input;
use driver::JobKind;
use rusqlite::{types::FromSql, OptionalExtension, ToSql};

use crate::endpoints::InspectifyJobMeta;

#[derive(Clone)]
pub struct CheckoDb {
    conn: Arc<Mutex<rusqlite::Connection>>,
}
pub struct Compressed<T> {
    data: Vec<u8>,
    _ph: PhantomData<T>,
}

impl FromSql for Compressed<JobData> {
    fn column_result(value: rusqlite::types::ValueRef) -> rusqlite::types::FromSqlResult<Self> {
        Ok(Self {
            data: FromSql::column_result(value)?,
            _ph: PhantomData,
        })
    }
}

impl ToSql for Compressed<JobData> {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput> {
        self.data.to_sql()
    }
}

impl<T: serde::Serialize + for<'a> serde::Deserialize<'a>> Compressed<T> {
    pub fn compress(data: &T) -> Self {
        let data = serde_json::to_vec(data).unwrap();
        let data = lz4_flex::compress_prepend_size(&data);
        Self {
            data,
            _ph: PhantomData,
        }
    }
    #[tracing::instrument(skip_all)]
    pub fn decompress(&self) -> T {
        let data = lz4_flex::decompress_size_prepended(&self.data).unwrap();
        serde_json::from_slice(&data).unwrap()
    }
}

pub struct Id<T> {
    pub id: usize,
    _ph: PhantomData<T>,
}

impl<T> std::fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Id")
            .field(&std::any::type_name::<T>())
            .field(&self.id)
            .finish()
    }
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> FromSql for Id<T> {
    fn column_result(value: rusqlite::types::ValueRef) -> rusqlite::types::FromSqlResult<Self> {
        Ok(Self {
            id: FromSql::column_result(value)?,
            _ph: PhantomData,
        })
    }
}

impl<T> ToSql for Id<T> {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput> {
        self.id.to_sql()
    }
}

pub struct WithId<T> {
    pub id: Id<T>,
    data: T,
}

impl<T> std::ops::Deref for WithId<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

pub struct Run<T = JobData> {
    pub group_name: String,
    input_md5: [u8; 16],
    pub data: T,
    queued: chrono::DateTime<chrono::Utc>,
    started: Option<chrono::DateTime<chrono::Utc>>,
    finished: Option<chrono::DateTime<chrono::Utc>>,
}

pub type JobData = driver::JobData<InspectifyJobMeta>;

pub type CompressedRun = Run<Compressed<JobData>>;

impl From<Run> for CompressedRun {
    fn from(run: Run) -> Self {
        let data = Compressed::compress(&run.data);
        Self {
            group_name: run.group_name,
            input_md5: run.input_md5,
            data,
            queued: run.queued,
            started: run.started,
            finished: run.finished,
        }
    }
}

impl From<CompressedRun> for Run {
    fn from(run: CompressedRun) -> Self {
        let data = run.data.decompress();
        Self {
            group_name: run.group_name,
            input_md5: run.input_md5,
            data,
            queued: run.queued,
            started: run.started,
            finished: run.finished,
        }
    }
}

impl Run {
    pub fn new(group_name: String, input: Input) -> color_eyre::Result<Self> {
        let input_md5 = input.hash();
        Ok(Self {
            group_name: group_name.clone(),
            input_md5,
            data: JobData::new(
                JobKind::Analysis(input),
                InspectifyJobMeta {
                    group_name: Some(group_name),
                },
            ),
            queued: chrono::Utc::now(),
            started: None,
            finished: None,
        })
    }
}

impl CompressedRun {
    pub fn input(&self) -> Option<Input> {
        match self.data.decompress().kind {
            JobKind::Analysis(input) => Some(input),
            _ => None,
        }
    }
}

impl CheckoDb {
    pub fn open(path: &Path) -> color_eyre::Result<Self> {
        tracing::debug!(?path, "opening db");

        let conn = rusqlite::Connection::open(path)?;

        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS runs (
                id INTEGER PRIMARY KEY,
                group_name TEXT NOT NULL,
                input_md5 BLOB NOT NULL,
                data BLOB NOT NULL,
                queued TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                started TIMESTAMP,
                finished TIMESTAMP
            );
            "#,
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn conn(&self) -> std::sync::MutexGuard<rusqlite::Connection> {
        self.conn.lock().unwrap()
    }

    pub fn create_run(&self, run: Run) -> color_eyre::Result<()> {
        let run: CompressedRun = run.into();
        self.conn().execute(
            "INSERT INTO runs (group_name, input_md5, data, started, finished) VALUES (?1, ?2, ?3, ?4, ?5)",
            (&run.group_name, &run.input_md5, &run.data, &run.started, &run.finished),
        )?;
        Ok(())
    }

    pub fn start_run(&self, id: Id<CompressedRun>) -> color_eyre::Result<()> {
        self.conn().execute(
            "UPDATE runs SET started = CURRENT_TIMESTAMP WHERE id = ?1",
            [id.id],
        )?;
        Ok(())
    }

    pub fn finish_run(&self, id: Id<CompressedRun>, data: &JobData) -> color_eyre::Result<()> {
        let data = Compressed::compress(data);
        self.conn().execute(
            "UPDATE runs SET finished = CURRENT_TIMESTAMP, data = ?2 WHERE id = ?1",
            (id.id, data),
        )?;
        Ok(())
    }

    pub fn unfinished_runs(&self, count: usize) -> color_eyre::Result<Vec<WithId<CompressedRun>>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            // "SELECT id, group_name, input_md5, data, queued, started, finished FROM runs WHERE finished IS NULL ORDER BY queued LIMIT ?1",
            "SELECT id, group_name, input_md5, data, queued, started, finished FROM runs WHERE finished IS NULL ORDER BY input_md5 LIMIT ?1",
        )?;
        let runs = stmt
            .query_map([count], |row| {
                let id = row.get(0)?;
                let data = Run {
                    group_name: row.get(1)?,
                    input_md5: row.get(2)?,
                    data: row.get(3)?,
                    queued: row.get(4)?,
                    started: row.get(5)?,
                    finished: row.get(6)?,
                };
                Ok(WithId { id, data })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(runs)
    }

    pub fn run_by_group_and_input(
        &self,
        group_name: &str,
        input: &Input,
    ) -> color_eyre::Result<Option<Id<CompressedRun>>> {
        let input_md5 = input.hash();
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT id FROM runs WHERE group_name = ?1 AND input_md5 = ?2")?;
        let id = stmt
            .query_row((group_name, input_md5), |row| row.get(0))
            .optional()?;
        Ok(id)
    }

    pub fn runs_by_group(
        &self,
        group_name: &str,
    ) -> color_eyre::Result<Vec<WithId<CompressedRun>>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, group_name, input_md5, data, queued, started, finished FROM runs WHERE group_name = ?1",
        )?;
        let runs = stmt
            .query_map([group_name], |row| {
                let id = row.get(0)?;
                let data = Run {
                    group_name: row.get(1)?,
                    input_md5: row.get(2)?,
                    data: row.get(3)?,
                    queued: row.get(4)?,
                    started: row.get(5)?,
                    finished: row.get(6)?,
                };
                Ok(WithId { id, data })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(runs)
    }

    pub fn all_runs(&self) -> color_eyre::Result<Vec<WithId<CompressedRun>>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, group_name, input_md5, data, queued, started, finished FROM runs",
        )?;
        let runs = stmt
            .query_map([], |row| {
                let id = row.get(0)?;
                let data = Run {
                    group_name: row.get(1)?,
                    input_md5: row.get(2)?,
                    data: row.get(3)?,
                    queued: row.get(4)?,
                    started: row.get(5)?,
                    finished: row.get(6)?,
                };
                Ok(WithId { id, data })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(runs)
    }
}
