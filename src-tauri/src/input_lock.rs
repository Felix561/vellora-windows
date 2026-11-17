//! One process-wide lock held during any clipboard + `SendInput` sequence, shared by dictation
//! delivery and Text Assist, so the two never interleave clipboard writes and injected keys.

use std::sync::OnceLock;

use tokio::sync::{Mutex, MutexGuard};

static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn lock_ref() -> &'static Mutex<()> {
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Async acquisition for code running on the async runtime.
pub async fn lock() -> MutexGuard<'static, ()> {
    lock_ref().lock().await
}

/// Blocking acquisition for worker threads. Must not be called from an async context.
#[allow(dead_code)] // Used by Text Assist capture/replace (Task F).
pub fn blocking_lock() -> MutexGuard<'static, ()> {
    lock_ref().blocking_lock()
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::Duration,
    };

    use super::*;

    #[test]
    fn blocking_holders_are_mutually_exclusive() {
        let inside = Arc::new(AtomicBool::new(false));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let inside = inside.clone();
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        let _guard = blocking_lock();
                        assert!(!inside.swap(true, Ordering::SeqCst));
                        std::thread::sleep(Duration::from_micros(200));
                        inside.store(false, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
    }

    #[tokio::test]
    async fn async_holder_blocks_a_worker_thread() {
        let guard = lock().await;
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _guard = blocking_lock();
            sender.send(()).unwrap();
        });
        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
        drop(guard);
        receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
    }
}
