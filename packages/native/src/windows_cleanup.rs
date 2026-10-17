use super::windows_bridge::Owner;
use super::windows_lifecycle::{Registration, Registry};
use super::UiCommand;
use futures::channel::mpsc;
use napi::{Env, Result};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

pub(super) struct RendererShutdown {
    pub owner: Arc<Owner>,
    pub sender: mpsc::UnboundedSender<UiCommand>,
}

pub(super) type RendererRegistration = Registration<RendererShutdown>;
type CleanupRegistry = Registry<RendererShutdown>;

thread_local! {
    static OWNERS: RefCell<BTreeMap<usize, Arc<CleanupRegistry>>> = const { RefCell::new(BTreeMap::new()) };
}

pub(super) fn for_env(env: Env) -> Result<Arc<CleanupRegistry>> {
    let key = env.raw() as usize;
    OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        if let Some(owner) = owners.get(&key) {
            return Ok(Arc::clone(owner));
        }
        let owner = Arc::new(Registry::new(|resource: RendererShutdown| {
            resource.owner.shutdown();
            resource.sender.unbounded_send(UiCommand::WindowsShutdownBridge).ok();
        }));
        env.add_env_cleanup_hook((key, Arc::clone(&owner)), |(key, owner)| {
            OWNERS.with(|owners| owners.borrow_mut().remove(&key));
            owner.shutdown();
        })?;
        owners.insert(key, Arc::clone(&owner));
        Ok(owner)
    })
}
