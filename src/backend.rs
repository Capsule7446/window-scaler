use crate::{
    model::{Size, WindowId, WindowInfo},
    native::Registry,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Shrink(u32),
    Maximize,
    Hide,
    Show,
}

#[derive(Debug)]
pub struct Outcome {
    pub id: WindowId,
    pub result: Result<AppliedChange, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppliedChange {
    Shrunk(Size),
    Maximized(Size),
    RestoredOriginal(Size),
    Hidden,
    Shown,
}

impl AppliedChange {
    pub fn size(self) -> Option<Size> {
        match self {
            Self::Shrunk(size) | Self::Maximized(size) | Self::RestoredOriginal(size) => Some(size),
            Self::Hidden | Self::Shown => None,
        }
    }
}

pub enum Event {
    Windows(Result<Vec<WindowInfo>, String>),
    Batch {
        outcomes: Vec<Outcome>,
        exiting: bool,
    },
    Open,
    Exit,
}

struct Job {
    action: Action,
    exiting: bool,
    reply: mpsc::Sender<Outcome>,
}
struct Worker {
    tx: mpsc::Sender<Job>,
    busy: Arc<AtomicBool>,
    affected: Arc<AtomicBool>,
    pending_shrink: Arc<AtomicBool>,
}

impl Worker {
    fn new(id: WindowId, registry: Arc<Registry>) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<Job>();
        let busy = Arc::new(AtomicBool::new(false));
        let affected = Arc::new(AtomicBool::new(false));
        let pending_shrink = Arc::new(AtomicBool::new(false));
        let (worker_busy, worker_affected, worker_pending) =
            (busy.clone(), affected.clone(), pending_shrink.clone());
        thread::Builder::new()
            .name(format!("window-{}", id.hwnd))
            .spawn(move || {
                let mut baseline = None;
                while let Ok(job) = rx.recv() {
                    let result = (|| -> Result<AppliedChange, String> {
                        if job.exiting {
                            if registry.needs_show(id) {
                                registry.show(id)?;
                            }
                            if !worker_affected.load(Ordering::SeqCst) {
                                return Ok(AppliedChange::Shown);
                            }
                        } else if !matches!(job.action, Action::Hide | Action::Show)
                            && registry.needs_show(id)
                        {
                            registry.show(id)?;
                        }
                        let action = if job.exiting {
                            Action::Maximize
                        } else {
                            job.action
                        };
                        match action {
                            Action::Shrink(width) => registry
                                .resize(id, width, &mut baseline, &worker_affected)
                                .map(AppliedChange::Shrunk),
                            Action::Maximize => match registry.maximize(id) {
                                Ok(size) => Ok(AppliedChange::Maximized(size)),
                                Err(maximize_error) => match baseline {
                                    Some(original) => registry
                                        .restore_original(id, original, &worker_affected)
                                        .map(AppliedChange::RestoredOriginal)
                                        .map_err(|restore_error| {
                                            format!(
                                                "{maximize_error}；恢复原尺寸失败：{restore_error}"
                                            )
                                        }),
                                    None => {
                                        Err(format!("{maximize_error}；本次运行没有原尺寸记录"))
                                    }
                                },
                            },
                            Action::Hide => registry.hide(id).map(|()| AppliedChange::Hidden),
                            Action::Show => registry.show(id).map(|()| AppliedChange::Shown),
                        }
                    })();
                    worker_pending.store(false, Ordering::SeqCst);
                    worker_busy.store(false, Ordering::SeqCst);
                    let _ = job.reply.send(Outcome { id, result });
                }
            })
            .map_err(|e| format!("无法创建窗口操作线程：{e}"))?;
        Ok(Self {
            tx,
            busy,
            affected,
            pending_shrink,
        })
    }
}

pub struct Backend {
    pub registry: Arc<Registry>,
    workers: Mutex<HashMap<WindowId, Worker>>,
    events: async_channel::Sender<Event>,
}

