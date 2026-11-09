use crate::{
    layout::{self, Position, Rect, TilingOptions},
    model::{Size, WindowId, WindowInfo},
    native::{Registry, TilingSnapshot},
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
    Tile {
        expected: TilingSnapshot,
        target: Position,
    },
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
    Tiled(Size),
}

impl AppliedChange {
    pub fn size(self) -> Option<Size> {
        match self {
            Self::Shrunk(size)
            | Self::Maximized(size)
            | Self::RestoredOriginal(size)
            | Self::Tiled(size) => Some(size),
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
                        } else if matches!(job.action, Action::Shrink(_) | Action::Maximize)
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
                            Action::Tile { expected, target } => registry
                                .move_tiled(id, expected, target)
                                .map(AppliedChange::Tiled),
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

fn claim_worker<'a>(
    workers: &'a mut HashMap<WindowId, Worker>,
    registry: &Arc<Registry>,
    id: WindowId,
) -> Result<&'a Worker, String> {
    registry.require(id)?;
    if let std::collections::hash_map::Entry::Vacant(entry) = workers.entry(id) {
        entry.insert(Worker::new(id, registry.clone())?);
    }
    let worker = &workers[&id];
    if worker.busy.swap(true, Ordering::SeqCst) {
        return Err("先前操作尚未完成；未提交新请求，请稍后重试".into());
    }
    Ok(worker)
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

    pub fn tile(self: &Arc<Self>, selected: Vec<WindowId>, area: Rect, options: TilingOptions) {
        let this = self.clone();
        thread::spawn(move || {
            let (tx, rx) = mpsc::channel();
            let mut outcomes = Vec::new();
            let mut pending = HashSet::new();
            {
                // Reserve each worker through measurement, planning and dispatch.
                // No moves are sent until the entire layout fits.
                let mut workers = this.workers.lock().unwrap();
                let mut measured = Vec::new();
                let mut seen = HashSet::new();
                for id in selected.into_iter().filter(|id| seen.insert(*id)) {
                    let result =
                        claim_worker(&mut workers, &this.registry, id).and_then(|worker| {
                            let result = this.registry.measure_tiling(id);
                            if result.is_err() {
                                worker.busy.store(false, Ordering::SeqCst);
                            }
                            result
                        });
                    match result {
                        Ok(snapshot) => measured.push((id, snapshot)),
                        Err(error) => outcomes.push(Outcome {
                            id,
                            result: Err(error),
                        }),
                    }
                }
                let sizes = measured
                    .iter()
                    .map(|(_, s)| s.frame.size())
                    .collect::<Result<Vec<_>, _>>();
                match sizes.and_then(|sizes| layout::plan(area, &sizes, options)) {
                    Ok(positions) => {
                        for ((id, expected), target) in measured.into_iter().zip(positions) {
                            let worker = &workers[&id];
                            if worker
                                .tx
                                .send(Job {
                                    action: Action::Tile { expected, target },
                                    exiting: false,
                                    reply: tx.clone(),
                                })
                                .is_err()
                            {
                                worker.busy.store(false, Ordering::SeqCst);
                                outcomes.push(Outcome {
                                    id,
                                    result: Err("窗口操作线程已停止".into()),
                                });
                            } else {
                                pending.insert(id);
                            }
                        }
                    }
                    Err(error) => {
                        for (id, _) in measured {
                            workers[&id].busy.store(false, Ordering::SeqCst);
                            outcomes.push(Outcome {
                                id,
                                result: Err(error.clone()),
                            });
                        }
                    }
                }
            }
            drop(tx);
            this.finish_batch(rx, pending, outcomes, false);
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
                    let worker = match claim_worker(&mut workers, &this.registry, id) {
                        Ok(worker) => worker,
                        Err(error) => {
                            outcomes.push(Outcome {
                                id,
                                result: Err(error),
                            });
                            continue;
                        }
                    };
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
            this.finish_batch(rx, pending, outcomes, exiting);
        });
    }

    fn finish_batch(
        &self,
        rx: mpsc::Receiver<Outcome>,
        mut pending: HashSet<WindowId>,
        mut outcomes: Vec<Outcome>,
        exiting: bool,
    ) {
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
        let _ = self
            .events
            .send_blocking(Event::Batch { outcomes, exiting });
    }
}
