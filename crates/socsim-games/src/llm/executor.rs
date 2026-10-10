use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use socsim_llm::{LlmClient, LlmConfig, LlmError};

use super::{CacheKeyV1, LlmReply};
use crate::RuleError;

/// Constructs one cache-free backend in the thread that will use it.
pub type BackendFactory = Arc<dyn Fn() -> Result<Box<dyn LlmClient>, LlmError> + Send + Sync>;

/// Selects whether requests are completed on the caller or worker threads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum LlmExecutor {
    #[default]
    Sequential,
    Threaded {
        workers: usize,
    },
}

/// Retry, timeout, and queue limits for [`LlmExecutor`].
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ExecutorOptions {
    pub timeout: Duration,
    pub max_backend_retries: u32,
    pub backoff_base: Duration,
    pub queue_per_worker: usize,
}

impl Default for ExecutorOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            max_backend_retries: 3,
            backoff_base: Duration::from_secs(1),
            queue_per_worker: 2,
        }
    }
}

impl LlmExecutor {
    /// Complete a batch while preserving its ordered keys in the result map.
    ///
    /// `CacheKeyV1` travels with the request so the caller can use this method
    /// directly on cache misses without rebuilding its batch representation.
    pub(crate) fn execute<K>(
        &self,
        backend: &BackendFactory,
        requests: BTreeMap<K, (CacheKeyV1, LlmConfig, String)>,
        options: &ExecutorOptions,
    ) -> BTreeMap<K, Result<LlmReply, RuleError>>
    where
        K: Clone + Ord + Send + 'static,
    {
        if requests.is_empty() {
            return BTreeMap::new();
        }
        match *self {
            Self::Sequential => execute_sequential(backend, requests, options),
            Self::Threaded { workers } => execute_threaded(backend, requests, options, workers),
        }
    }
}

fn execute_sequential<K>(
    backend: &BackendFactory,
    requests: BTreeMap<K, (CacheKeyV1, LlmConfig, String)>,
    options: &ExecutorOptions,
) -> BTreeMap<K, Result<LlmReply, RuleError>>
where
    K: Ord,
{
    let client = match backend() {
        Ok(client) => client,
        Err(error) => {
            let message = format!("failed to construct LLM backend: {error}");
            return requests
                .into_keys()
                .map(|key| (key, Err(RuleError::Backend(message.clone()))))
                .collect();
        }
    };

    requests
        .into_iter()
        .map(|(key, (_cache_key, config, prompt))| {
            let reply = complete_with_retries(client.as_ref(), &prompt, &config, options, None);
            (key, reply)
        })
        .collect()
}

struct Job<K> {
    key: K,
    _cache_key: CacheKeyV1,
    config: LlmConfig,
    prompt: String,
}

enum WorkerEvent<K> {
    Started {
        worker: usize,
        key: K,
        at: Instant,
    },
    Finished {
        worker: usize,
        key: K,
        at: Instant,
        result: Result<LlmReply, RuleError>,
    },
}