impl Backend {
    pub fn new(events: async_channel::Sender<Event>) -> Arc<Self> {
        Arc::new(Self {
            registry: Arc::new(Registry::new()),
            workers: Default::default(),
            events,
        })
    }

    pub fn refresh(self: &Arc<Self>) {
        let this = self.clone();
        thread::spawn(move || {
            let result = this.registry.discover();
            this.workers.lock().unwrap().retain(|id, worker| {
                this.registry.valid(*id) || worker.busy.load(Ordering::SeqCst)
            });
            let _ = this.events.send_blocking(Event::Windows(result));
        });
    }

    pub fn run(self: &Arc<Self>, selected: Vec<WindowId>, action: Action, exiting: bool) {
        let this = self.clone();
        thread::spawn(move || {
            let (tx, rx) = mpsc::channel();
            let mut outcomes = Vec::new();
            let mut pending = HashSet::new();
            {
                let mut workers = this.workers.lock().unwrap();
                let targets = if exiting {
                    workers
                        .iter()
                        .filter(|(id, w)| {
                            w.affected.load(Ordering::SeqCst)
                                || w.pending_shrink.load(Ordering::SeqCst)
                                || w.busy.load(Ordering::SeqCst)
                                || this.registry.needs_show(**id)
                        })
                        .map(|(id, _)| *id)
                        .collect()
                } else {
                    selected
                };
                for id in targets {
                    if !this.registry.valid(id) {
                        if !exiting {
                            outcomes.push(Outcome {
                                id,
                                result: Err("窗口已关闭或身份改变".into()),
                            });
                        }
                        continue;
                    }
                    if let std::collections::hash_map::Entry::Vacant(entry) = workers.entry(id) {
                        match Worker::new(id, this.registry.clone()) {
                            Ok(worker) => {
                                entry.insert(worker);
                            }
                            Err(error) => {
                                outcomes.push(Outcome {
                                    id,
                                    result: Err(error),
                                });
                                continue;
                            }
                        }
                    }
                    let worker = &workers[&id];
                    if worker.busy.swap(true, Ordering::SeqCst) {
                        outcomes.push(Outcome {
                            id,
                            result: Err("先前操作尚未完成；未提交新请求，请稍后重试".into()),
                        });
                        continue;
                    }
                    // Work can finish between collecting exit targets and
                    // claiming this slot. An idle, never-modified target needs
                    // no additional operation merely to leave the app.
                    if exiting
                        && !worker.affected.load(Ordering::SeqCst)
                        && !worker.pending_shrink.load(Ordering::SeqCst)
                        && !this.registry.needs_show(id)
                    {
                        worker.busy.store(false, Ordering::SeqCst);
                        continue;
                    }
                    worker
                        .pending_shrink
                        .store(matches!(action, Action::Shrink(_)), Ordering::SeqCst);
                    if worker
                        .tx
                        .send(Job {
                            action,
                            exiting,
                            reply: tx.clone(),
                        })
                        .is_err()
                    {
                        worker.busy.store(false, Ordering::SeqCst);
                        worker.pending_shrink.store(false, Ordering::SeqCst);
                        outcomes.push(Outcome {
                            id,
                            result: Err("窗口操作线程已停止".into()),
                        });
                        continue;
                    }
                    pending.insert(id);
                }
            }
            drop(tx);
            let deadline = Instant::now() + Duration::from_secs(4);
            while !pending.is_empty() {
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    break;
                };
                match rx.recv_timeout(remaining) {
                    Ok(outcome) => {
                        pending.remove(&outcome.id);
                        outcomes.push(outcome);
                    }
                    Err(_) => break,
                }
            }
            for id in pending {
                outcomes.push(Outcome {
                    id,
                    result: Err("目标响应超时；请求可能仍在执行，退出前必须再次确认".into()),
                });
            }
            let _ = this
                .events
                .send_blocking(Event::Batch { outcomes, exiting });
        });
    }
}
