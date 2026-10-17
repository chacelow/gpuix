use std::collections::BTreeMap;
use parking_lot::Mutex;
use std::sync::Arc;

struct State<T> {
    next_id: u64,
    active: BTreeMap<u64, T>,
}

pub(super) struct Registry<T> {
    state: Mutex<State<T>>,
    shutdown: fn(T),
}

pub(super) struct Registration<T> {
    registry: Arc<Registry<T>>,
    id: u64,
}

impl<T> Registry<T> {
    pub fn new(shutdown: fn(T)) -> Self {
        Self {
            state: Mutex::new(State {
                next_id: 0,
                active: BTreeMap::new(),
            }),
            shutdown,
        }
    }

    pub fn register(self: &Arc<Self>, resource: T) -> Registration<T> {
        let mut state = self.state.lock();
        let id = state.next_id;
        state.next_id += 1;
        state.active.insert(id, resource);
        Registration {
            registry: Arc::clone(self),
            id,
        }
    }

    pub fn shutdown(&self) {
        let active = std::mem::take(&mut self.state.lock().active);
        for resource in active.into_values() {
            (self.shutdown)(resource);
        }
    }

    fn close(&self, id: u64) {
        let resource = self.state.lock().active.remove(&id);
        if let Some(resource) = resource {
            (self.shutdown)(resource);
        }
    }

    #[cfg(test)]
    fn active_count(&self) -> usize {
        self.state.lock().active.len()
    }
}

impl<T> Registration<T> {
    pub fn close(&self) {
        self.registry.close(self.id);
    }
}

impl<T> Drop for Registration<T> {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    fn count_shutdown(count: Arc<AtomicUsize>) {
        count.fetch_add(1, Ordering::Relaxed);
    }

    #[test]
    fn repeated_instances_unregister_without_retaining_resources() {
        let registry = Arc::new(Registry::new(count_shutdown));
        let count = Arc::new(AtomicUsize::new(0));
        for expected in 1..=256 {
            let registration = registry.register(Arc::clone(&count));
            assert_eq!(registry.active_count(), 1);
            assert_eq!(Arc::strong_count(&count), 2);
            drop(registration);
            assert_eq!(registry.active_count(), 0);
            assert_eq!(Arc::strong_count(&count), 1);
            assert_eq!(count.load(Ordering::Relaxed), expected);
        }
        registry.shutdown();
        assert_eq!(count.load(Ordering::Relaxed), 256);
    }

    #[test]
    fn close_then_drop_does_not_repeat_shutdown() {
        let registry = Arc::new(Registry::new(count_shutdown));
        let count = Arc::new(AtomicUsize::new(0));
        let registration = registry.register(Arc::clone(&count));
        registration.close();
        registration.close();
        assert_eq!(registry.active_count(), 0);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        drop(registration);
        registry.shutdown();
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(Arc::strong_count(&count), 1);
    }

    #[test]
    fn env_teardown_shuts_down_only_remaining_instances() {
        let registry = Arc::new(Registry::new(count_shutdown));
        let count = Arc::new(AtomicUsize::new(0));
        let first = registry.register(Arc::clone(&count));
        let second = registry.register(Arc::clone(&count));
        let third = registry.register(Arc::clone(&count));
        first.close();
        assert_eq!(registry.active_count(), 2);
        registry.shutdown();
        registry.shutdown();
        assert_eq!(registry.active_count(), 0);
        assert_eq!(count.load(Ordering::Relaxed), 3);
        drop((first, second, third));
        assert_eq!(count.load(Ordering::Relaxed), 3);
        assert_eq!(Arc::strong_count(&count), 1);
    }

    #[test]
    fn retained_completion_sender_does_not_delay_shutdown_request() {
        let registry = Arc::new(Registry::new(|sender: mpsc::Sender<()>| {
            sender.send(()).unwrap();
        }));
        let (sender, receiver) = mpsc::channel();
        let completion_sender = sender.clone();
        let registration = registry.register(sender);
        drop(registration);
        assert_eq!(registry.active_count(), 0);
        receiver.try_recv().unwrap();
        assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty));
        drop(receiver);
        assert!(completion_sender.send(()).is_err());
    }

    #[test]
    fn env_teardown_racing_drop_shuts_down_once() {
        let registry = Arc::new(Registry::new(count_shutdown));
        let count = Arc::new(AtomicUsize::new(0));
        let registration = registry.register(Arc::clone(&count));
        std::thread::scope(|scope| {
            scope.spawn(|| registry.shutdown());
            scope.spawn(move || drop(registration));
        });
        assert_eq!(registry.active_count(), 0);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(Arc::strong_count(&count), 1);
    }
}
