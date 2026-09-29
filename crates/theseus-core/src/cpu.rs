//! The daemon's CPU pool (theseus-a60): one semaphore with a permit per core.
//!
//! Waiting is async and costs no thread; computing takes a core. Every
//! in-process toollet takes a permit before it runs on tokio's blocking pool
//! and holds it until it returns. So tool calls, across every session, never
//! run more CPU work at once than there are cores, and never a thread per
//! call. A call that waits for a permit is waiting, not failing.

use std::sync::Arc;

use tokio::sync::Semaphore;

pub struct CpuPool {
    permits: Arc<Semaphore>,
    size: usize,
}

impl CpuPool {
    /// A pool of `size` permits, at least one.
    pub fn new(size: usize) -> Arc<Self> {
        let size = size.max(1);
        Arc::new(Self {
            permits: Arc::new(Semaphore::new(size)),
            size,
        })
    }

    /// A permit per core this process may use.
    pub fn for_host() -> Arc<Self> {
        Self::new(std::thread::available_parallelism().map_or(1, |n| n.get()))
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// Permits held now.
    pub fn busy(&self) -> usize {
        self.size - self.permits.available_permits()
    }

    /// Wait for a free core, then run `f` on the blocking pool. `f` holds the
    /// core until it returns, even when its caller stopped waiting for it.
    pub async fn spawn<T, F>(&self, f: F) -> tokio::task::JoinHandle<T>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .expect("the pool's semaphore is never closed");
        tokio::task::spawn_blocking(move || {
            let _core = permit;
            f()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    /// 32 CPU-bound jobs on a pool of 4 never run more than 4 at once, every
    /// one of them runs, and the ones that waited only waited.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn thirty_two_jobs_never_hold_more_permits_than_the_pool_has() {
        let pool = CpuPool::new(4);
        let (now, max) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let t0 = Instant::now();
        let jobs = (0..32).map(|i| {
            let (pool, now, max) = (pool.clone(), now.clone(), max.clone());
            async move {
                let p = pool.clone();
                let h = pool
                    .spawn(move || {
                        let n = now.fetch_add(1, Ordering::SeqCst) + 1;
                        max.fetch_max(n, Ordering::SeqCst);
                        assert!(p.busy() <= 4, "{} permits held", p.busy());
                        // CPU work, about 20 ms.
                        let until = Instant::now() + Duration::from_millis(20);
                        let mut x = i as u64;
                        while Instant::now() < until {
                            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                        }
                        now.fetch_sub(1, Ordering::SeqCst);
                        x
                    })
                    .await;
                h.await.unwrap()
            }
        });
        let out = futures_util::future::join_all(jobs).await;
        assert_eq!(out.len(), 32);
        assert_eq!(max.load(Ordering::SeqCst), 4, "4 at once, never more");
        assert_eq!(pool.busy(), 0, "every permit came back");
        // 8 rounds of 4: about 160 ms, never 32 × 20 ms.
        assert!(
            t0.elapsed() < Duration::from_millis(600),
            "{:?}",
            t0.elapsed()
        );
    }

    #[test]
    fn the_host_pool_has_a_permit_per_core() {
        let n = std::thread::available_parallelism().unwrap().get();
        assert_eq!(CpuPool::for_host().size(), n);
        assert_eq!(CpuPool::new(0).size(), 1);
    }
}