fn execute_threaded<K>(
    backend: &BackendFactory,
    requests: BTreeMap<K, (CacheKeyV1, LlmConfig, String)>,
    options: &ExecutorOptions,
    workers: usize,
) -> BTreeMap<K, Result<LlmReply, RuleError>>
where
    K: Clone + Ord + Send + 'static,
{
    if workers == 0 {
        return backend_errors(
            requests,
            "threaded LLM executor requires at least one worker",
        );
    }
    if options.queue_per_worker == 0 {
        return backend_errors(
            requests,
            "threaded LLM executor queue_per_worker must be greater than zero",
        );
    }
    let Some(queue_capacity) = workers.checked_mul(options.queue_per_worker) else {
        return backend_errors(requests, "threaded LLM executor queue capacity overflowed");
    };
    let total = requests.len();
    if total == 0 {
        return BTreeMap::new();
    }

    let mut pending: VecDeque<_> = requests
        .into_iter()
        .map(|(key, (cache_key, config, prompt))| Job {
            key,
            _cache_key: cache_key,
            config,
            prompt,
        })
        .collect();
    let (job_tx, job_rx) = mpsc::sync_channel(queue_capacity);
    let job_rx = Arc::new(Mutex::new(job_rx));
    let (event_tx, event_rx) = mpsc::channel();
    let mut next_worker = 0;
    let mut stop_flags = HashMap::new();
    for _ in 0..workers {
        spawn_worker(
            &mut next_worker,
            backend,
            options,
            &job_rx,
            &event_tx,
            &mut stop_flags,
        );
    }

    let mut results = BTreeMap::new();
    let mut active: BTreeMap<K, (usize, Instant)> = BTreeMap::new();

    while results.len() < total {
        pump_pending(&job_tx, &mut pending);

        let next_deadline = active.values().map(|(_, deadline)| *deadline).min();
        let event = match next_deadline {
            Some(deadline) => {
                match event_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(event) => Some(event),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            None => event_rx.recv().ok(),
        };

        if let Some(event) = event {
            match event {
                WorkerEvent::Started { worker, key, at } => {
                    if !results.contains_key(&key) {
                        active.insert(key, (worker, at + options.timeout));
                    }
                }
                WorkerEvent::Finished {
                    worker,
                    key,
                    at,
                    result,
                } => {
                    if let Some((_, deadline)) = active.remove(&key) {
                        if at <= deadline {
                            results.insert(key, result);
                        } else {
                            results.insert(key, Err(timeout_error(options.timeout)));
                            stop_worker(worker, &stop_flags);
                            if results.len() < total {
                                spawn_worker(
                                    &mut next_worker,
                                    backend,
                                    options,
                                    &job_rx,
                                    &event_tx,
                                    &mut stop_flags,
                                );
                            }
                        }
                    }
                }
            }
        }

        let now = Instant::now();
        let timed_out: Vec<_> = active
            .iter()
            .filter(|(_, (_, deadline))| *deadline <= now)
            .map(|(key, (worker, _))| (key.clone(), *worker))
            .collect();
        for (key, worker) in timed_out {
            active.remove(&key);
            if results
                .insert(key, Err(timeout_error(options.timeout)))
                .is_none()
            {
                stop_worker(worker, &stop_flags);
                if results.len() < total {
                    spawn_worker(
                        &mut next_worker,
                        backend,
                        options,
                        &job_rx,
                        &event_tx,
                        &mut stop_flags,
                    );
                }
            }
        }
    }

    results
}

fn backend_errors<K: Ord>(
    requests: BTreeMap<K, (CacheKeyV1, LlmConfig, String)>,
    message: &str,
) -> BTreeMap<K, Result<LlmReply, RuleError>> {
    requests
        .into_keys()
        .map(|key| (key, Err(RuleError::Backend(message.to_string()))))
        .collect()
}

fn pump_pending<K>(sender: &SyncSender<Job<K>>, pending: &mut VecDeque<Job<K>>) {
    while let Some(job) = pending.pop_front() {
        match sender.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(job)) => {
                pending.push_front(job);
                break;
            }
            Err(TrySendError::Disconnected(job)) => {
                pending.push_front(job);
                break;
            }
        }
    }
}

fn spawn_worker<K>(
    next_worker: &mut usize,
    backend: &BackendFactory,
    options: &ExecutorOptions,
    jobs: &Arc<Mutex<Receiver<Job<K>>>>,
    events: &mpsc::Sender<WorkerEvent<K>>,
    stop_flags: &mut HashMap<usize, Arc<AtomicBool>>,
) where
    K: Clone + Send + 'static,
{
    let worker = *next_worker;
    *next_worker = next_worker.wrapping_add(1);
    let stop = Arc::new(AtomicBool::new(false));
    stop_flags.insert(worker, Arc::clone(&stop));
    let backend = Arc::clone(backend);
    let options = options.clone();
    let jobs = Arc::clone(jobs);
    let events = events.clone();

    thread::spawn(move || {
        let client = backend().map_err(|error| {
            format!("failed to construct LLM backend in worker {worker}: {error}")
        });
        loop {
            let job = {
                let Ok(receiver) = jobs.lock() else {
                    return;
                };
                match receiver.recv() {
                    Ok(job) => job,
                    Err(_) => return,
                }
            };
            let started_at = Instant::now();
            if events
                .send(WorkerEvent::Started {
                    worker,
                    key: job.key.clone(),
                    at: started_at,
                })
                .is_err()
            {
                return;
            }

            let result = match &client {
                Ok(client) => complete_with_retries(
                    client.as_ref(),
                    &job.prompt,
                    &job.config,
                    &options,
                    Some(stop.as_ref()),
                ),
                Err(message) => Err(RuleError::Backend(message.clone())),
            };
            if events
                .send(WorkerEvent::Finished {
                    worker,
                    key: job.key,
                    at: Instant::now(),
                    result,
                })
                .is_err()
            {
                return;
            }
            if stop.load(Ordering::Acquire) {
                return;
            }
        }
    });
}

fn stop_worker(worker: usize, stop_flags: &HashMap<usize, Arc<AtomicBool>>) {
    if let Some(stop) = stop_flags.get(&worker) {
        stop.store(true, Ordering::Release);
    }
}

