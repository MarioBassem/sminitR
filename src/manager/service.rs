use std::{
    process::{self, Command},
    sync::Arc,
    thread, time,
};

use serde::{Deserialize, Serialize};
use tokio::sync::{
    mpsc::{Receiver, Sender},
    Mutex, RwLock,
};

use super::ManagerSignal;

#[derive(Debug, Clone, Copy)]
pub enum Status {
    Pending,
    Init,
    Running,
    Successful,
    Stopped,
    Failure,
}

#[derive(Debug)]
pub enum ServiceSignal {
    Stop,
}

#[derive(Debug)]
pub struct SimpleService {
    pub name: String,
    pub pid: u32,
    pub status: Status,
    pub after: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ServiceOpts {
    #[serde(skip)]
    pub name: String,
    pub cmd: String,
    pub health_check: String,
    #[serde(default)]
    pub combine_log: bool,
    #[serde(default)]
    pub one_shot: bool,
    #[serde(default)]
    pub after: Vec<String>,
}

#[derive(Debug)]
pub struct Service {
    pub id: u64,
    pub pid: Mutex<u32>,
    pub opts: ServiceOpts,
    pub status: Arc<Mutex<Status>>,
    pub indegree: u32,
    pub healthy_parents: RwLock<u32>,
}

impl Service {
    pub async fn background_service(
        &self,
        tx: Sender<ManagerSignal>,
        mut rx: Receiver<ServiceSignal>,
    ) {
        if self.opts.one_shot {
            self.run(tx.clone()).await;
            return;
        }

        loop {
            thread::sleep(time::Duration::from_secs(1));
            match rx.try_recv() {
                Ok(sig) => {
                    match sig {
                        ServiceSignal::Stop => {
                            // this block should handle the deletion of the service
                            return;
                        }
                    };
                }
                Err(_) => {
                    // this means that there is no incoming signal from the manager, service should keep running
                    self.run(tx.clone()).await;
                    continue;
                }
            }
        }
    }

    async fn run(&self, tx: Sender<ManagerSignal>) {
        let stdout_cfg = match self.opts.combine_log {
            false => process::Stdio::null(),
            true => process::Stdio::inherit(),
        };

        let mut ch = match Command::new("sh")
            .arg("-c")
            .arg(self.opts.cmd.as_str())
            .stdout(stdout_cfg)
            .spawn()
        {
            Err(err) => {
                log::error!("failed to spawn service {}: {}", self.opts.name, err);
                return;
            }
            Ok(ch) => ch,
        };

        self.change_pid(ch.id()).await;
        self.change_status(Status::Running).await;

        // send a message to the manager to indicate that this service is healthy
        if let Err(err) = tx
            .send(ManagerSignal::Healthy(self.opts.name.clone()))
            .await
        {
            log::error!(
                "failed to send healthy signal to service manager from service {}: {}",
                self.opts.name,
                err
            );
        }

        let exit_status = ch.wait();

        // process terminated
        self.change_pid(0).await;

        let code = match exit_status {
            Err(err) => {
                log::error!("failed to wait for command: {}", err);
                self.change_status(Status::Failure).await;
                return;
            }
            Ok(exit_status) => exit_status.code(),
        };

        if code != Some(0) {
            self.change_status(Status::Failure).await;
            return;
        }

        self.change_status(Status::Successful).await;
    }

    async fn change_pid(&self, new_pid: u32) {
        let mut pid = self.pid.lock().await;
        *pid = new_pid;
    }

    pub async fn change_status(&self, new_status: Status) {
        let mut status = self.status.lock().await;
        *status = new_status;
    }

    pub async fn increase_healthy_parents(&self) {
        let mut healthy_parents = self.healthy_parents.write().await;
        *healthy_parents += 1;
        info!("healthy parents {}", *healthy_parents)
    }

    pub async fn simplify(&self) -> SimpleService {
        let mut after = Vec::new();
        for parent in &self.opts.after {
            after.push(parent.clone());
        }

        let pid = self.pid.lock().await;
        let status = self.status.lock().await;
        SimpleService {
            name: self.opts.name.clone(),
            after,
            pid: *pid,
            status: *status,
        }
    }
}
