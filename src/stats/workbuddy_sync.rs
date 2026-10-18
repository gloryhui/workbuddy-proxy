//! Internal synchronization journal controls. No public management command may
//! submit these intents; production file mutation holds the Bridge mutation lock.
use super::*;
use workbuddy_import::{classify::ConfigObservation, observation::ManagedModel};

/// Cancellation releases the in-memory guard. The durable prepared rows remain
/// and are recovered from the next real profile snapshot, including lost ACKs.
pub(crate) struct WorkbuddySync {
    token: Option<String>,
    lease: Arc<AtomicBool>,
    shared: Arc<workbuddy_import::runtime::Shared>,
}

impl Drop for WorkbuddySync {
    fn drop(&mut self) {
        self.lease.store(false, Ordering::Release);
        self.shared.request_observation();
    }
}

impl StatsHandle {
    pub(crate) async fn prepare_workbuddy_sync(
        &self,
        models: Vec<ManagedModel>,
    ) -> Result<WorkbuddySync, String> {
        let mut session = WorkbuddySync {
            token: None,
            lease: Arc::new(AtomicBool::new(true)),
            shared: self.inner.import_runtime.clone(),
        };
        let response = match self
            .control(ControlKind::WorkbuddySyncPrepare {
                models,
                lease: session.lease.clone(),
            })
            .await
        {
            Ok(response) => response,
            Err(_) if self.inner.import_runtime.legacy_sync_allowed() => return Ok(session),
            Err(error) => return Err(error),
        };
        session.token = serde_json::from_str(&response)
            .map_err(|_| error(ERR_UNAVAILABLE, "invalid synchronization journal reply"))?;
        Ok(session)
    }

    pub(crate) async fn finish_workbuddy_sync(
        &self,
        session: &WorkbuddySync,
        observations: Vec<ConfigObservation>,
    ) -> Result<bool, String> {
        let Some(token) = session.token.as_ref() else {
            return Ok(true);
        };
        let response = self
            .control(ControlKind::WorkbuddySyncFinish {
                token: token.clone(),
                observations,
            })
            .await?;
        serde_json::from_str(&response)
            .map_err(|_| error(ERR_UNAVAILABLE, "invalid synchronization journal reply"))
    }
}

pub(super) fn prepare(
    inner: &Arc<Inner>,
    writer: &mut store::Writer,
    models: Vec<ManagedModel>,
    lease: Arc<AtomicBool>,
) -> Result<String, String> {
    let tx = writer
        .conn
        .transaction()
        .map_err(|_| error(ERR_UNAVAILABLE, "cannot prepare synchronization journal"))?;
    let token = workbuddy_import::observation::prepare_sync(&tx, &models, now_ms())
        .map_err(|_| error(ERR_UNAVAILABLE, "cannot prepare synchronization journal"))?;
    tx.commit()
        .map_err(|_| error(ERR_UNAVAILABLE, "cannot commit synchronization journal"))?;
    if token.is_some() {
        inner.import_runtime.register_sync(lease);
    }
    serde_json::to_string(&token)
        .map_err(|_| error(ERR_UNAVAILABLE, "cannot serialize synchronization journal"))
}

pub(super) fn finish(
    writer: &mut store::Writer,
    token: &str,
    observations: Vec<ConfigObservation>,
) -> Result<String, String> {
    let tx = writer
        .conn
        .transaction()
        .map_err(|_| error(ERR_UNAVAILABLE, "cannot finalize synchronization journal"))?;
    let confirmed = workbuddy_import::observation::finish_sync(&tx, token, &observations, now_ms())
        .map_err(|_| error(ERR_UNAVAILABLE, "cannot finalize synchronization journal"))?;
    tx.commit()
        .map_err(|_| error(ERR_UNAVAILABLE, "cannot commit synchronization journal"))?;
    Ok(confirmed.to_string())
}