fn complete_with_retries(
    client: &dyn LlmClient,
    prompt: &str,
    config: &LlmConfig,
    options: &ExecutorOptions,
    stop: Option<&AtomicBool>,
) -> Result<LlmReply, RuleError> {
    let mut retries = 0;
    loop {
        match client.complete(prompt, config) {
            Ok(response) => {
                return Ok(LlmReply {
                    text: response.text,
                    thinking: response.thinking,
                    usage: response.metadata.usage,
                    cache_hit: response.metadata.cache_hit,
                });
            }
            Err(_error) if retries < options.max_backend_retries => {
                if stop.is_some_and(|stop| stop.load(Ordering::Acquire)) {
                    return Err(timeout_error(options.timeout));
                }
                let multiplier = 1_u32.checked_shl(retries).unwrap_or(u32::MAX);
                thread::sleep(options.backoff_base.saturating_mul(multiplier));
                if stop.is_some_and(|stop| stop.load(Ordering::Acquire)) {
                    return Err(timeout_error(options.timeout));
                }
                retries += 1;
            }
            Err(error) => {
                return Err(RuleError::Backend(format!(
                    "LLM backend failed after {} attempt(s): {error}",
                    retries + 1
                )));
            }
        }
    }
}

fn timeout_error(timeout: Duration) -> RuleError {
    RuleError::Backend(format!(
        "LLM backend call timed out after {} ms",
        timeout.as_millis()
    ))
}

/// Construct the canonical cache-free live backend inside each executor worker.
#[cfg(feature = "live")]
pub fn live_backend_factory() -> BackendFactory {
    Arc::new(socsim_llm::build_live_backend)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::BackendIdentity;
    use socsim_llm::{LlmError, LlmResponse};
    use std::sync::atomic::AtomicUsize;

    fn job(key: usize) -> Job<usize> {
        Job {
            key,
            _cache_key: CacheKeyV1 {
                version: 1,
                backend: BackendIdentity {
                    model: "test".to_owned(),
                    endpoint: "mock://queue".to_owned(),
                },
                system: None,
                prompt: format!("request-{key}"),
                temperature_bits: 0,
                seed: 0,
                omit_seed: false,
                max_tokens: None,
                think: None,
                allow_blank: false,
            },
            config: LlmConfig::deterministic(),
            prompt: format!("request-{key}"),
        }
    }

    #[test]
    fn pending_jobs_stop_at_the_sync_channel_capacity() {
        let (sender, receiver) = mpsc::sync_channel(2);
        let mut pending = (0..5).map(job).collect();

        pump_pending(&sender, &mut pending);

        assert_eq!(pending.len(), 3);
        assert_eq!(receiver.try_recv().unwrap().key, 0);
        assert_eq!(receiver.try_recv().unwrap().key, 1);
        assert!(receiver.try_recv().is_err());
    }

    struct FailingClient {
        calls: Arc<AtomicUsize>,
        delay: Duration,
    }

    impl LlmClient for FailingClient {
        fn model(&self) -> &str {
            "failing"
        }

        fn endpoint(&self) -> &str {
            "mock://failing"
        }

        fn complete(&self, _prompt: &str, _config: &LlmConfig) -> Result<LlmResponse, LlmError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            thread::sleep(self.delay);
            Err(LlmError::Transport {
                endpoint: self.endpoint().to_owned(),
                message: "expected failure".to_owned(),
            })
        }
    }

    fn request() -> BTreeMap<usize, (CacheKeyV1, LlmConfig, String)> {
        let value = job(0);
        BTreeMap::from([(0, (value._cache_key, value.config, value.prompt))])
    }

    #[test]
    fn backend_failures_retry_only_to_the_configured_limit() {
        let calls = Arc::new(AtomicUsize::new(0));
        let client = FailingClient {
            calls: Arc::clone(&calls),
            delay: Duration::ZERO,
        };
        let options = ExecutorOptions {
            max_backend_retries: 2,
            backoff_base: Duration::ZERO,
            ..ExecutorOptions::default()
        };

        assert!(
            complete_with_retries(&client, "p", &LlmConfig::default(), &options, None).is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn threaded_timeout_does_not_resend_the_timed_out_call() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let backend: BackendFactory = Arc::new(move || {
            Ok(Box::new(FailingClient {
                calls: Arc::clone(&observed),
                delay: Duration::from_millis(100),
            }))
        });
        let options = ExecutorOptions {
            timeout: Duration::from_millis(10),
            max_backend_retries: 3,
            backoff_base: Duration::ZERO,
            ..ExecutorOptions::default()
        };
        let result = LlmExecutor::Threaded { workers: 1 }.execute(&backend, request(), &options);
        assert!(result[&0].is_err());
        thread::sleep(Duration::from_millis(150));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn zero_queue_capacity_is_a_backend_error_instead_of_a_deadlock() {
        let backend: BackendFactory = Arc::new(|| unreachable!("backend must not be built"));
        let options = ExecutorOptions {
            queue_per_worker: 0,
            ..ExecutorOptions::default()
        };
        let result = LlmExecutor::Threaded { workers: 1 }.execute(&backend, request(), &options);
        assert!(matches!(result[&0], Err(RuleError::Backend(_))));
    }
}
