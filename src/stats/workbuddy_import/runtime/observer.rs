//! Bounded current-profile observation on the dedicated source thread.
//! Only sanitized observations survive a read; credentials never enter Work.
use super::*;
use std::{fs, io::Read, path::PathBuf, time::SystemTime};

#[derive(Clone, PartialEq, Eq)]
struct Stamp(Option<(u64, Option<SystemTime>)>);

fn stamp(path: &Path) -> Result<Stamp, &'static str> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            Ok(Stamp(Some((meta.len(), meta.modified().ok()))))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Stamp(None)),
        _ => Err("stats_workbuddy_source_read_failed"),
    }
}

/// Typed projection prevents unknown provider/key fields from leaving this read.
fn local_key(path: PathBuf) -> Result<String, &'static str> {
    #[derive(serde::Deserialize)]
    struct Projection {
        #[serde(default)]
        local_key: String,
    }
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        _ => return Err("stats_workbuddy_source_read_failed"),
    };
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "stats_workbuddy_source_read_failed")?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("stats_workbuddy_source_read_failed");
    }
    serde_json::from_slice::<Projection>(&bytes)
        .map(|config| config.local_key)
        .map_err(|_| "stats_workbuddy_unsupported_format")
}

#[derive(Default)]
pub(super) struct Observer {
    due: Option<Instant>,
    stamps: Option<(Stamp, Stamp)>,
    observations: Vec<super::super::classify::ConfigObservation>,
    epoch: u64,
    last_error: Option<&'static str>,
}

impl Observer {
    pub(super) fn next_due(&self) -> Option<Instant> {
        self.due
    }

    /// Invoked before JSONL work. Refreshes evidence each fixed five seconds,
    /// reusing only sanitized observations when both file metadata are stable.
    pub(super) fn poll(
        &mut self,
        inner: &Inner,
        root: &reader::ReaderRoot,
        sender: &SyncSender<Envelope>,
        shared: &Shared,
    ) -> Result<(), &'static str> {
        if shared.sync_active() {
            return Ok(());
        }
        let force = shared.observation_requested.swap(false, Ordering::AcqRel);
        if !force && self.due.is_some_and(|due| Instant::now() < due) {
            return self.last_error.map_or(Ok(()), Err);
        }
        self.due = Some(Instant::now() + reader::SYNC_INTERVAL);
        telemetry::source_io(shared);
        let result = self.observe(inner, root, sender, shared, force);
        self.last_error = result.err();
        result
    }

    fn observe(
        &mut self,
        inner: &Inner,
        root: &reader::ReaderRoot,
        sender: &SyncSender<Envelope>,
        shared: &Shared,
        force: bool,
    ) -> Result<(), &'static str> {
        let epoch = shared.observation_epoch.load(Ordering::Acquire);
        let path = root.root().join("models.json");
        let before = (stamp(&path)?, stamp(&inner.dir.join("config.json"))?);
        let at = now_ms();
        let observations = if force || self.epoch != epoch || self.stamps.as_ref() != Some(&before)
        {
            let key = local_key(inner.dir.join("config.json"))?;
            crate::workbuddy::import_observations(&path, &key, at)
                .map_err(|_| "stats_workbuddy_unsupported_format")?
        } else {
            self.observations.clone()
        };
        if before != (stamp(&path)?, stamp(&inner.dir.join("config.json"))?) {
            return Err("stats_workbuddy_stale_batch");
        }
        // The final marker belongs to the same snapshot timestamp. Interrupted
        // batches are retried in full, so absent entries are never closed early.
        for chunk in observations.chunks(types::MAX_BATCH_ITEMS) {
            submit(
                inner,
                sender,
                shared,
                Work::Observe {
                    observations: chunk.to_vec(),
                    at_ms: at,
                    final_batch: false,
                    epoch,
                },
            )?;
        }
        submit(
            inner,
            sender,
            shared,
            Work::Observe {
                observations: Vec::new(),
                at_ms: at,
                final_batch: true,
                epoch,
            },
        )?;
        self.stamps = Some(before);
        self.observations = observations;
        self.epoch = epoch;
        Ok(())
    }
}

use super::worker::submit;
